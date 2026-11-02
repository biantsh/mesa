use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::Mutex,
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::llama::{PICTURE_MODEL, TEXT_MODEL};
use crate::index::lock;

#[derive(Clone, Copy, Debug)]
pub struct ModelFile {
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

pub const FILES: [ModelFile; 2] = [
    ModelFile {
        name: TEXT_MODEL,
        bytes: 309_855_456,
        sha256: "2188ac1deca4b77dffefd603c2776a9d76d9d74ec01841392982ebb840b09135",
    },
    ModelFile {
        name: PICTURE_MODEL,
        bytes: 554_821_056,
        sha256: "0531309472a6bc6f4b146c3736eee7795ec9db1903c25490d3c937490cef6dd8",
    },
];

pub const SOURCE: &str =
    "https://huggingface.co/ggml-org/embeddinggemma-2-GGUF/resolve/6ea3e3a309d12a4779cfda98b8c19800d2d5da71";
const SPARE_BYTES: u64 = 100 * 1024 * 1024;
const ATTEMPTS: u32 = 4;
const RETRY_PAUSE: Duration = if cfg!(test) {
    Duration::from_millis(10)
} else {
    Duration::from_secs(2)
};
const SILENCE: Duration = Duration::from_secs(30);
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ModelStatus {
    Downloading { done: u64, total: u64 },
    Verifying,
    Starting,
    Preparing,
    Ready,
    Failed { reason: String, setup: bool },
}

pub struct ModelState(Mutex<ModelStatus>);

impl ModelState {
    pub fn new(status: ModelStatus) -> Self {
        Self(Mutex::new(status))
    }

    pub fn get(&self) -> ModelStatus {
        lock(&self.0).clone()
    }

    pub fn set(&self, status: ModelStatus) {
        *lock(&self.0) = status;
    }

    pub fn finish_preparing(&self) {
        let mut status = lock(&self.0);
        if *status == ModelStatus::Preparing {
            *status = ModelStatus::Ready;
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Check {
    Good,
    NeedsVerifying,
    Missing,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
struct Stamp {
    name: String,
    bytes: u64,
    modified_ms: u128,
}

#[derive(Serialize, Deserialize, Default)]
struct Verified {
    files: Vec<Stamp>,
}

const MARKER: &str = "verified.json";

fn read_marker(dir: &Path) -> Verified {
    fs::read_to_string(dir.join(MARKER))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_marker(dir: &Path, marker: &Verified) -> Result<(), String> {
    let text = serde_json::to_string_pretty(marker).map_err(|e| e.to_string())?;
    let temporary = dir.join(format!("{MARKER}.tmp"));
    fs::write(&temporary, text)
        .and_then(|()| fs::rename(&temporary, dir.join(MARKER)))
        .map_err(|e| format!("Couldn't write in {}: {e}", dir.display()))
}

fn stamp_of(dir: &Path, file: &ModelFile) -> Option<Stamp> {
    let metadata = fs::metadata(dir.join(file.name)).ok()?;
    if metadata.len() != file.bytes {
        return None;
    }
    let modified_ms = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis();
    Some(Stamp {
        name: file.name.to_owned(),
        bytes: file.bytes,
        modified_ms,
    })
}

pub fn check(dir: &Path, files: &[ModelFile]) -> Check {
    let marker = read_marker(dir);
    let mut known = true;
    for file in files {
        let Some(stamp) = stamp_of(dir, file) else {
            return Check::Missing;
        };
        known &= marker.files.contains(&stamp);
    }
    if known { Check::Good } else { Check::NeedsVerifying }
}

pub fn initial_status(dir: &Path, files: &[ModelFile]) -> ModelStatus {
    match check(dir, files) {
        Check::Good => ModelStatus::Starting,
        Check::NeedsVerifying => ModelStatus::Verifying,
        Check::Missing => {
            let total = files.iter().map(|f| f.bytes).sum();
            let have = files.iter().map(|f| present_bytes(dir, f)).sum::<u64>().min(total);
            ModelStatus::Downloading { done: have, total }
        }
    }
}

fn present_bytes(dir: &Path, file: &ModelFile) -> u64 {
    if stamp_of(dir, file).is_some() {
        return file.bytes;
    }
    fs::metadata(part_path(dir, file)).map_or(0, |m| m.len().min(file.bytes))
}

fn part_path(dir: &Path, file: &ModelFile) -> std::path::PathBuf {
    dir.join(format!("{}.part", file.name))
}

pub fn ensure(dir: &Path, files: &[ModelFile], source: &str, state: &ModelState) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("Couldn't make the folder {}: {e}", dir.display()))?;
    let mut marker = read_marker(dir);

    let mut good = Vec::new();
    let mut to_verify = Vec::new();
    for file in files {
        match stamp_of(dir, file) {
            Some(stamp) if marker.files.contains(&stamp) => good.push(file.name),
            Some(stamp) => to_verify.push((file, stamp)),
            None => {
                let _ = fs::remove_file(dir.join(file.name));
            }
        }
    }
    if !to_verify.is_empty() {
        state.set(ModelStatus::Verifying);
        for (file, stamp) in to_verify {
            if sha256_of(&dir.join(file.name)).is_ok_and(|hash| hash == file.sha256) {
                marker.files.retain(|s| s.name != file.name);
                marker.files.push(stamp);
                good.push(file.name);
            } else {
                let _ = fs::remove_file(dir.join(file.name));
            }
        }
        write_marker(dir, &marker)?;
    }

    let missing: Vec<&ModelFile> = files.iter().filter(|f| !good.contains(&f.name)).collect();
    if missing.is_empty() {
        return Ok(());
    }

    let total: u64 = files.iter().map(|f| f.bytes).sum();
    let already: u64 = files.iter().map(|f| present_bytes(dir, f)).sum();
    let needed = total.saturating_sub(already);
    if let Some(free) = free_space(dir)
        && free < needed + SPARE_BYTES
    {
        return Err(format!(
            "There isn't enough room on the disk to download the search model: {} are needed, and {} are free.",
            megabytes(needed + SPARE_BYTES),
            megabytes(free)
        ));
    }

    let mut done_before = total - missing.iter().map(|f| f.bytes).sum::<u64>();
    state.set(ModelStatus::Downloading {
        done: already.min(total),
        total,
    });
    for file in &missing {
        download(
            &format!("{source}/{}", file.name),
            &part_path(dir, file),
            file.bytes,
            done_before,
            total,
            state,
        )?;
        done_before += file.bytes;
    }

    state.set(ModelStatus::Verifying);
    for file in missing {
        let part = part_path(dir, file);
        if !sha256_of(&part).is_ok_and(|hash| hash == file.sha256) {
            let _ = fs::remove_file(&part);
            return Err(format!(
                "The downloaded file {} is damaged (its checksum doesn't match). Try again.",
                file.name
            ));
        }
        fs::rename(&part, dir.join(file.name)).map_err(|e| format!("Couldn't save {}: {e}", file.name))?;
        let stamp = stamp_of(dir, file)
            .ok_or_else(|| format!("{} isn't the size it should be after the download.", file.name))?;
        marker.files.retain(|s| s.name != file.name);
        marker.files.push(stamp);
    }
    write_marker(dir, &marker)
}

fn megabytes(bytes: u64) -> String {
    format!("{} MB", bytes / 1_000_000)
}

enum Failure {
    Again(String),
    Final(String),
}

fn download(
    url: &str,
    part: &Path,
    bytes: u64,
    done_before: u64,
    total: u64,
    state: &ModelState,
) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_per_read(Some(SILENCE))
        .http_status_as_error(false)
        .build()
        .into();
    let mut last = String::new();
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            thread::sleep(RETRY_PAUSE * attempt);
        }
        match try_download(&agent, url, part, bytes, done_before, total, state) {
            Ok(()) => return Ok(()),
            Err(Failure::Final(why)) => return Err(why),
            Err(Failure::Again(why)) => last = why,
        }
    }
    Err(format!(
        "The download didn't finish. Check the internet connection and try again. ({last})"
    ))
}

fn try_download(
    agent: &ureq::Agent,
    url: &str,
    part: &Path,
    bytes: u64,
    done_before: u64,
    total: u64,
    state: &ModelState,
) -> Result<(), Failure> {
    let mut have = fs::metadata(part).map_or(0, |m| m.len());
    if have > bytes {
        let _ = fs::remove_file(part);
        have = 0;
    }
    if have == bytes {
        return Ok(());
    }
    let mut request = agent.get(url).header("Accept-Encoding", "identity");
    if have > 0 {
        request = request.header("Range", format!("bytes={have}-"));
    }
    let response = request.call().map_err(|e| Failure::Again(e.to_string()))?;
    let status = response.status().as_u16();
    let appending = match status {
        206 => true,
        200 => false,
        416 => {
            let _ = fs::remove_file(part);
            return Err(Failure::Again("the server wouldn't continue the download".into()));
        }
        408 | 429 | 500..=599 => return Err(Failure::Again(format!("the server answered {status}"))),
        _ => {
            return Err(Failure::Final(format!(
                "The server doesn't give the search model ({status}); {url}"
            )));
        }
    };
    let mut file = if appending {
        OpenOptions::new().append(true).open(part)
    } else {
        File::create(part)
    }
    .map_err(|e| Failure::Final(format!("Couldn't write {}: {e}", part.display())))?;

    let mut written = if appending { have } else { 0 };
    let mut reader = response.into_body().into_reader();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut reported = Instant::now() - PROGRESS_EVERY;
    loop {
        let n = reader.read(&mut buffer).map_err(|e| Failure::Again(e.to_string()))?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])
            .map_err(|e| Failure::Final(format!("Couldn't write {}: {e}", part.display())))?;
        written += n as u64;
        if written > bytes {
            let _ = fs::remove_file(part);
            return Err(Failure::Final(
                "The server gave more than the search model is. Try again later.".into(),
            ));
        }
        if reported.elapsed() >= PROGRESS_EVERY {
            reported = Instant::now();
            state.set(ModelStatus::Downloading {
                done: done_before + written,
                total,
            });
        }
    }
    state.set(ModelStatus::Downloading {
        done: done_before + written,
        total,
    });
    if written == bytes {
        Ok(())
    } else {
        Err(Failure::Again("the connection ended early".into()))
    }
}

pub fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn free_space(dir: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use windows::{Win32::Storage::FileSystem::GetDiskFreeSpaceExW, core::HSTRING};
        let mut free = 0u64;
        let path = HSTRING::from(dir.to_string_lossy().as_ref());
        unsafe { GetDiskFreeSpaceExW(&path, Some(&mut free), None, None) }.ok()?;
        Some(free)
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
        None
    }
}

#[cfg(test)]
pub(super) mod testing {
    use std::{
        io::{BufRead, BufReader},
        net::TcpListener,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
    };

    use super::*;

    pub fn files(contents: &[(&str, &[u8])]) -> Vec<ModelFile> {
        contents
            .iter()
            .map(|(name, bytes)| {
                let hash = format!("{:x}", Sha256::digest(bytes));
                ModelFile {
                    name: Box::leak(name.to_string().into_boxed_str()),
                    bytes: bytes.len() as u64,
                    sha256: Box::leak(hash.into_boxed_str()),
                }
            })
            .collect()
    }

    pub fn content(seed: u8, length: usize) -> Vec<u8> {
        (0..length)
            .map(|n| (n as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    #[derive(Default)]
    pub struct Behaviour {
        pub cut_first_after: Option<usize>,
        pub no_ranges: bool,
        pub wrong_bytes: bool,
        pub busy_first: usize,
    }

    pub fn server(
        served: Vec<(String, Vec<u8>)>,
        behaviour: Behaviour,
    ) -> (String, Arc<Mutex<Vec<String>>>, Arc<AtomicUsize>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let requests = Arc::new(AtomicUsize::new(0));
        let (seen_ranges, seen_requests) = (ranges.clone(), requests.clone());
        let behaviour = Arc::new(behaviour);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let (served, behaviour, ranges, requests) = (
                    served.clone(),
                    behaviour.clone(),
                    seen_ranges.clone(),
                    seen_requests.clone(),
                );
                thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request_line = String::new();
                    reader.read_line(&mut request_line).unwrap();
                    let path = request_line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .trim_start_matches('/')
                        .to_owned();
                    let mut range = None;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(value) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                            range = value.trim().trim_end_matches('-').parse::<usize>().ok();
                        }
                    }
                    let nth = requests.fetch_add(1, Ordering::SeqCst);
                    if nth < behaviour.busy_first {
                        let _ = write!(stream, "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n");
                        return;
                    }
                    let Some((_, content)) = served.iter().find(|(name, _)| *name == path) else {
                        let _ = write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                        return;
                    };
                    let mut content = content.clone();
                    if behaviour.wrong_bytes {
                        content.iter_mut().for_each(|b| *b ^= 0xff);
                    }
                    let start = if behaviour.no_ranges { None } else { range };
                    if let Some(start) = start {
                        ranges.lock().unwrap().push(format!("{path} from {start}"));
                    }
                    let body = &content[start.unwrap_or(0)..];
                    let status = if start.is_some() {
                        "206 Partial Content"
                    } else {
                        "200 OK"
                    };
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\n\r\n",
                        body.len()
                    );
                    let cut = if nth == behaviour.busy_first {
                        behaviour.cut_first_after
                    } else {
                        None
                    };
                    let _ = stream.write_all(&body[..cut.unwrap_or(body.len()).min(body.len())]);
                });
            }
        });
        (address, ranges, requests)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;

    use super::testing::{Behaviour, content, files, server};

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mesa-model-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resumes_interrupted_download() {
        let dir = temp_dir("resume");
        let a = content(3, 500_000);
        let files = files(&[("a.gguf", &a)]);
        let behaviour = Behaviour {
            cut_first_after: Some(200_000),
            ..Default::default()
        };
        let (address, ranges, _) = server(vec![("a.gguf".into(), a.clone())], behaviour);

        ensure(&dir, &files, &address, &ModelState::new(ModelStatus::Verifying)).unwrap();

        assert_eq!(fs::read(dir.join("a.gguf")).unwrap(), a);
        let ranges = ranges.lock().unwrap();
        assert_eq!(ranges.len(), 1, "{ranges:?}");
        assert!(
            ranges[0].starts_with("a.gguf from ") && !ranges[0].ends_with(" from 0"),
            "{ranges:?}"
        );
    }

    #[test]
    fn rejects_wrong_download() {
        let dir = temp_dir("damaged");
        let a = content(5, 100_000);
        let files = files(&[("a.gguf", &a)]);
        let (address, _, _) = server(
            vec![("a.gguf".into(), a)],
            Behaviour {
                wrong_bytes: true,
                ..Default::default()
            },
        );
        let error = ensure(&dir, &files, &address, &ModelState::new(ModelStatus::Verifying)).unwrap_err();
        assert!(error.contains("damaged") && error.contains("a.gguf"), "{error}");
        assert!(!dir.join("a.gguf").exists() && !part_path(&dir, &files[0]).exists());
        assert_eq!(check(&dir, &files), Check::Missing);
    }

    #[test]
    fn refetches_corrupt_file() {
        let dir = temp_dir("wrong-contents");
        let a = content(8, 80_000);
        let files = files(&[("a.gguf", &a)]);
        fs::write(dir.join("a.gguf"), content(9, 80_000)).unwrap();
        let (address, _, requests) = server(vec![("a.gguf".into(), a.clone())], Behaviour::default());
        ensure(&dir, &files, &address, &ModelState::new(ModelStatus::Verifying)).unwrap();
        assert_eq!(fs::read(dir.join("a.gguf")).unwrap(), a);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }
}
