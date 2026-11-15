use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, OnceLock},
};

use ort::{
    environment::Environment,
    session::{
        Session,
        builder::{AutoDevicePolicy, GraphOptimizationLevel, SessionBuilder},
    },
    value::Tensor,
};
#[cfg(target_os = "macos")]
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Gpu,
    Cpu,
}

pub const CORE_ML: bool = cfg!(target_os = "macos");
pub type Output = (Vec<usize>, Vec<f32>);

pub struct Net {
    name: String,
    bytes: Vec<u8>,
    metadata: HashMap<String, String>,
    gpu: bool,
    idle: Mutex<Vec<Session>>,
    #[cfg(target_os = "macos")]
    sized: Mutex<Sized>,
    #[cfg(target_os = "macos")]
    max_sizes: usize,
    #[cfg(target_os = "macos")]
    saved: Option<Saved>,
}

#[cfg(target_os = "macos")]
struct Saved {
    folder: PathBuf,
}

#[cfg(target_os = "macos")]
impl Saved {
    fn new(root: &Path, name: &str, bytes: &[u8]) -> Self {
        let stem = name.trim_end_matches(".onnx");
        let mine = format!("{stem}-{:016x}", fnv1a(bytes));
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let other = entry.file_name().to_string_lossy().into_owned();
                if other != mine
                    && other
                        .strip_prefix(stem)
                        .is_some_and(|rest| rest.len() == 17 && rest.starts_with('-'))
                {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
        Self {
            folder: root.join(mine),
        }
    }

    fn made(&self, shape: &[usize]) -> Option<PathBuf> {
        let folder = self.folder.join(size_name(shape));
        folder.is_dir().then_some(folder)
    }

    fn making(&self, shape: &[usize]) -> Option<PathBuf> {
        static MADE: AtomicUsize = AtomicUsize::new(0);
        let folder = self.folder.join(format!(
            ".{}-{}-{}",
            size_name(shape),
            std::process::id(),
            MADE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&folder).ok().map(|()| folder)
    }

    fn keep(&self, made: &Path, shape: &[usize]) {
        if let Ok(entries) = fs::read_dir(made) {
            for key in entries.flatten() {
                if let Ok(models) = fs::read_dir(key.path()) {
                    for model in models.flatten() {
                        let _ = fs::remove_dir_all(model.path().join("model").join("Data"));
                        let _ = fs::remove_dir_all(model.path().join("model").join("tmp"));
                    }
                }
            }
        }
        if fs::rename(made, self.folder.join(size_name(shape))).is_err() {
            let _ = fs::remove_dir_all(made);
        }
    }
}

pub fn keep_only(root: &Path, files: &[&str]) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stem = name.rsplit_once('-').map_or(name.as_str(), |(stem, _)| stem);
        if !CORE_ML || !files.iter().any(|file| file.trim_end_matches(".onnx") == stem) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(target_os = "macos")]
fn size_name(shape: &[usize]) -> String {
    shape.iter().map(usize::to_string).collect::<Vec<_>>().join("x")
}

#[cfg(target_os = "macos")]
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(target_os = "macos")]
type Shared = Arc<Mutex<Session>>;

#[cfg(target_os = "macos")]
#[derive(Default)]
struct Sized {
    by_shape: HashMap<Vec<usize>, Arc<OnceLock<Result<Shared, String>>>>,
    used: Vec<Vec<usize>>,
}

fn environment() -> Result<&'static Environment, String> {
    static ENVIRONMENT: OnceLock<Result<Environment, String>> = OnceLock::new();
    ENVIRONMENT
        .get_or_init(|| {
            ort::init()
                .with_name("mesa-ocr")
                .build()
                .map_err(|e| format!("ONNX Runtime couldn't be started: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

impl Net {
    pub fn load(path: &Path, place: Place, max_sizes: usize, saved: Option<&Path>) -> Result<Self, String> {
        let name = path
            .file_name()
            .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let bytes = std::fs::read(path).map_err(|e| format!("can't read {name}: {e}"))?;
        let metadata = metadata(&bytes).ok_or_else(|| format!("{name} isn't an ONNX file"))?;
        environment()?;
        let gpu = place == Place::Gpu;
        #[cfg(target_os = "macos")]
        {
            let saved = saved.filter(|_| gpu).map(|root| Saved::new(root, &name, &bytes));
            Ok(Self {
                name,
                bytes,
                metadata,
                gpu,
                idle: Mutex::default(),
                sized: Mutex::default(),
                max_sizes: max_sizes.max(1),
                saved,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (max_sizes, saved);
            Ok(Self {
                name,
                bytes,
                metadata,
                gpu,
                idle: Mutex::default(),
            })
        }
    }

    pub fn metadata(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(String::as_str)
    }

    pub fn on_gpu(&self) -> bool {
        self.gpu
    }

    fn builder(&self) -> Result<SessionBuilder, String> {
        let failed = |e: ort::Error| format!("can't load {}: {e}", self.name);
        Session::builder(environment()?)
            .map_err(failed)?
            .with_optimization_level(GraphOptimizationLevel::All)
            .map_err(|e| failed(e.into()))?
            .with_intra_threads(1)
            .map_err(|e| failed(e.into()))
    }

    fn any_size(&self) -> Result<Session, String> {
        let failed = |e: ort::Error| format!("can't load {}: {e}", self.name);
        let builder = self.builder()?;
        #[cfg(not(target_os = "macos"))]
        if self.gpu {
            return builder
                .with_execution_providers([ort::ep::WebGPU::default().build().error_on_failure()])
                .map_err(|e| failed(e.into()))?
                .commit_from_memory(&self.bytes)
                .map_err(failed);
        }
        builder
            .with_auto_device(AutoDevicePolicy::PreferCPU)
            .map_err(|e| failed(e.into()))?
            .with_execution_providers([ort::ep::CPU::default().build()])
            .map_err(|e| failed(e.into()))?
            .commit_from_memory(&self.bytes)
            .map_err(failed)
    }

    #[cfg(target_os = "macos")]
    fn core_ml(&self, shape: &[usize], saved: Option<&Path>) -> Result<Session, String> {
        use ort::ep::coreml::{ComputeUnits, CoreML, ModelFormat};
        let failed = |e: ort::Error| format!("can't load {}: {e}", self.name);
        let mut builder = self.builder()?;
        for (name, &size) in ["N", "", "H", "W"].iter().zip(shape) {
            if !name.is_empty() {
                builder = builder
                    .with_dimension_override(*name, size as i64)
                    .map_err(|e| failed(e.into()))?;
            }
        }
        let mut core_ml = CoreML::default()
            .with_model_format(ModelFormat::MLProgram)
            .with_compute_units(ComputeUnits::CPUAndGPU)
            .with_static_input_shapes(true);
        if let Some(saved) = saved {
            core_ml = core_ml.with_model_cache_dir(saved.display());
        }
        builder
            .with_execution_providers([core_ml.build()])
            .map_err(|e| failed(e.into()))?
            .commit_from_memory(&self.bytes)
            .map_err(failed)
    }

    #[cfg(target_os = "macos")]
    fn sized(&self, shape: &[usize]) -> Result<Shared, String> {
        let cell = {
            let mut sized = self.sized.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            sized.used.retain(|s| s != shape);
            sized.used.push(shape.to_vec());
            while sized.used.len() > self.max_sizes {
                let oldest = sized.used.remove(0);
                sized.by_shape.remove(&oldest);
            }
            sized.by_shape.entry(shape.to_vec()).or_default().clone()
        };
        cell.get_or_init(|| self.made(shape).map(|session| Arc::new(Mutex::new(session))))
            .clone()
    }

    #[cfg(target_os = "macos")]
    fn made(&self, shape: &[usize]) -> Result<Session, String> {
        let Some(saved) = &self.saved else {
            return self.core_ml(shape, None);
        };
        if let Some(folder) = saved.made(shape) {
            match self.core_ml(shape, Some(&folder)) {
                Ok(session) => return Ok(session),
                Err(_) => {
                    let _ = fs::remove_dir_all(&folder);
                }
            }
        }
        let Some(making) = saved.making(shape) else {
            return self.core_ml(shape, None);
        };
        let session = self.core_ml(shape, Some(&making));
        match &session {
            Ok(_) => saved.keep(&making, shape),
            Err(_) => {
                let _ = fs::remove_dir_all(&making);
            }
        }
        session
    }

    pub fn save(&self, shape: &[usize]) -> Result<bool, String> {
        #[cfg(target_os = "macos")]
        {
            let Some(saved) = self.saved.as_ref().filter(|_| self.gpu) else {
                return Ok(false);
            };
            if saved.made(shape).is_some() {
                return Ok(false);
            }
            drop(self.made(shape)?);
            if let Some(folder) = saved.made(shape) {
                let mut session = self.core_ml(shape, Some(&folder))?;
                let nothing = Tensor::from_array((shape.to_vec(), vec![0f32; shape.iter().product()]))
                    .map_err(|e| e.to_string())?;
                session
                    .run(ort::inputs![nothing])
                    .map_err(|e| format!("{} failed: {e}", self.name))?;
            }
            Ok(true)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = shape;
            Ok(false)
        }
    }

    pub fn prepare(&self, shape: &[usize]) -> Result<(), String> {
        if !self.gpu {
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        return self.sized(shape).map(drop);
        #[cfg(not(target_os = "macos"))]
        self.run(shape, vec![0f32; shape.iter().product()]).map(drop)
    }

    pub fn run(&self, shape: &[usize], input: Vec<f32>) -> Result<Vec<Output>, String> {
        let tensor = Tensor::from_array((shape.to_vec(), input)).map_err(|e| e.to_string())?;
        let run = |session: &mut Session| -> Result<Vec<Output>, String> {
            let outputs = session
                .run(ort::inputs![tensor])
                .map_err(|e| format!("{} failed: {e}", self.name))?;
            let mut all = Vec::with_capacity(outputs.len());
            for (_, output) in outputs.iter() {
                let gave = |e: ort::Error| format!("{} gave something else: {e}", self.name);
                let (shape, values) = match output.try_extract_tensor::<f32>() {
                    Ok((shape, values)) => (shape.to_vec(), values.to_vec()),
                    Err(_) => {
                        let (shape, values) = output.try_extract_tensor::<i64>().map_err(gave)?;
                        (shape.to_vec(), values.iter().map(|&v| v as f32).collect())
                    }
                };
                all.push((shape.iter().map(|&d| d as usize).collect(), values));
            }
            Ok(all)
        };
        #[cfg(target_os = "macos")]
        if self.gpu {
            let session = self.sized(shape)?;
            let mut session = session.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            return run(&mut session);
        }
        let taken = self.idle.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).pop();
        let mut session = match taken {
            Some(session) => session,
            None => self.any_size()?,
        };
        let result = run(&mut session);
        self.idle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(session);
        result
    }
}

fn metadata(bytes: &[u8]) -> Option<HashMap<String, String>> {
    let mut found = HashMap::new();
    for (field, value) in fields(bytes)? {
        if let (14, Field::Bytes(entry)) = (field, value) {
            let (mut key, mut text) = (None, None);
            for (field, value) in fields(entry)? {
                match (field, value) {
                    (1, Field::Bytes(k)) => key = Some(String::from_utf8_lossy(k).into_owned()),
                    (2, Field::Bytes(v)) => text = Some(String::from_utf8_lossy(v).into_owned()),
                    _ => {}
                }
            }
            if let (Some(key), Some(text)) = (key, text) {
                found.insert(key, text);
            }
        }
    }
    Some(found)
}

enum Field<'a> {
    Number,
    Bytes(&'a [u8]),
}

fn fields(mut bytes: &[u8]) -> Option<Vec<(u64, Field<'_>)>> {
    fn varint(bytes: &mut &[u8]) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let (&byte, rest) = bytes.split_first()?;
            *bytes = rest;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }
    let mut out = Vec::new();
    while !bytes.is_empty() {
        let key = varint(&mut bytes)?;
        let field = match key & 7 {
            0 => {
                varint(&mut bytes)?;
                Field::Number
            }
            1 => {
                bytes = bytes.get(8..)?;
                Field::Number
            }
            2 => {
                let length = usize::try_from(varint(&mut bytes)?).ok()?;
                let (value, rest) = (bytes.get(..length)?, bytes.get(length..)?);
                bytes = rest;
                Field::Bytes(value)
            }
            5 => {
                bytes = bytes.get(4..)?;
                Field::Number
            }
            _ => return None,
        };
        out.push((key >> 3, field));
    }
    Some(out)
}
