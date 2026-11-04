//! Saving writes a new file and renames it into place, so a crash can't leave half a file.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};

use crate::{
    embed::Device,
    index::{Tuning, lock},
};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub gpu_acceleration: bool,
    pub turbo_indexing: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            gpu_acceleration: true,
            turbo_indexing: false,
        }
    }
}

impl Settings {
    pub fn tuning(&self) -> Tuning {
        Tuning {
            turbo: self.turbo_indexing,
        }
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
        *lock(&self.current)
    }

    pub fn set(&self, settings: Settings) -> io::Result<()> {
        let mut current = lock(&self.current);
        fs::create_dir_all(&self.dir)?;
        let temporary = self.dir.join(format!("{FILE}.new"));
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(&settings).map_err(io::Error::other)?,
        )?;
        fs::rename(&temporary, self.dir.join(FILE))?;
        *current = settings;
        Ok(())
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
        store
            .set(Settings {
                gpu_acceleration: false,
                ..Settings::default()
            })
            .unwrap();
        assert!(!SettingsStore::open(&dir).get().gpu_acceleration);
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
