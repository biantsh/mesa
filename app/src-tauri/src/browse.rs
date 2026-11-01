use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::index::IndexStatus;

#[cfg(windows)]
const COMPUTER_LABEL: &str = "This PC";
#[cfg(not(windows))]
const COMPUTER_LABEL: &str = "Computer";

const INDEXABLE: &[(Category, &[&str])] = &[
    (Category::Image, &["jpg", "jpeg", "png", "gif", "webp", "bmp", "heic"]),
    (Category::Video, &["mp4", "mov", "mkv", "avi", "webm"]),
    (Category::Audio, &["mp3", "wav", "flac", "m4a", "ogg"]),
    (Category::Document, &["pdf", "txt", "md", "docx"]),
];

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Drive,
    Folder,
    File,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Image,
    Video,
    Audio,
    Document,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub kind: Kind,
    pub category: Option<Category>,
    pub readable: bool,
    pub has_contents: bool,
    pub index: IndexStatus,
    pub droppable: bool,
}

impl Entry {
    fn new(name: String, path: &Path, kind: Kind) -> Self {
        Self {
            name,
            path: path.to_string_lossy().into_owned(),
            kind,
            category: None,
            readable: true,
            has_contents: false,
            index: IndexStatus::NotIndexed,
            droppable: false,
        }
    }

    fn drive(root: &Path) -> Self {
        Self {
            readable: fs::read_dir(root).is_ok(),
            ..Self::new(display_name(root), root, Kind::Drive)
        }
    }

    fn folder(name: String, path: &Path) -> Self {
        let (readable, has_contents) = probe(path);
        Self {
            readable,
            has_contents,
            ..Self::new(name, path, Kind::Folder)
        }
    }

    fn file(name: String, path: &Path, category: Category) -> Self {
        Self {
            category: Some(category),
            ..Self::new(name, path, Kind::File)
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crumb {
    pub name: String,
    pub path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub path: String,
    pub parent: Option<String>,
    pub breadcrumbs: Vec<Crumb>,
    pub entries: Vec<Entry>,
}

pub fn list(path: &str) -> io::Result<Listing> {
    let entries = if path.is_empty() {
        root_entries()
    } else {
        read_entries(Path::new(path))?
    };
    Ok(Listing {
        path: path.to_owned(),
        parent: parent(path),
        breadcrumbs: breadcrumbs(path),
        entries,
    })
}

#[cfg(windows)]
pub fn roots() -> Vec<PathBuf> {
    ('A'..='Z')
        .map(|letter| PathBuf::from(format!("{letter}:\\")))
        .filter(|root| root.exists())
        .collect()
}

#[cfg(not(windows))]
pub fn roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/")]
}

fn root_entries() -> Vec<Entry> {
    roots().iter().map(|root| Entry::drive(root)).collect()
}

fn read_entries(dir: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in fs::read_dir(dir)? {
        let Ok(item) = item else { continue };
        let Some((name, shown)) = classify(&item) else { continue };
        let path = item.path();
        entries.push(match shown {
            Shown::Folder => Entry::folder(name, &path),
            Shown::File(category) => Entry::file(name, &path, category),
        });
    }
    entries.sort_by_cached_key(|e| (e.kind == Kind::File, e.name.to_lowercase()));
    Ok(entries)
}

pub enum Shown {
    Folder,
    File(Category),
}

pub fn classify(item: &fs::DirEntry) -> Option<(String, Shown)> {
    let name = item.file_name().to_string_lossy().into_owned();
    if is_hidden(item, &name) {
        return None;
    }
    let shown = if is_dir(item)? {
        Shown::Folder
    } else {
        Shown::File(category_of(&name)?)
    };
    Some((name, shown))
}

const EMPTY_CHECK_LIMIT: usize = 200;

fn probe(dir: &Path) -> (bool, bool) {
    match fs::read_dir(dir) {
        Err(_) => (false, false),
        Ok(items) => {
            let has_contents = items
                .take(EMPTY_CHECK_LIMIT)
                .flatten()
                .any(|item| classify(&item).is_some());
            (true, has_contents)
        }
    }
}

fn is_dir(item: &fs::DirEntry) -> Option<bool> {
    let file_type = item.file_type().ok()?;
    if file_type.is_symlink() {
        fs::metadata(item.path()).ok().map(|m| m.is_dir())
    } else {
        Some(file_type.is_dir())
    }
}

#[cfg_attr(not(windows), allow(unused_variables))]
fn is_hidden(item: &fs::DirEntry, name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        if let Ok(metadata) = item.metadata() {
            return metadata.file_attributes() & (HIDDEN | SYSTEM) != 0;
        }
    }
    false
}

pub fn category_of(name: &str) -> Option<Category> {
    let extension = Path::new(name).extension()?.to_str()?;
    INDEXABLE
        .iter()
        .find(|(_, extensions)| extensions.iter().any(|e| e.eq_ignore_ascii_case(extension)))
        .map(|(category, _)| *category)
}

fn parent(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let parent = Path::new(path).parent();
    Some(parent.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default())
}

fn breadcrumbs(path: &str) -> Vec<Crumb> {
    let mut crumbs = vec![Crumb {
        name: COMPUTER_LABEL.to_owned(),
        path: String::new(),
    }];
    if !path.is_empty() {
        let mut chain: Vec<&Path> = Path::new(path)
            .ancestors()
            .filter(|p| !p.as_os_str().is_empty())
            .collect();
        chain.reverse();
        crumbs.extend(chain.into_iter().map(|p| Crumb {
            name: display_name(p),
            path: p.to_string_lossy().into_owned(),
        }));
    }
    crumbs
}

pub fn display_name(path: &Path) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => {
            let full = path.to_string_lossy();
            let trimmed = full.trim_end_matches(['\\', '/']);
            if trimmed.is_empty() {
                full.into_owned()
            } else {
                trimmed.to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mesa-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn folders_first_then_case_insensitive_name() {
        let dir = temp_dir("sort");
        fs::create_dir(dir.join("zeta")).unwrap();
        fs::create_dir(dir.join("Alpha")).unwrap();
        fs::write(dir.join("b.png"), "").unwrap();
        fs::write(dir.join("A.png"), "").unwrap();

        let listing = list(dir.to_str().unwrap()).unwrap();
        let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "zeta", "A.png", "b.png"]);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn breadcrumbs_from_top_level() {
        let dir = temp_dir("crumbs");
        let path = dir.to_string_lossy().into_owned();

        let crumbs = breadcrumbs(&path);
        assert_eq!(crumbs.first().unwrap().path, "");
        assert_eq!(crumbs.first().unwrap().name, COMPUTER_LABEL);
        assert_eq!(crumbs.last().unwrap().path, path);
        assert_eq!(crumbs.last().unwrap().name, dir.file_name().unwrap().to_string_lossy());
        for pair in crumbs[1..].windows(2) {
            assert_eq!(Path::new(&pair[1].path).parent(), Some(Path::new(&pair[0].path)));
        }

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn extension_match_ignores_case() {
        assert_eq!(category_of("A.PnG"), Some(Category::Image));
        assert_eq!(category_of("clip.mkv"), Some(Category::Video));
        assert_eq!(category_of("notes.md"), Some(Category::Document));
        assert_eq!(category_of("archive.zip"), None);
    }
}
