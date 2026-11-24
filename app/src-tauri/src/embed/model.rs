use std::{
    cell::Cell,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
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
    pub packed: Option<Packed>,
}

#[derive(Clone, Copy, Debug)]
pub struct Packed {
    pub piece: u64,
    pub sizes: &'static [u64],
}

impl Packed {
    fn offset(&self, piece: usize) -> u64 {
        self.sizes[..piece.min(self.sizes.len())].iter().sum()
    }

    fn bytes(&self) -> u64 {
        self.offset(self.sizes.len())
    }

    fn piece_of(&self, at: u64) -> usize {
        (at / self.piece) as usize
    }
}

pub const FILES: [ModelFile; 2] = [
    ModelFile {
        name: TEXT_MODEL,
        bytes: 309_855_456,
        sha256: "2188ac1deca4b77dffefd603c2776a9d76d9d74ec01841392982ebb840b09135",
        packed: Some(Packed {
            piece: PIECE,
            sizes: &[
                4_131_144, 13_895_274, 16_173_776, 16_174_008, 16_163_666, 16_153_952, 16_156_076, 16_136_581,
                16_135_957, 16_090_445, 12_956_457, 16_135_526, 16_124_531, 16_115_377, 16_111_047, 16_098_636,
                16_134_903, 16_136_134, 7_553_754,
            ],
        }),
    },
    ModelFile {
        name: PICTURE_MODEL,
        bytes: 554_821_056,
        sha256: "0531309472a6bc6f4b146c3736eee7795ec9db1903c25490d3c937490cef6dd8",
        packed: Some(Packed {
            piece: PIECE,
            sizes: &[
                4_962_179, 5_972_435, 5_621_512, 5_368_734, 6_033_505, 5_399_902, 5_645_464, 5_963_108, 4_977_090,
                6_042_669, 5_646_061, 5_333_711, 5_876_398, 5_448_928, 5_557_433, 6_042_709, 5_020_981, 6_099_188,
                5_709_763, 11_947_093, 16_015_093, 16_106_448, 16_126_881, 16_112_852, 16_052_007, 16_023_288,
                16_045_157, 16_060_560, 16_085_898, 8_790_321, 7_791_206, 7_955_281, 7_787_097, 570_776,
            ],
        }),
    },
];

pub const OCR_FILES: [ModelFile; 3] = [
    ocr_file(
        "PP-OCRv6_det_tiny.onnx",
        1_760_696,
        "e987bf563858fea8c3480f4a0377d70d170cd231cab8348f1928b1d29525635f",
        &[1_569_537],
    ),
    ocr_file(
        "mesa_line_filter.onnx",
        720_412,
        "60f707748d1282b48ee19eeec4b81cc7e01218d1bfc38622824ee4e83f8fc2f4",
        &[664_540],
    ),
    ocr_file(
        "PP-OCRv6_rec_tiny.onnx",
        4_470_077,
        "356408c28c7d7134b79bb34da8794ba34b43995d36bc9c161e27650fe8085833",
        &[4_115_337],
    ),
];

const fn ocr_file(name: &'static str, bytes: u64, sha256: &'static str, sizes: &'static [u64]) -> ModelFile {
    ModelFile {
        name,
        bytes,
        sha256,
        packed: Some(Packed { piece: PIECE, sizes }),
    }
}

pub const OCR_PACKED_SOURCE: &str =
    "https://huggingface.co/biantsh/mesa-ocr/resolve/46645e90dd0c1aa9f704747d09aa7254d9a7d74a";
pub const SOURCE: &str =
    "https://huggingface.co/ggml-org/embeddinggemma-2-GGUF/resolve/6ea3e3a309d12a4779cfda98b8c19800d2d5da71";
pub const PACKED_SOURCE: &str =
    "https://huggingface.co/biantsh/embeddinggemma-2-GGUF-zstd/resolve/ccb0ae1208fc908d9fa29a1cf0eadacd3a96ac15";
const PIECE: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Sources {
    pub packed: Option<String>,
    pub plain: String,
}

impl Sources {
    pub fn ours() -> Self {
        Self {
            packed: Some(PACKED_SOURCE.to_owned()),
            plain: SOURCE.to_owned(),
        }
    }

    #[cfg(test)]
    pub fn plain(plain: &str) -> Self {
        Self {
            packed: None,
            plain: plain.to_owned(),
        }
    }

    pub fn packed_for(&self, files: &[ModelFile]) -> Option<&str> {
        self.packed
            .as_deref()
            .filter(|_| files.iter().all(|f| f.packed.is_some()))
    }
}

#[derive(Clone, Debug)]
pub struct Group {
    pub dir: PathBuf,
    pub files: Vec<ModelFile>,
    pub sources: Sources,
    pub optional: bool,
}

impl Group {
    fn packed(&self) -> bool {
        self.sources.packed_for(&self.files).is_some()
    }
}

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
    Finalizing,
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

pub fn initial_status_of(groups: &[Group]) -> ModelStatus {
    let checks: Vec<Check> = groups.iter().map(|g| check(&g.dir, &g.files)).collect();
    if checks.contains(&Check::Missing) {
        let packed: Vec<bool> = groups.iter().map(Group::packed).collect();
        downloading(groups, &packed)
    } else if checks.contains(&Check::NeedsVerifying) {
        ModelStatus::Verifying
    } else {
        ModelStatus::Starting
    }
}

fn downloading(groups: &[Group], packed: &[bool]) -> ModelStatus {
    let (done, total) = progress_around(groups, packed, None);
    ModelStatus::Downloading { done, total }
}

fn download_size(file: &ModelFile, packed: bool) -> u64 {
    file.packed.filter(|_| packed).map_or(file.bytes, |copy| copy.bytes())
}

fn downloaded(dir: &Path, file: &ModelFile, packed: bool) -> u64 {
    let have = present_bytes(dir, file);
    match file.packed.filter(|_| packed) {
        Some(copy) if have < file.bytes => copy.offset(copy.piece_of(have)),
        Some(copy) => copy.bytes(),
        None => have,
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

#[cfg(test)]
pub fn ensure(dir: &Path, files: &[ModelFile], sources: &Sources, state: &ModelState) -> Result<(), String> {
    ensure_all(
        &[Group {
            dir: dir.to_owned(),
            files: files.to_vec(),
            sources: sources.clone(),
            optional: false,
        }],
        state,
    )
}

pub fn ensure_all(groups: &[Group], state: &ModelState) -> Result<(), String> {
    let mut markers = Vec::with_capacity(groups.len());
    let mut missing: Vec<Vec<ModelFile>> = Vec::with_capacity(groups.len());
    let mut without = vec![false; groups.len()];
    for (n, group) in groups.iter().enumerate() {
        match what_is_there(group, state) {
            Ok((marker, files)) => {
                markers.push(marker);
                missing.push(files);
            }
            Err(why) if group.optional => {
                eprintln!(
                    "Mesa: the files in {} can't be kept there, so the app goes without them for now: {why}",
                    group.dir.display()
                );
                without[n] = true;
                markers.push(Verified::default());
                missing.push(Vec::new());
            }
            Err(why) => return Err(why),
        }
    }
    if missing.iter().all(Vec::is_empty) {
        return Ok(());
    }

    let kept = || {
        groups
            .iter()
            .zip(&without)
            .filter(|(_, gone)| !**gone)
            .map(|(group, _)| group)
    };
    let total: u64 = kept().flat_map(|g| &g.files).map(|f| f.bytes).sum();
    let already: u64 = kept()
        .flat_map(|g| g.files.iter().map(|f| present_bytes(&g.dir, f)))
        .sum();
    let needed = total.saturating_sub(already);
    if let Some(free) = kept().next().and_then(|group| free_space(&group.dir))
        && free < needed + SPARE_BYTES
    {
        return Err(format!(
            "There isn't enough room on the disk to download the search model: {} are needed, and {} are free.",
            megabytes(needed + SPARE_BYTES),
            megabytes(free)
        ));
    }

    let mut packed: Vec<bool> = groups.iter().map(Group::packed).collect();
    let last = missing.iter().rposition(|files| !files.is_empty());
    for (n, group) in groups.iter().enumerate() {
        if missing[n].is_empty() {
            continue;
        }
        match fetch_group(
            groups,
            n,
            &missing[n],
            &mut packed,
            &mut markers[n],
            state,
            Some(n) == last,
        ) {
            Ok(()) => {}
            Err(why) if group.optional => {
                eprintln!(
                    "Mesa: the files in {} couldn't be had, so the app goes without them for now: {why}",
                    group.dir.display()
                );
            }
            Err(why) => return Err(why),
        }
    }
    Ok(())
}

fn what_is_there(group: &Group, state: &ModelState) -> Result<(Verified, Vec<ModelFile>), String> {
    let dir = &group.dir;
    fs::create_dir_all(dir).map_err(|e| format!("Couldn't make the folder {}: {e}", dir.display()))?;
    let mut marker = read_marker(dir);

    let mut good = Vec::new();
    let mut to_verify = Vec::new();
    for file in &group.files {
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
    let missing = group
        .files
        .iter()
        .filter(|f| !good.contains(&f.name))
        .copied()
        .collect();
    Ok((marker, missing))
}

#[allow(clippy::too_many_arguments)]
fn fetch_group(
    groups: &[Group],
    n: usize,
    missing: &[ModelFile],
    packed: &mut [bool],
    marker: &mut Verified,
    state: &ModelState,
    last: bool,
) -> Result<(), String> {
    let group = &groups[n];
    let received = Cell::new(false);
    let mut came_packed: Vec<&str> = Vec::new();
    for file in missing {
        if packed[n] {
            let from = group
                .sources
                .packed
                .as_deref()
                .expect("a group downloaded packed has a place for it");
            let copy = file
                .packed
                .expect("every file of a group downloaded packed has a packed copy (`packed_for`)");
            match download_packed(
                &format!("{from}/{}.zst", file.name),
                copy,
                groups,
                packed,
                n,
                file,
                state,
                &received,
            ) {
                Ok(()) => {
                    came_packed.push(file.name);
                    continue;
                }
                Err(Failure::Again(last)) if received.get() => return Err(unfinished(&last)),
                Err(Failure::Again(why) | Failure::Final(why)) if group.optional => return Err(why),
                Err(Failure::Again(why) | Failure::Final(why)) => {
                    eprintln!(
                        "Mesa: the packed copy of {} couldn't be had, so it is downloaded as it is: {why}",
                        file.name
                    );
                    packed[n] = false;
                }
            }
        }
        if group.optional {
            return Err(format!("{} has no packed copy", file.name));
        }
        download_plain(groups, packed, n, file, state)?;
    }

    if last {
        state.set(ModelStatus::Verifying);
    }
    let dir = &group.dir;
    for file in missing {
        let part = part_path(dir, file);
        let mut right = sha256_of(&part).is_ok_and(|hash| hash == file.sha256);
        if !right && came_packed.contains(&file.name) && !group.optional {
            eprintln!(
                "Mesa: the packed copy of {} isn't the model, so it is downloaded as it is",
                file.name
            );
            let _ = fs::remove_file(&part);
            packed[n] = false;
            download_plain(groups, packed, n, file, state)?;
            if last {
                state.set(ModelStatus::Verifying);
            }
            right = sha256_of(&part).is_ok_and(|hash| hash == file.sha256);
        }
        if !right {
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
    write_marker(dir, marker)
}

fn progress_around(groups: &[Group], packed: &[bool], except: Option<(usize, &str)>) -> (u64, u64) {
    let (mut done, mut total) = (0, 0);
    for (n, group) in groups.iter().enumerate() {
        for file in &group.files {
            total += download_size(file, packed[n]);
            if except != Some((n, file.name)) {
                done += downloaded(&group.dir, file, packed[n]);
            }
        }
    }
    (done.min(total), total)
}

fn unfinished(last: &str) -> String {
    format!("The download didn't finish. Check the internet connection and try again. ({last})")
}

fn download_plain(
    groups: &[Group],
    packed: &[bool],
    n: usize,
    file: &ModelFile,
    state: &ModelState,
) -> Result<(), String> {
    let group = &groups[n];
    let (done_before, total) = progress_around(groups, packed, Some((n, file.name)));
    let (url, part) = (
        format!("{}/{}", group.sources.plain, file.name),
        part_path(&group.dir, file),
    );
    state.set(downloading(groups, packed));
    with_retries(|agent| try_download(agent, &url, &part, file.bytes, done_before, total, state)).map_err(|failure| {
        match failure {
            Failure::Again(last) => unfinished(&last),
            Failure::Final(why) => why,
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn download_packed(
    url: &str,
    copy: Packed,
    groups: &[Group],
    packed: &[bool],
    n: usize,
    file: &ModelFile,
    state: &ModelState,
    received: &Cell<bool>,
) -> Result<(), Failure> {
    let (done_before, total) = progress_around(groups, packed, Some((n, file.name)));
    let part = part_path(&groups[n].dir, file);
    state.set(downloading(groups, packed));
    with_retries(|agent| {
        try_packed(
            agent,
            url,
            copy,
            &part,
            file.bytes,
            (done_before, total),
            state,
            received,
        )
    })
}

fn megabytes(bytes: u64) -> String {
    format!("{} MB", bytes / 1_000_000)
}

enum Failure {
    Again(String),
    Final(String),
}

fn with_retries(mut attempt: impl FnMut(&ureq::Agent) -> Result<(), Failure>) -> Result<(), Failure> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_per_read(Some(SILENCE))
        .http_status_as_error(false)
        .build()
        .into();
    let mut last = String::new();
    for attempt_number in 0..ATTEMPTS {
        if attempt_number > 0 {
            thread::sleep(RETRY_PAUSE * attempt_number);
        }
        match attempt(&agent) {
            Ok(()) => return Ok(()),
            Err(Failure::Final(why)) => return Err(Failure::Final(why)),
            Err(Failure::Again(why)) => last = why,
        }
    }
    Err(Failure::Again(last))
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

#[allow(clippy::too_many_arguments)]
fn try_packed(
    agent: &ureq::Agent,
    url: &str,
    copy: Packed,
    part: &Path,
    bytes: u64,
    (done_before, total): (u64, u64),
    state: &ModelState,
    received: &Cell<bool>,
) -> Result<(), Failure> {
    let have = fs::metadata(part).map_or(0, |m| m.len());
    if have == bytes {
        return Ok(());
    }
    let mut piece = if have > bytes { 0 } else { copy.piece_of(have) };
    let mut request = agent.get(url).header("Accept-Encoding", "identity");
    if piece > 0 {
        request = request.header("Range", format!("bytes={}-", copy.offset(piece)));
    }
    let response = request.call().map_err(|e| Failure::Again(e.to_string()))?;
    let status = response.status().as_u16();
    match status {
        206 if piece > 0 => {}
        200 => piece = 0,
        408 | 429 | 500..=599 => return Err(Failure::Again(format!("the server answered {status}"))),
        _ => return Err(Failure::Final(format!("the server answered {status} for {url}"))),
    }
    let start = copy.offset(piece);
    let cant_write = |e: io::Error| Failure::Final(format!("Couldn't write {}: {e}", part.display()));
    let mut out = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(part)
        .map_err(cant_write)?;
    out.set_len(piece as u64 * copy.piece).map_err(cant_write)?;
    out.seek(SeekFrom::End(0)).map_err(cant_write)?;

    let counted = Counted {
        inner: response.into_body().into_reader(),
        read: 0,
        ended: false,
        received,
    };
    let mut input = BufReader::with_capacity(256 * 1024, counted);
    let mut unpacked = Vec::with_capacity(copy.piece.min(bytes) as usize);
    let mut buffer = vec![0u8; 256 * 1024];
    let mut reported = Instant::now() - PROGRESS_EVERY;
    while piece < copy.sizes.len() {
        let expected = copy.piece.min(bytes.saturating_sub(piece as u64 * copy.piece)) as usize;
        unpacked.clear();
        let mut frame = zstd::stream::read::Decoder::with_buffer(input)
            .map_err(|e| Failure::Final(e.to_string()))?
            .single_frame();
        let result = loop {
            match frame.read(&mut buffer) {
                Ok(0) => break Ok(()),
                Ok(n) if unpacked.len() + n > expected => {
                    break Err(io::Error::other("a piece is longer than it should be"));
                }
                Ok(n) => unpacked.extend_from_slice(&buffer[..n]),
                Err(e) => break Err(e),
            }
            if reported.elapsed() >= PROGRESS_EVERY {
                reported = Instant::now();
                state.set(ModelStatus::Downloading {
                    done: done_before + start + frame.get_ref().get_ref().read,
                    total,
                });
            }
        };
        input = frame.finish();
        let cut_off = input.get_ref().ended;
        match result {
            Ok(()) if unpacked.len() == expected => {}
            Ok(()) if cut_off => return Err(Failure::Again("the connection ended early".into())),
            Ok(()) => {
                return Err(Failure::Final(format!(
                    "a piece of {url} isn't as long as it should be"
                )));
            }
            Err(e) if cut_off => return Err(Failure::Again(e.to_string())),
            Err(e) => return Err(Failure::Final(format!("{url} couldn't be unpacked: {e}"))),
        }
        out.write_all(&unpacked).map_err(cant_write)?;
        piece += 1;
    }
    state.set(ModelStatus::Downloading {
        done: done_before + copy.bytes(),
        total,
    });
    Ok(())
}

struct Counted<'a, R> {
    inner: R,
    read: u64,
    ended: bool,
    received: &'a Cell<bool>,
}

impl<R: Read> Read for Counted<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let result = self.inner.read(buffer);
        match result {
            Ok(0) | Err(_) => self.ended = true,
            Ok(n) => {
                self.read += n as u64;
                self.received.set(true);
            }
        }
        result
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
    #[cfg(unix)]
    {
        use std::{ffi::CString, mem::MaybeUninit, os::unix::ffi::OsStrExt};
        let path = CString::new(dir.as_os_str().as_bytes()).ok()?;
        let mut stats = MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
            return None;
        }
        let stats = unsafe { stats.assume_init() };
        #[allow(clippy::useless_conversion)]
        let free = u64::from(stats.f_bavail).saturating_mul(u64::from(stats.f_frsize));
        Some(free)
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
                    packed: None,
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
        pub busy_after: bool,
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
                            range = value
                                .trim()
                                .split('-')
                                .next()
                                .and_then(|start| start.parse::<usize>().ok());
                        }
                    }
                    let nth = requests.fetch_add(1, Ordering::SeqCst);
                    if nth < behaviour.busy_first || (behaviour.busy_after && nth > behaviour.busy_first) {
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

        ensure(
            &dir,
            &files,
            &Sources::plain(&address),
            &ModelState::new(ModelStatus::Verifying),
        )
        .unwrap();

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
        let error = ensure(
            &dir,
            &files,
            &Sources::plain(&address),
            &ModelState::new(ModelStatus::Verifying),
        )
        .unwrap_err();
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
        ensure(
            &dir,
            &files,
            &Sources::plain(&address),
            &ModelState::new(ModelStatus::Verifying),
        )
        .unwrap();
        assert_eq!(fs::read(dir.join("a.gguf")).unwrap(), a);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }
}
