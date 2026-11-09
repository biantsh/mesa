use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, RwLock},
};

use crate::index::lock;

const FILE: &str = "folder-access.json";
static UNASKED: RwLock<Vec<PathBuf>> = RwLock::new(Vec::new());

pub fn may_open(path: &Path) -> bool {
    UNASKED
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .all(|folder| !path.starts_with(folder))
}

pub struct FolderAccess {
    file: PathBuf,
    protected: Vec<Folder>,
    asked: Mutex<BTreeSet<PathBuf>>,
    published: bool,
}

impl FolderAccess {
    pub fn open(dir: &Path, home: Option<&Path>) -> Self {
        Self::make(dir, home.map(protected_in).unwrap_or_default(), true)
    }

    fn make(dir: &Path, protected: Vec<Folder>, published: bool) -> Self {
        let kept: Vec<PathBuf> = fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let asked = kept
            .into_iter()
            .filter(|path| protected.iter().any(|folder| &folder.path == path))
            .collect();
        let access = Self {
            file: dir.join(FILE),
            protected,
            asked: Mutex::new(asked),
            published,
        };
        access.publish(&lock(&access.asked));
        access
    }

    pub fn reading(&self, path: &Path) {
        self.note(|folder| path.starts_with(folder));
    }

    pub fn indexing<'a>(&self, paths: impl IntoIterator<Item = &'a Path>) {
        let paths: Vec<&Path> = paths.into_iter().collect();
        self.note(|folder| {
            paths
                .iter()
                .any(|path| path.starts_with(folder) || folder.starts_with(path))
        });
    }

    pub fn asking_all(&self) {
        self.note(|_| true);
    }

    pub fn folders(&self) -> Vec<Protected> {
        let asked = lock(&self.asked);
        self.protected
            .iter()
            .map(|folder| Protected {
                path: folder.path.clone(),
                name: folder.name.clone(),
                asked: asked.contains(&folder.path),
            })
            .collect()
    }

    pub fn pane_for(&self, path: &Path) -> Option<Pane> {
        self.protected
            .iter()
            .find(|folder| folder.path == path)
            .map(|folder| folder.pane)
    }

    fn note(&self, applies: impl Fn(&Path) -> bool) {
        let mut asked = lock(&self.asked);
        let before = asked.len();
        asked.extend(
            self.protected
                .iter()
                .map(|folder| &folder.path)
                .filter(|path| applies(path))
                .cloned(),
        );
        if asked.len() == before {
            return;
        }
        self.publish(&asked);
        let _ = fs::create_dir_all(self.file.parent().unwrap_or(Path::new(".")));
        let _ = serde_json::to_vec_pretty(&*asked).map(|bytes| fs::write(&self.file, bytes));
    }

    fn publish(&self, asked: &BTreeSet<PathBuf>) {
        if !self.published {
            return;
        }
        *UNASKED.write().unwrap_or_else(|e| e.into_inner()) = self
            .protected
            .iter()
            .map(|folder| &folder.path)
            .filter(|path| !asked.contains(*path))
            .cloned()
            .collect();
    }
}

struct Folder {
    path: PathBuf,
    name: String,
    pane: Pane,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Protected {
    pub path: PathBuf,
    pub name: String,
    pub asked: bool,
}

#[cfg(target_os = "macos")]
fn protected_in(home: &Path) -> Vec<Folder> {
    [
        ("Desktop", "Desktop", Pane::FilesAndFolders),
        ("Documents", "Documents", Pane::FilesAndFolders),
        ("Downloads", "Downloads", Pane::FilesAndFolders),
        ("Pictures/Photos Library.photoslibrary", "Photos library", Pane::Photos),
        ("Music/Music", "Music library", Pane::Media),
        ("Movies/TV", "TV library", Pane::Media),
    ]
    .iter()
    .map(|&(path, name, pane)| Folder {
        path: home.join(path),
        name: name.to_string(),
        pane,
    })
    .collect()
}

#[cfg(not(target_os = "macos"))]
fn protected_in(_home: &Path) -> Vec<Folder> {
    Vec::new()
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Pane {
    FilesAndFolders,
    Photos,
    Media,
}

impl Pane {
    pub fn settings_url(self) -> &'static str {
        match self {
            Self::FilesAndFolders => "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders",
            Self::Photos => "x-apple.systempreferences:com.apple.preference.security?Privacy_Photos",
            Self::Media => "x-apple.systempreferences:com.apple.preference.security?Privacy_Media",
        }
    }
}
