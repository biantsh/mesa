use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, PoisonError},
    time::SystemTime,
};

use crate::{
    embed::{Embedder, MOST_PIECES},
    index::lock,
    vectors::Record,
};

pub const FIRST_PIECES: usize = 2;
pub const MOST_FILES: usize = 16;
const MAX_FILES: usize = 128;

struct Example {
    len: u64,
    modified: Option<SystemTime>,
    records: Vec<Record>,
    slots: usize,
    reading: bool,
    failed: Option<String>,
    text: Option<String>,
}

#[derive(Default)]
struct Entry {
    state: Mutex<Option<Example>>,
    changed: Condvar,
}

impl Entry {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Example>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[derive(Default)]
pub struct Examples {
    entries: Mutex<HashMap<String, Arc<Entry>>>,
}

#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub made: usize,
    pub of: usize,
    pub reading: bool,
}

pub fn path_key(path: &Path) -> String {
    if cfg!(windows) {
        path.to_string_lossy().replace('/', "\\").to_lowercase()
    } else {
        path.to_string_lossy().into_owned()
    }
}

impl Examples {
    fn entry(&self, path: &Path) -> Arc<Entry> {
        let mut entries = lock(&self.entries);
        let key = path_key(path);
        if entries.len() >= MAX_FILES && !entries.contains_key(&key) {
            entries.retain(|_, entry| {
                Arc::strong_count(entry) > 1 || entry.lock().as_ref().is_some_and(|example| example.reading)
            });
        }
        entries.entry(key).or_default().clone()
    }

    pub fn add(&self, embedder: &Arc<dyn Embedder>, path: &Path) -> Result<(), String> {
        self.add_reading(embedder, path, false)
    }

    pub fn add_reading(&self, embedder: &Arc<dyn Embedder>, path: &Path, read_text: bool) -> Result<(), String> {
        let entry = self.entry(path);
        let now = std::fs::metadata(path)
            .ok()
            .map(|meta| (meta.len(), meta.modified().ok()));
        {
            let mut state = entry.lock();
            while state
                .as_ref()
                .is_some_and(|example| example.reading && example.records.is_empty() && example.failed.is_none())
            {
                state = entry.changed.wait(state).unwrap_or_else(PoisonError::into_inner);
            }
            let current = |example: &Example| {
                now.is_none_or(|(len, modified)| example.len == len && example.modified == modified)
            };
            match state.as_ref() {
                Some(example) if current(example) && example.failed.is_none() => return Ok(()),
                _ if now.is_none() => {
                    return match state.as_ref() {
                        Some(example) if !example.records.is_empty() => Ok(()),
                        _ => Err(format!("{} isn't there any more.", name_of(path))),
                    };
                }
                _ => {}
            }
            let (len, modified) = now.expect("there");
            *state = Some(Example {
                len,
                modified,
                records: Vec::new(),
                slots: 0,
                reading: true,
                failed: None,
                text: None,
            });
        }

        let first = embedder.embed_example(path, 0, FIRST_PIECES).map_err(|e| e.to_string());
        let picture = read_text
            && path.file_name().is_some_and(|name| {
                crate::browse::category_of(&name.to_string_lossy()) == Some(crate::browse::Category::Image)
            });
        let more = {
            let mut state = entry.lock();
            let example = state.as_mut().expect("set above");
            let more = match &first {
                Ok((records, slots)) => {
                    example.records = records.clone();
                    example.slots = *slots;
                    *slots > records.len()
                }
                Err(why) => {
                    example.failed = Some(why.clone());
                    false
                }
            };
            example.reading = more || (picture && first.is_ok());
            entry.changed.notify_all();
            more
        };
        first?;
        if picture {
            let (entry, embedder, path) = (entry.clone(), Arc::clone(embedder), path.to_path_buf());
            let (len, modified) = now.expect("there");
            std::thread::spawn(move || {
                crate::power::below_the_page();
                let text = embedder.read_text(&path);
                if let Some(example) = entry
                    .lock()
                    .as_mut()
                    .filter(|example| example.len == len && example.modified == modified)
                {
                    example.text = text;
                    example.reading = false;
                }
                entry.changed.notify_all();
            });
        }
        if more {
            let (entry, embedder, path) = (entry.clone(), Arc::clone(embedder), path.to_path_buf());
            let (len, modified) = now.expect("there");
            let as_read = move |example: &Example| example.len == len && example.modified == modified;
            std::thread::spawn(move || {
                crate::power::below_the_page();
                let _ = embedder.embed_example_each(&path, FIRST_PIECES, MOST_PIECES, &mut |records| {
                    if let Some(example) = entry.lock().as_mut().filter(|example| as_read(example)) {
                        example.records.extend(records);
                    }
                });
                if let Some(example) = entry.lock().as_mut().filter(|example| as_read(example)) {
                    example.reading = false;
                }
                entry.changed.notify_all();
            });
        }
        Ok(())
    }

    pub fn vectors(&self, embedder: &Arc<dyn Embedder>, path: &Path) -> Result<(Vec<Record>, usize), String> {
        self.add(embedder, path)?;
        let entry = self.entry(path);
        let state = entry.lock();
        match state.as_ref() {
            Some(example) if !example.records.is_empty() => {
                Ok((example.records.clone(), example.slots.max(example.records.len())))
            }
            Some(example) => Err(example
                .failed
                .clone()
                .unwrap_or_else(|| format!("{} has nothing to search with.", name_of(path)))),
            None => Err(format!("{} isn't there any more.", name_of(path))),
        }
    }

    pub fn text(&self, path: &Path) -> Option<String> {
        let entries = lock(&self.entries);
        let entry = entries.get(&path_key(path))?;
        entry.lock().as_ref().and_then(|example| example.text.clone())
    }

    pub fn progress(&self, paths: &[PathBuf]) -> Vec<Option<Progress>> {
        let entries = lock(&self.entries);
        paths
            .iter()
            .map(|path| {
                let entry = entries.get(&path_key(path))?;
                let state = entry.lock();
                let example = state.as_ref()?;
                Some(Progress {
                    made: example.records.len(),
                    of: example.slots.max(example.records.len()),
                    reading: example.reading,
                })
            })
            .collect()
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn shares(wanted: &[usize], budget: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..wanted.len()).collect();
    order.sort_by_key(|&file| wanted[file]);
    let mut shares = vec![0; wanted.len()];
    let mut left = budget;
    for (n, &file) in order.iter().enumerate() {
        let even = left / (wanted.len() - n);
        shares[file] = wanted[file].min(even).max(1);
        left = left.saturating_sub(shares[file]);
    }
    shares
}
