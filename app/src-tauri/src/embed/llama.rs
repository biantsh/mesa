use std::{
    collections::hash_map::RandomState,
    fs::{self, File},
    hash::{BuildHasher, Hasher},
    io::{Read, Seek, SeekFrom},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

use super::{
    Device, Error,
    model::{self, ModelState, ModelStatus},
    pipeline::Backend,
    prepare::Prepared,
    prime,
};
use crate::index::lock;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

pub const TEXT_MODEL: &str = "embeddinggemma-2-Q8_0.gguf";
pub const PICTURE_MODEL: &str = "mmproj-embeddinggemma-2-Q8_0.gguf";
const SERVER: &str = if cfg!(windows) {
    "llama-server.exe"
} else {
    "llama-server"
};

const SLOTS: usize = 8;
const TOKENS_PER_SLOT: usize = 2048;
const START_TIMEOUT: Duration = Duration::from_secs(180);
const PORT_TRIES: usize = 3;
const PORT_TAKEN: &str = "couldn't bind HTTP server socket";

#[derive(Clone, Debug)]
pub struct Locations {
    pub runtime: PathBuf,
    pub models: PathBuf,
    pub log: PathBuf,
}

impl Locations {
    pub fn new(data_dir: &Path, resource_dir: Option<&Path>) -> Self {
        let from_env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        let built = Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime").join("llama");
        let mut candidates: Vec<PathBuf> = Vec::new();
        if cfg!(all(debug_assertions, target_os = "linux")) {
            candidates.push(built.clone());
        }
        candidates.extend(resource_dir.map(|dir| dir.join("llama")));
        candidates.extend(
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join("llama"))),
        );
        if cfg!(any(debug_assertions, test)) && !candidates.contains(&built) {
            candidates.push(built);
        }
        let runtime = from_env("MESA_LLAMA_DIR")
            .or_else(|| choose_runtime(&candidates))
            .unwrap_or_else(|| data_dir.join("llama"));
        Self {
            runtime,
            models: from_env("MESA_MODELS_DIR").unwrap_or_else(|| data_dir.join("models").join("embeddinggemma-2")),
            log: data_dir.join("logs").join("llama-server.log"),
        }
    }

    fn server(&self) -> PathBuf {
        self.runtime.join(SERVER)
    }

    pub(super) fn missing(&self) -> Vec<PathBuf> {
        [
            self.server(),
            self.models.join(TEXT_MODEL),
            self.models.join(PICTURE_MODEL),
        ]
        .into_iter()
        .filter(|path| !path.is_file())
        .collect()
    }

    fn problem(&self) -> Option<String> {
        let missing = self.missing();
        if missing.is_empty() {
            return None;
        }
        let list: Vec<_> = missing.iter().map(|path| path.display().to_string()).collect();
        let mut advice = Vec::new();
        if missing.contains(&self.server()) {
            advice.push(if cfg!(target_os = "macos") {
                "The program is built with the app: run `npm run build:runtime` in app/ (it needs Xcode's command line tools and CMake)."
            } else {
                "The program is built with the app: run `npm run build:runtime` in app/ (it needs the Vulkan SDK, or --backend=cpu)."
            });
        }
        if missing.iter().any(|path| path.starts_with(&self.models)) {
            advice.push("Let Mesa download the model, or put its .gguf files in that folder.");
        }
        Some(format!(
            "The embedding model isn't set up; missing: {}. {}",
            list.join(", "),
            advice.join(" ")
        ))
    }
}

fn choose_runtime(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|dir| dir.join(SERVER).is_file())
        .or_else(|| candidates.first())
        .cloned()
}

#[derive(Clone)]
struct Connection {
    agent: ureq::Agent,
    url: String,
    key: String,
    generation: u64,
}

struct Running {
    child: Option<Child>,
    connection: Connection,
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub struct LlamaServer {
    locations: Locations,
    device: Mutex<Device>,
    running: Mutex<Option<Running>>,
    generation: AtomicU64,
    gpu: OnceLock<Option<FoundGpu>>,
    model: Arc<ModelState>,
    source: Option<model::Sources>,
    files: Vec<model::ModelFile>,
    more: Vec<model::Group>,
    files_ok: AtomicBool,
    setting_up: Mutex<()>,
}

#[cfg(test)]
pub(super) static RETRIED: AtomicUsize = AtomicUsize::new(0);
const RETRIES_AFTER_A_CHANGE: usize = 3;

impl LlamaServer {
    pub fn new(locations: Locations) -> Self {
        Self {
            locations,
            device: Mutex::new(Device::Auto),
            running: Mutex::new(None),
            generation: AtomicU64::new(0),
            gpu: OnceLock::new(),
            model: Arc::new(ModelState::new(ModelStatus::Starting)),
            source: None,
            files: model::FILES.to_vec(),
            more: Vec::new(),
            files_ok: AtomicBool::new(false),
            setting_up: Mutex::new(()),
        }
    }

    pub fn download_from(mut self, sources: model::Sources) -> Self {
        self.source = Some(sources);
        self.model.set(model::initial_status_of(&self.groups()));
        self
    }

    pub fn download_too(mut self, group: model::Group) -> Self {
        self.more.push(group);
        if self.source.is_some() {
            self.model.set(model::initial_status_of(&self.groups()));
        }
        self
    }

    fn groups(&self) -> Vec<model::Group> {
        let Some(sources) = &self.source else { return Vec::new() };
        let mut groups = self.more.clone();
        groups.push(model::Group {
            dir: self.locations.models.clone(),
            files: self.files.clone(),
            sources: sources.clone(),
            optional: false,
        });
        groups
    }

    fn ensure_model(&self) -> Result<(), Error> {
        if self.source.is_none() {
            return Ok(());
        }
        if self.files_ok.load(Ordering::Acquire) {
            return Ok(());
        }
        let _turn = lock(&self.setting_up);
        if self.files_ok.load(Ordering::Acquire) {
            return Ok(());
        }
        if let ModelStatus::Failed { reason, setup: true } = self.model.get() {
            return Err(Error(reason));
        }
        let getting = matches!(
            self.model.get(),
            ModelStatus::Downloading { .. } | ModelStatus::Verifying
        );
        match model::ensure_all(&self.groups(), &self.model) {
            Ok(()) => {
                if getting {
                    crate::estimate::before_model_starts(&self.model);
                }
                self.files_ok.store(true, Ordering::Release);
                self.model.set(ModelStatus::Starting);
                Ok(())
            }
            Err(reason) => {
                self.model.set(ModelStatus::Failed {
                    reason: reason.clone(),
                    setup: true,
                });
                Err(Error(reason))
            }
        }
    }

    pub fn choose_device(&self, device: Device) {
        *lock(&self.device) = device;
    }

    fn connection(&self) -> Result<Connection, Error> {
        self.ensure_model()?;
        let mut running = lock(&self.running);
        if let Some(server) = running.as_mut() {
            let alive = server
                .child
                .as_mut()
                .is_none_or(|child| matches!(child.try_wait(), Ok(None)));
            if alive {
                return Ok(server.connection.clone());
            }
            *running = None;
        }
        let (server, used) = start_or_fall_back(*lock(&self.device), |device| self.launch_new(device))?;
        *lock(&self.device) = used;
        let connection = server.connection.clone();
        *running = Some(server);
        Ok(connection)
    }

    fn found_gpu(&self) -> Option<FoundGpu> {
        self.gpu.get_or_init(|| find_gpu(&self.locations)).clone()
    }

    fn launch_new(&self, device: Device) -> Result<Running, Error> {
        let device = if self.shared_gpu().is_some() {
            Device::Cpu
        } else {
            device
        };
        let gpu = self.found_gpu();
        let pinned = gpu.as_ref().filter(|gpu| gpu.pinned).map(|gpu| gpu.id.as_str());
        self.model.set(ModelStatus::Starting);
        let mut server = launch(&self.locations, device, pinned).inspect_err(|e| {
            self.model.set(ModelStatus::Failed {
                reason: e.0.clone(),
                setup: false,
            });
        })?;
        server.connection.generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        #[cfg(windows)]
        if let Some(child) = &server.child {
            crate::power::never_throttle(child);
        }
        if device == Device::Auto && self.gpu().is_some() {
            self.model.set(ModelStatus::Preparing);
            prime_in_the_background(server.connection.clone(), self.model.clone());
        } else {
            self.model.set(ModelStatus::Ready);
        }
        Ok(server)
    }

    fn lost(&self, generation: u64) {
        let mut running = lock(&self.running);
        if running
            .as_ref()
            .is_some_and(|server| server.connection.generation == generation)
        {
            *running = None;
        }
    }

    fn attempt(&self, batch: &[&Prepared]) -> Result<Vec<Result<Vec<f32>, String>>, Error> {
        let connection = self.connection()?;
        match self.post(&connection, batch)? {
            Ok(vectors) => Ok(vectors.into_iter().map(Ok).collect()),
            Err(_) if batch.len() > 1 => batch
                .iter()
                .map(|one| {
                    Ok(match self.post(&connection, &[*one])? {
                        Ok(mut vectors) => Ok(vectors.remove(0)),
                        Err(why) => Err(why),
                    })
                })
                .collect(),
            Err(why) => Ok(vec![Err(why)]),
        }
    }

    fn post(&self, connection: &Connection, batch: &[&Prepared]) -> Result<Result<Vec<Vec<f32>>, String>, Error> {
        request(connection, batch).map_err(|what| {
            self.lost(connection.generation);
            Error(format!("lost contact with the embedding server: {what}"))
        })
    }
}

fn request(connection: &Connection, batch: &[&Prepared]) -> Result<Result<Vec<Vec<f32>>, String>, String> {
    let inputs: Vec<Value> = batch
        .iter()
        .map(|prepared| match prepared {
            Prepared::Image(bytes) => json!({"content": [{
                "type": "image_url",
                "image_url": {"url": format!("data:image/bmp;base64,{}", STANDARD.encode(bytes))},
            }]}),
            Prepared::Audio(bytes) => json!({"content": [{
                "type": "input_audio",
                "input_audio": {"data": STANDARD.encode(bytes), "format": "wav"},
            }]}),
            Prepared::Text(text) => json!({"content": [{"type": "text", "text": text}]}),
        })
        .collect();
    let body = json!({"input": inputs, "encoding_format": "float"}).to_string();

    let mut response = connection
        .agent
        .post(&format!("{}/v1/embeddings", connection.url))
        .header("Authorization", &format!("Bearer {}", connection.key))
        .header("Content-Type", "application/json")
        .send(body.as_bytes())
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let reply = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if status != 200 {
        return Ok(Err(format!("the model turned it down ({status}): {}", brief(&reply))));
    }
    parse_embeddings(&reply, batch.len()).map(Ok)
}

fn prime_in_the_background(connection: Connection, model: Arc<ModelState>) {
    let spawned = thread::Builder::new().name("embedder-prime".into()).spawn({
        let model = model.clone();
        move || {
            let started = Instant::now();
            for batch in prime::batches() {
                let batch: Vec<&Prepared> = batch.iter().collect();
                if request(&connection, &batch).is_err() {
                    break;
                }
            }
            eprintln!("Mesa: the model is primed ({} ms)", started.elapsed().as_millis());
            model.finish_preparing();
        }
    });
    if spawned.is_err() {
        model.finish_preparing();
    }
}

impl Backend for LlamaServer {
    fn start(&self) -> Result<(), Error> {
        self.connection().map(drop)
    }

    fn embed_batch(&self, batch: &[&Prepared]) -> Result<Vec<Result<Vec<f32>, String>>, Error> {
        for _ in 0..RETRIES_AFTER_A_CHANGE {
            let before = self.generation.load(Ordering::SeqCst);
            match self.attempt(batch) {
                Err(_) if self.generation.load(Ordering::SeqCst) != before => {
                    #[cfg(test)]
                    RETRIED.fetch_add(1, Ordering::SeqCst);
                    continue;
                }
                result => return result,
            }
        }
        self.attempt(batch)
    }

    fn stop(&self) {
        *lock(&self.running) = None;
    }

    fn set_device(&self, device: Device) -> Result<(), Error> {
        let mut running = lock(&self.running);
        let mut current = lock(&self.device);
        let alive = running.as_mut().is_some_and(|server| {
            server
                .child
                .as_mut()
                .is_none_or(|child| matches!(child.try_wait(), Ok(None)))
        });
        if *current == device && alive {
            return Ok(());
        }
        *current = device;
        self.generation.fetch_add(1, Ordering::SeqCst);
        *running = None;
        *running = Some(self.launch_new(device)?);
        Ok(())
    }

    fn gpu(&self) -> Option<String> {
        self.found_gpu()
            .filter(|gpu| !gpu.integrated || gpu.pinned)
            .map(|gpu| gpu.name)
    }

    fn shared_gpu(&self) -> Option<String> {
        self.found_gpu()
            .filter(|gpu| gpu.integrated && !gpu.pinned)
            .map(|gpu| gpu.name)
    }

    fn model_status(&self) -> ModelStatus {
        self.model.get()
    }

    fn retry_setup(&self) -> bool {
        match self.model.get() {
            ModelStatus::Failed { setup: true, .. } if self.source.is_some() => {
                self.model.set(model::initial_status_of(&self.groups()));
            }
            ModelStatus::Failed { setup: false, .. } => self.model.set(ModelStatus::Starting),
            _ => return false,
        }
        true
    }

    fn max_batch(&self) -> usize {
        SLOTS
    }
}

fn start_or_fall_back<T>(
    device: Device,
    mut start: impl FnMut(Device) -> Result<T, Error>,
) -> Result<(T, Device), Error> {
    match start(device) {
        Ok(server) => Ok((server, device)),
        Err(first) if device == Device::Auto => {
            eprintln!("Mesa: the embedding model couldn't start on the GPU, so the processor is used: {first}");
            start(Device::Cpu)
                .map(|server| (server, Device::Cpu))
                .map_err(|_| first)
        }
        Err(e) => Err(e),
    }
}

fn device_args(device: Device, pinned: Option<&str>) -> Vec<String> {
    match device {
        Device::Auto => {
            let mut args = Vec::with_capacity(4);
            if let Some(id) = pinned {
                args.extend(["-dev".to_owned(), id.to_owned()]);
            }
            args.extend(["-ngl".to_owned(), "all".to_owned()]);
            args
        }
        Device::Cpu => ["-dev", "none", "-ngl", "0"].map(str::to_owned).to_vec(),
    }
}

#[derive(Clone, Debug, PartialEq)]
struct FoundGpu {
    id: String,
    name: String,
    integrated: bool,
    /// Chosen with `MESA_LLAMA_DEVICE` instead of picked automatically, so `-dev` is passed to
    /// llama.cpp to make it use this one.
    pinned: bool,
}

fn find_gpu(locations: &Locations) -> Option<FoundGpu> {
    let mut command = Command::new(locations.server());
    command
        .current_dir(&locations.runtime)
        .arg("--list-devices")
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command.output().ok()?;
    let gpus: Vec<FoundGpu> = listed_devices(&String::from_utf8_lossy(&output.stdout))
        .into_iter()
        .map(|device| FoundGpu {
            integrated: is_integrated(&device.name),
            name: tidy_gpu_name(&device.name),
            id: device.id,
            pinned: false,
        })
        .collect();
    let wanted = std::env::var("MESA_LLAMA_DEVICE").ok();
    let (chosen, pinned) = choose(&gpus, wanted.as_deref())?;
    let mut gpu = chosen.clone();
    gpu.pinned = pinned;
    Some(gpu)
}

/// Picks the GPU to tell the user about. A discrete GPU is preferred over an integrated one, and
/// `MESA_LLAMA_DEVICE` (a device id like `Vulkan1`, an index, or part of a name) wins
/// over both.
fn choose<'a>(gpus: &'a [FoundGpu], wanted: Option<&str>) -> Option<(&'a FoundGpu, bool)> {
    if let Some(wanted) = wanted.map(str::trim).filter(|wanted| !wanted.is_empty()) {
        let found = gpus
            .iter()
            .find(|gpu| gpu.id.eq_ignore_ascii_case(wanted))
            .or_else(|| wanted.parse::<usize>().ok().and_then(|index| gpus.get(index)))
            .or_else(|| {
                let wanted = wanted.to_lowercase();
                gpus.iter().find(|gpu| gpu.name.to_lowercase().contains(&wanted))
            });
        if let Some(found) = found {
            return Some((found, true));
        }
        eprintln!("Mesa: no GPU matches MESA_LLAMA_DEVICE={wanted}; picking one automatically");
    }
    gpus.iter()
        .find(|gpu| !gpu.integrated)
        .or_else(|| gpus.first())
        .map(|gpu| (gpu, false))
}

#[cfg(any(target_os = "linux", windows))]
fn is_integrated(name: &str) -> bool {
    use ash::vk;
    unsafe {
        let Ok(entry) = ash::Entry::load() else { return false };
        let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_0);
        let Ok(instance) = entry.create_instance(&vk::InstanceCreateInfo::default().application_info(&app), None)
        else {
            return false;
        };
        let integrated = instance
            .enumerate_physical_devices()
            .unwrap_or_default()
            .into_iter()
            .find_map(|device| {
                let properties = instance.get_physical_device_properties(device);
                let device_name = properties.device_name_as_c_str().ok()?.to_str().ok()?;
                same_gpu(device_name, name).then_some(properties.device_type == vk::PhysicalDeviceType::INTEGRATED_GPU)
            });
        instance.destroy_instance(None);
        integrated.unwrap_or(false)
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
fn is_integrated(_name: &str) -> bool {
    false
}

/// The same card is spelled differently by each backend (`Arc(tm)` against `Arc(TM)`, or with the
/// chip's code added, `... A770 Graphics (DG2)`), so compare names by their letters and digits only.
fn same_gpu(a: &str, b: &str) -> bool {
    let (a, b) = (gpu_key(a), gpu_key(b));
    a == b || (a.len() >= 12 && b.len() >= 12 && (a.starts_with(&b) || b.starts_with(&a)))
}

fn gpu_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ListedDevice {
    id: String,
    name: String,
}

fn listed_devices(listing: &str) -> Vec<ListedDevice> {
    listing
        .lines()
        .skip_while(|line| !line.trim().starts_with("Available devices"))
        .skip(1)
        .filter_map(|line| {
            let (id, rest) = line.trim().split_once(": ")?;
            if id.is_empty() || id.starts_with("CPU") || id.starts_with("BLAS") || id.contains(char::is_whitespace) {
                return None;
            }
            let name = rest.rfind(" (").map_or(rest, |end| &rest[..end]).trim();
            (!name.is_empty()).then(|| ListedDevice {
                id: id.to_owned(),
                name: name.to_owned(),
            })
        })
        .collect()
}

fn tidy_gpu_name(name: &str) -> String {
    let Some((product, chip)) = name.strip_suffix(')').and_then(|rest| rest.rsplit_once(" (RADV ")) else {
        return name.to_owned();
    };
    let known: Vec<&str> = product.split_whitespace().filter(|word| *word != "Unknown").collect();
    let is_named = product.split_whitespace().count() == known.len() && !known.is_empty();
    if is_named {
        return known.join(" ");
    }
    known
        .into_iter()
        .chain([chip.trim()])
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_embeddings(reply: &str, expected: usize) -> Result<Vec<Vec<f32>>, String> {
    let reply: Value = serde_json::from_str(reply).map_err(|e| format!("unreadable reply: {e}"))?;
    let data = reply["data"].as_array().ok_or("the reply has no data")?;
    if data.len() != expected {
        return Err(format!("{} vectors came back for {expected} inputs", data.len()));
    }
    let mut vectors = vec![Vec::new(); expected];
    for entry in data {
        let index = entry["index"].as_u64().ok_or("a vector has no index")? as usize;
        let numbers = entry["embedding"].as_array().ok_or("a vector has no numbers")?;
        let slot = vectors.get_mut(index).ok_or("a vector has an index out of range")?;
        *slot = numbers.iter().map(|n| n.as_f64().unwrap_or(f64::NAN) as f32).collect();
    }
    Ok(vectors)
}

fn brief(text: &str) -> String {
    let text = text.trim();
    match text.char_indices().nth(200) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.to_owned(),
    }
}

fn server_threads() -> usize {
    thread::available_parallelism().map_or(4, |n| n.get()).min(8)
}

enum Start {
    PortTaken(Error),
    Failed(Error),
}

impl From<Error> for Start {
    fn from(error: Error) -> Self {
        Start::Failed(error)
    }
}

fn launch(locations: &Locations, device: Device, pinned: Option<&str>) -> Result<Running, Error> {
    if let Some(problem) = locations.problem() {
        return Err(Error(problem));
    }
    let mut tries = 1;
    loop {
        match start(locations, device, pinned) {
            Ok(server) => return Ok(server),
            Err(Start::PortTaken(_)) if tries < PORT_TRIES => tries += 1,
            Err(Start::PortTaken(error) | Start::Failed(error)) => return Err(error),
        }
    }
}

fn start(locations: &Locations, device: Device, pinned: Option<&str>) -> Result<Running, Start> {
    let port = TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map_err(|e| Error(format!("no free port for the embedding server: {e}")))?
        .port();
    let key = secret();

    if let Some(folder) = locations.log.parent() {
        let _ = fs::create_dir_all(folder);
    }
    let log =
        File::create(&locations.log).map_err(|e| Error(format!("can't write {}: {e}", locations.log.display())))?;
    let log_for_errors = log.try_clone().map_err(|e| Error(e.to_string()))?;

    let threads = server_threads().to_string();
    let mut command = Command::new(locations.server());
    command
        .current_dir(&locations.runtime)
        .args(["-m"])
        .arg(locations.models.join(TEXT_MODEL))
        .arg("--mmproj")
        .arg(locations.models.join(PICTURE_MODEL))
        .args(["--embedding", "--pooling", "mean"])
        .args(device_args(device, pinned))
        .args(["-np", &SLOTS.to_string(), "-c", &(SLOTS * TOKENS_PER_SLOT).to_string()])
        .args(["-b", &TOKENS_PER_SLOT.to_string(), "-ub", &TOKENS_PER_SLOT.to_string()])
        .args(["--image-min-tokens", "1", "--image-max-tokens", "1024"])
        .args(["-t", &threads, "-tb", &threads])
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .env("LLAMA_API_KEY", &key)
        .args(["--no-webui", "-lv", "2"])
        // Skip the fit check and the warm-up run: together they cost 0.4 s of a 1.6 s start.
        .args(["--fit", "off", "--no-warmup"])
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_for_errors);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        command.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: only `setpriority` is called between the fork and the exec, which is safe there.
        unsafe {
            command.pre_exec(|| {
                libc::setpriority(libc::PRIO_PROCESS, 0, 5);
                Ok(())
            });
        }
    }
    let child = command
        .spawn()
        .map_err(|e| Error(format!("can't start {}: {e}", locations.server().display())))?;
    #[cfg(windows)]
    super::process_group::tie_to_app(&child);
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    super::watchdog::tie_to_app(&child);

    let mut server = Running {
        child: Some(child),
        connection: Connection {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(300)))
                .http_status_as_error(false)
                .build()
                .into(),
            url: format!("http://127.0.0.1:{port}"),
            key,
            generation: 0,
        },
    };

    let probe: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(2)))
        .http_status_as_error(false)
        .build()
        .into();
    let started = Instant::now();
    loop {
        if let Some(status) = server.child.as_mut().and_then(|child| child.try_wait().ok().flatten()) {
            let tail = tail_of(&locations.log);
            let error = Error(format!(
                "the embedding server stopped while starting ({status}); its log, {}, ends: {tail}",
                locations.log.display(),
            ));
            return Err(if tail.contains(PORT_TAKEN) {
                Start::PortTaken(error)
            } else {
                Start::Failed(error)
            });
        }
        let ready = probe
            .get(&format!("{}/health", server.connection.url))
            .call()
            .is_ok_and(|response| response.status().as_u16() == 200);
        if ready {
            let ours = probe
                .get(&format!("{}/v1/models", server.connection.url))
                .header("Authorization", &format!("Bearer {}", server.connection.key))
                .call()
                .is_ok_and(|response| response.status().as_u16() == 200);
            if ours {
                return Ok(server);
            }
            return Err(Start::PortTaken(Error(format!(
                "another program answered on port {port}"
            ))));
        }
        if started.elapsed() > START_TIMEOUT {
            return Err(Error(format!(
                "the embedding server didn't get ready within {START_TIMEOUT:?}"
            ))
            .into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn secret() -> String {
    (0..4)
        .map(|round| {
            let mut hasher = RandomState::new().build_hasher();
            hasher.write_u32(round);
            format!("{:016x}", hasher.finish())
        })
        .collect()
}

fn tail_of(path: &Path) -> String {
    let read = || -> std::io::Result<String> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        file.seek(SeekFrom::Start(length.saturating_sub(600)))?;
        let mut text = String::new();
        let _ = file.take(600).read_to_string(&mut text);
        Ok(text)
    };
    read().map(|text| text.trim().replace('\n', " | ")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader, Write},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use super::*;

    #[test]
    fn replies_reordered_by_index() {
        let reply = r#"{"data":[{"index":1,"embedding":[3,4]},{"index":0,"embedding":[1.5,2]}]}"#;
        assert_eq!(parse_embeddings(reply, 2).unwrap(), [vec![1.5, 2.0], vec![3.0, 4.0]]);
        assert!(parse_embeddings(reply, 3).is_err());
        assert!(parse_embeddings("not json", 1).is_err());
        assert!(parse_embeddings(r#"{"data":[{"index":5,"embedding":[1]}]}"#, 1).is_err());
    }

    #[test]
    fn parses_gpu_name() {
        let listing = "Available devices:\n  Vulkan0: NVIDIA GeForce RTX 3070 Ti (8017 MiB, 7249 MiB free)\n";
        let devices = listed_devices(listing);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "Vulkan0");
        assert_eq!(devices[0].name, "NVIDIA GeForce RTX 3070 Ti");

        let two = "Available devices:\n  Vulkan0: AMD Radeon(TM) Graphics (2048 MiB, 1900 MiB free)\n  Vulkan1: Other (1 MiB, 1 MiB free)\n  CPU: Some CPU (0 MiB, 0 MiB free)\n";
        let devices = listed_devices(two);
        assert_eq!(devices.len(), 2, "{devices:?}");
        assert_eq!(devices[0].name, "AMD Radeon(TM) Graphics");
        assert_eq!(devices[1].name, "Other");
        assert_eq!(
            tidy_gpu_name("AMD Radeon Graphics (RADV RENOIR)"),
            "AMD Radeon Graphics"
        );
        assert_eq!(
            tidy_gpu_name("Intel(R) Arc(tm) A770 Graphics"),
            "Intel(R) Arc(tm) A770 Graphics"
        );
        assert_eq!(
            gpu_key("Intel(R) Arc(tm) A770 Graphics"),
            gpu_key("Intel(R) Arc(TM) A770 Graphics")
        );
        assert!(same_gpu(
            "Intel(R) Arc(tm) A770 Graphics (DG2)",
            "Intel(R) Arc(TM) A770 Graphics"
        ));
    }

    #[test]
    fn prefers_a_discrete_gpu() {
        let gpus = [
            FoundGpu {
                id: "Vulkan0".into(),
                name: "AMD Radeon Graphics".into(),
                integrated: true,
                pinned: false,
            },
            FoundGpu {
                id: "Vulkan1".into(),
                name: "Intel(R) Arc(tm) A770 Graphics".into(),
                integrated: false,
                pinned: false,
            },
        ];
        assert_eq!(
            choose(&gpus, None).map(|(gpu, pinned)| (gpu.id.as_str(), pinned)),
            Some(("Vulkan1", false))
        );
        assert_eq!(
            choose(&gpus, Some("Vulkan0")).map(|(gpu, pinned)| (gpu.id.as_str(), pinned)),
            Some(("Vulkan0", true))
        );
        assert_eq!(
            choose(&gpus, Some("arc")).map(|(gpu, pinned)| (gpu.id.as_str(), pinned)),
            Some(("Vulkan1", true))
        );
        assert_eq!(
            choose(&gpus, Some("1")).map(|(gpu, pinned)| (gpu.id.as_str(), pinned)),
            Some(("Vulkan1", true))
        );
        assert_eq!(
            choose(&gpus, Some("nothing")).map(|(gpu, _)| gpu.id.as_str()),
            Some("Vulkan1")
        );
        assert_eq!(
            choose(&gpus[..1], None).map(|(gpu, _)| gpu.id.as_str()),
            Some("Vulkan0")
        );
    }

    fn fake_server(requests: Arc<AtomicUsize>) -> Connection {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let requests = requests.clone();
                thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    loop {
                        let mut length = 0;
                        let mut line = String::new();
                        loop {
                            line.clear();
                            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                                return;
                            }
                            if line == "\r\n" {
                                break;
                            }
                            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                                length = value.trim().parse().unwrap();
                            }
                        }
                        let mut body = vec![0; length];
                        reader.read_exact(&mut body).unwrap();
                        let body = String::from_utf8(body).unwrap();
                        requests.fetch_add(1, Ordering::SeqCst);
                        if body.contains("crash") {
                            return;
                        }
                        let (status, reply) = if body.contains("reject") {
                            ("400 Bad Request", r#"{"error":"no good"}"#.to_owned())
                        } else {
                            let inputs = body.matches("\"content\"").count();
                            let data: Vec<_> = (0..inputs)
                                .map(|i| json!({"index": i, "embedding": vec![i as f64 + 1.0; 4]}))
                                .collect();
                            ("200 OK", json!({"data": data}).to_string())
                        };
                        write!(
                            stream,
                            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{reply}",
                            reply.len()
                        )
                        .unwrap();
                    }
                });
            }
        });
        Connection {
            agent: ureq::Agent::config_builder().http_status_as_error(false).build().into(),
            url: format!("http://127.0.0.1:{port}"),
            key: "k".into(),
            generation: 0,
        }
    }

    fn server_at(connection: Connection) -> LlamaServer {
        let server = LlamaServer::new(Locations {
            runtime: PathBuf::new(),
            models: PathBuf::new(),
            log: PathBuf::new(),
        });
        *lock(&server.running) = Some(Running {
            child: None,
            connection,
        });
        server
    }

    fn text(content: &str) -> Prepared {
        Prepared::Text(content.to_owned())
    }

    #[test]
    fn rejected_batch_retried_one_by_one() {
        let requests = Arc::new(AtomicUsize::new(0));
        let server = server_at(fake_server(requests.clone()));
        let (good, bad, also_good) = (text("fine"), text("reject me"), text("fine too"));
        let out = server.embed_batch(&[&good, &bad, &also_good]).unwrap();
        assert!(out[0].is_ok() && out[2].is_ok());
        assert!(out[1].as_ref().unwrap_err().contains("400"), "{:?}", out[1]);
        assert_eq!(requests.load(Ordering::SeqCst), 1 + 3);
    }
}
