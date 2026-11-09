use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use super::{
    Device, Embedder,
    llama::{LlamaServer, Locations},
    model::{self, ModelFile},
    pipeline::Pipeline,
};

const MOST_HEADER: usize = 64 * 1024 * 1024;
const FIRST_ASK: usize = 1024 * 1024;

pub fn embedder(data_dir: &Path, resource_dir: Option<&Path>, device: Device) -> Result<Arc<dyn Embedder>, String> {
    let real = Locations::new(data_dir, resource_dir);
    let folder = folder(data_dir);
    make(&model::FILES, &real.models, model::SOURCE, &folder)?;
    let locations = Locations {
        models: folder,
        log: data_dir.join("logs").join("estimate-server.log"),
        ..real
    };
    let server = LlamaServer::new(locations);
    server.choose_device(device);
    Ok(Arc::new(Pipeline::new(server)))
}

pub fn make(files: &[ModelFile], models: &Path, source: &str, folder: &Path) -> Result<(), String> {
    fs::create_dir_all(folder).map_err(|e| format!("can't make {}: {e}", folder.display()))?;
    for file in files {
        let header = header_of(file, models, source)?;
        let path = folder.join(file.name);
        let same = fs::metadata(&path).is_ok_and(|meta| meta.len() == file.bytes)
            && read_start(&path, header.len()).is_ok_and(|start| start == header);
        if same {
            continue;
        }
        let mut out = File::create(&path).map_err(|e| format!("can't write {}: {e}", path.display()))?;
        out.write_all(&header)
            .map_err(|e| format!("can't write {}: {e}", path.display()))?;
        out.set_len(file.bytes)
            .map_err(|e| format!("can't write {}: {e}", path.display()))?;
    }
    Ok(())
}

fn header_of(file: &ModelFile, models: &Path, source: &str) -> Result<Vec<u8>, String> {
    for local in [models.join(file.name), models.join(format!("{}.part", file.name))] {
        if let Ok(start) = read_start(&local, MOST_HEADER)
            && let Ok(Some(length)) = header_length(&start)
        {
            return Ok(start[..length].to_vec());
        }
    }
    download_header(&format!("{source}/{}", file.name))
}

fn read_start(path: &Path, most: usize) -> std::io::Result<Vec<u8>> {
    let mut start = Vec::new();
    File::open(path)?.take(most as u64).read_to_end(&mut start)?;
    Ok(start)
}

fn download_header(url: &str) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_per_read(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut have: Vec<u8> = Vec::new();
    let mut ask = FIRST_ASK;
    loop {
        let range = format!("bytes={}-{}", have.len(), have.len() + ask - 1);
        let response = agent
            .get(url)
            .header("Range", &range)
            .header("Accept-Encoding", "identity")
            .call()
            .map_err(|e| format!("couldn't get the start of the model: {e}"))?;
        if response.status().as_u16() != 206 {
            return Err(format!(
                "the server wouldn't give part of the model ({})",
                response.status()
            ));
        }
        let mut more = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(ask as u64)
            .read_to_end(&mut more)
            .map_err(|e| e.to_string())?;
        if more.is_empty() {
            return Err("the model ended before its header did".into());
        }
        have.extend(more);
        match header_length(&have)? {
            Some(length) => {
                have.truncate(length);
                return Ok(have);
            }
            None if have.len() >= MOST_HEADER => return Err("the model's header is longer than any should be".into()),
            None => ask *= 2,
        }
    }
}

pub fn header_length(bytes: &[u8]) -> Result<Option<usize>, String> {
    if bytes.get(..4).is_some_and(|magic| magic != b"GGUF") {
        return Err("not a GGUF file".into());
    }
    Ok(Reader { bytes, at: 0 }.header().ok())
}

struct Short;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

fn number_size(kind: u32) -> Option<usize> {
    match kind {
        0 | 1 | 7 => Some(1),
        2 | 3 => Some(2),
        4..=6 => Some(4),
        10..=12 => Some(8),
        _ => None,
    }
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Short> {
        let end = self.at.checked_add(n).ok_or(Short)?;
        let slice = self.bytes.get(self.at..end).ok_or(Short)?;
        self.at = end;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32, Short> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| Short)?))
    }

    fn u64(&mut self) -> Result<u64, Short> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().map_err(|_| Short)?))
    }

    fn skip_string(&mut self) -> Result<(), Short> {
        let length = self.u64()?;
        self.take(usize::try_from(length).map_err(|_| Short)?)?;
        Ok(())
    }

    fn value(&mut self, kind: u32) -> Result<Option<u64>, Short> {
        match kind {
            8 => self.skip_string().map(|()| None),
            9 => {
                let (inner, count) = (self.u32()?, self.u64()?);
                match number_size(inner) {
                    Some(size) => {
                        self.take(
                            usize::try_from(count)
                                .ok()
                                .and_then(|n| n.checked_mul(size))
                                .ok_or(Short)?,
                        )?;
                    }
                    None => {
                        for _ in 0..count {
                            self.value(inner)?;
                        }
                    }
                }
                Ok(None)
            }
            4 => self.u32().map(|n| Some(u64::from(n))),
            10 => self.u64().map(Some),
            _ => {
                self.take(number_size(kind).ok_or(Short)?)?;
                Ok(None)
            }
        }
    }

    fn header(&mut self) -> Result<usize, Short> {
        self.take(4)?;
        let _version = self.u32()?;
        let (tensors, settings) = (self.u64()?, self.u64()?);
        let mut alignment = 32;
        for _ in 0..settings {
            let key_length = self.u64()?;
            let key = self.take(usize::try_from(key_length).map_err(|_| Short)?)?.to_vec();
            let kind = self.u32()?;
            let value = self.value(kind)?;
            if key == b"general.alignment"
                && let Some(n) = value.filter(|&n| n > 0)
            {
                alignment = n as usize;
            }
        }
        for _ in 0..tensors {
            self.skip_string()?;
            let dimensions = self.u32()?;
            self.take(8 * dimensions as usize)?;
            self.u32()?;
            self.u64()?;
        }
        Ok(self.at.div_ceil(alignment) * alignment)
    }
}

fn folder(data_dir: &Path) -> PathBuf {
    data_dir.join("estimate").join("models")
}

pub fn remove(data_dir: &Path) {
    let _ = fs::remove_dir_all(folder(data_dir));
}
