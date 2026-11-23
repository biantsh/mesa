//! Saving writes a new file and renames it into place, so a crash can't leave half a file.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};

use crate::{
    browse::{Categories, Category},
    embed::Device,
    index::lock,
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub gpu_acceleration: bool,
    #[serde(skip_serializing)]
    pub indexed_types: Option<Vec<Category>>,
    pub search_hotkey: Option<String>,
    pub always_full_speed: bool,
    pub start_at_sign_in: bool,
    pub sign_in_entry_made: bool,
    pub check_for_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            gpu_acceleration: true,
            indexed_types: None,
            search_hotkey: None,
            always_full_speed: false,
            start_at_sign_in: true,
            sign_in_entry_made: false,
            check_for_updates: true,
        }
    }
}

impl Settings {
    pub fn indexed_before(&self) -> Option<Categories> {
        self.indexed_types.as_deref().map(Categories::of)
    }

    pub fn device(&self) -> Device {
        if self.gpu_acceleration {
            Device::Auto
        } else {
            Device::Cpu
        }
    }
}

const FILE: &str = "settings.json";

pub struct SettingsStore {
    dir: PathBuf,
    current: Mutex<Settings>,
}

impl SettingsStore {
    pub fn open(dir: &Path) -> Self {
        let settings = fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            dir: dir.to_owned(),
            current: Mutex::new(settings),
        }
    }

    pub fn get(&self) -> Settings {
        lock(&self.current).clone()
    }

    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> io::Result<Settings> {
        let mut current = lock(&self.current);
        let mut next = current.clone();
        change(&mut next);
        fs::create_dir_all(&self.dir)?;
        let temporary = self.dir.join(format!("{FILE}.new"));
        fs::write(&temporary, serde_json::to_vec_pretty(&next).map_err(io::Error::other)?)?;
        fs::rename(&temporary, self.dir.join(FILE))?;
        *current = next.clone();
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mesa-settings-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn corrupt_file_gives_defaults() {
        let dir = temp_dir("broken");
        fs::write(dir.join(FILE), "{ \"gpuAcceleration\": fal").unwrap();
        let store = SettingsStore::open(&dir);
        assert_eq!(store.get(), Settings::default());
        store.update(|settings| settings.gpu_acceleration = false).unwrap();
        assert!(!SettingsStore::open(&dir).get().gpu_acceleration);
    }

    #[test]
    fn changes_made_at_once_are_all_kept() {
        let dir = temp_dir("at-once");
        let store = std::sync::Arc::new(SettingsStore::open(&dir));
        for round in 0..50 {
            let on = round % 2 == 0;
            let start = std::sync::Arc::new(std::sync::Barrier::new(2));
            let speed = std::thread::spawn({
                let (store, start) = (store.clone(), start.clone());
                move || {
                    start.wait();
                    store.update(|settings| settings.always_full_speed = on).unwrap();
                }
            });
            let updates = std::thread::spawn({
                let (store, start) = (store.clone(), start.clone());
                move || {
                    start.wait();
                    store.update(|settings| settings.check_for_updates = on).unwrap();
                }
            });
            speed.join().unwrap();
            updates.join().unwrap();
            let kept = SettingsStore::open(&dir).get();
            assert_eq!(
                (kept.always_full_speed, kept.check_for_updates),
                (on, on),
                "round {round}"
            );
        }
    }

    #[test]
    fn unknown_settings_ignored() {
        let dir = temp_dir("unknown");
        fs::write(dir.join(FILE), r#"{"gpuAcceleration": false, "fromTheFuture": 7}"#).unwrap();
        assert_eq!(
            SettingsStore::open(&dir).get(),
            Settings {
                gpu_acceleration: false,
                ..Settings::default()
            }
        );
        fs::write(dir.join(FILE), r#"{"fromTheFuture": 7}"#).unwrap();
        assert_eq!(SettingsStore::open(&dir).get(), Settings::default());
    }
}
