use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    time::SystemTime,
};

use crate::{embed::Embedder, index::lock, vectors::Record};

pub const EAGER_PIECES: usize = 16;
const MAX_FILES: usize = 128;

struct Example {
    len: u64,
    modified: Option<SystemTime>,
    records: Vec<Record>,
    complete: bool,
}

#[derive(Default)]
pub struct Examples {
    entries: Mutex<HashMap<String, Arc<Mutex<Option<Example>>>>>,
}

pub fn path_key(path: &Path) -> String {
    if cfg!(windows) {
        path.to_string_lossy().replace('/', "\\").to_lowercase()
    } else {
        path.to_string_lossy().into_owned()
    }
}

impl Examples {
    pub fn vectors(&self, embedder: &dyn Embedder, path: &Path, wanted: usize) -> Result<Vec<Record>, String> {
        let wanted = wanted.max(1);
        let slot = {
            let mut entries = lock(&self.entries);
            let key = path_key(path);
            if entries.len() >= MAX_FILES && !entries.contains_key(&key) {
                entries.retain(|_, slot| Arc::strong_count(slot) > 1);
            }
            entries.entry(key).or_default().clone()
        };
        let mut cached = lock(&slot);

        let now = std::fs::metadata(path)
            .ok()
            .map(|meta| (meta.len(), meta.modified().ok()));
        if let (Some(example), Some((len, modified))) = (cached.as_ref(), now)
            && (example.len != len || example.modified != modified)
        {
            *cached = None;
        }
        if let Some(example) = cached.as_ref()
            && (example.records.len() >= wanted || example.complete || now.is_none())
        {
            return Ok(example.records.iter().take(wanted).cloned().collect());
        }
        let Some((len, modified)) = now else {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            return Err(format!("{name} isn't there any more."));
        };

        let read = wanted.max(EAGER_PIECES);
        let records = embedder.embed_example(path, read).map_err(|e| e.to_string())?;
        let taken = records.iter().take(wanted).cloned().collect();
        let complete = records.len() < read;
        *cached = Some(Example {
            len,
            modified,
            records,
            complete,
        });
        Ok(taken)
    }
}
