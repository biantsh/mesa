use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::OnceLock,
};

const PHOTOS_LIBRARY: &str = ".photoslibrary";
const PHOTOS_ORIGINALS: &[&str] = &["originals", "Masters"];

pub fn is_photos_library(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| ends_with_ignoring_case(&name.to_string_lossy(), PHOTOS_LIBRARY))
}

pub fn is_photos_originals(name: &str) -> bool {
    PHOTOS_ORIGINALS
        .iter()
        .any(|originals| originals.eq_ignore_ascii_case(name))
}

fn is_photos_own(path: &Path) -> bool {
    let mut parts = path.components().map(|part| part.as_os_str().to_string_lossy());
    while let Some(part) = parts.next() {
        if ends_with_ignoring_case(&part, PHOTOS_LIBRARY) {
            return parts.next().is_some_and(|below| !is_photos_originals(&below));
        }
    }
    false
}

fn ends_with_ignoring_case(text: &str, end: &str) -> bool {
    let (text, end) = (text.as_bytes(), end.as_bytes());
    text.len() >= end.len() && text[text.len() - end.len()..].eq_ignore_ascii_case(end)
}

pub fn is_unindexable(path: &Path) -> bool {
    is_photos_own(path) || is_system(path)
}

pub fn is_wholly_unindexable(path: &Path) -> bool {
    is_unindexable(path) && !locked().exceptions.iter().any(|exception| is_inside(exception, path))
}

pub fn is_app_data(path: &Path) -> bool {
    locked()
        .app_data
        .as_deref()
        .is_some_and(|app_data| is_inside(path, app_data) && path.components().count() == app_data.components().count())
}

fn is_system(path: &Path) -> bool {
    let locked = locked();
    locked.folders.iter().any(|folder| is_inside(path, folder))
        && !locked.exceptions.iter().any(|exception| is_inside(path, exception))
}

pub fn could_be_unindexable(name: &str) -> bool {
    locked()
        .folders
        .iter()
        .any(|folder| folder.file_name().is_some_and(|called| same(called, OsStr::new(name))))
}

pub fn children_may_differ(folder: &Path) -> bool {
    let locked = locked();
    is_photos_library(folder)
        || locked
            .folders
            .iter()
            .chain(&locked.exceptions)
            .any(|below| is_inside(below, folder) && below.components().count() > folder.components().count())
}

fn is_inside(path: &Path, folder: &Path) -> bool {
    let mut below = path.components();
    folder
        .components()
        .all(|part| below.next().is_some_and(|c| same(c.as_os_str(), part.as_os_str())))
}

fn same(a: &OsStr, b: &OsStr) -> bool {
    if cfg!(target_os = "linux") {
        a == b
    } else {
        a.eq_ignore_ascii_case(b)
    }
}

#[derive(Default)]
struct Locked {
    folders: Vec<PathBuf>,
    exceptions: Vec<PathBuf>,
    app_data: Option<PathBuf>,
}

fn locked() -> &'static Locked {
    static LOCKED: OnceLock<Locked> = OnceLock::new();
    LOCKED.get_or_init(Locked::here)
}

impl Locked {
    #[cfg(target_os = "macos")]
    fn here() -> Self {
        let app_data = std::env::home_dir().map(|home| home.join("Library"));
        Self {
            folders: MACOS_FOLDERS
                .iter()
                .map(PathBuf::from)
                .chain(app_data.clone())
                .collect(),
            exceptions: app_data
                .iter()
                .flat_map(|library| ["Mobile Documents", "CloudStorage"].map(|name| library.join(name)))
                .collect(),
            app_data,
        }
    }

    #[cfg(windows)]
    fn here() -> Self {
        let app_data = std::env::home_dir().map(|home| home.join("AppData"));
        Self {
            folders: [
                ("SystemRoot", r"C:\Windows"),
                ("ProgramFiles", r"C:\Program Files"),
                ("ProgramFiles(x86)", r"C:\Program Files (x86)"),
            ]
            .into_iter()
            .map(|(variable, usual)| std::env::var_os(variable).map_or_else(|| usual.into(), Into::into))
            .chain(app_data.clone())
            .collect(),
            exceptions: Vec::new(),
            app_data,
        }
    }

    #[cfg(target_os = "linux")]
    fn here() -> Self {
        Self {
            folders: LINUX_FOLDERS.iter().map(PathBuf::from).collect(),
            exceptions: LINUX_EXCEPTIONS.iter().map(PathBuf::from).collect(),
            app_data: None,
        }
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    fn here() -> Self {
        Self::default()
    }
}

#[cfg(target_os = "linux")]
const LINUX_FOLDERS: &[&str] = &[
    "/proc",
    "/sys",
    "/dev",
    "/run",
    "/boot",
    "/efi",
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib32",
    "/lib64",
    "/libx32",
    "/etc",
    "/var",
    "/opt",
    "/snap",
    "/nix",
    "/lost+found",
];

#[cfg(target_os = "linux")]
const LINUX_EXCEPTIONS: &[&str] = &["/run/media", "/var/home", "/var/mnt"];

#[cfg(target_os = "macos")]
const MACOS_FOLDERS: &[&str] = &[
    "/System",
    "/Library",
    "/Applications",
    "/private",
    "/usr",
    "/bin",
    "/sbin",
    "/cores",
    "/opt",
];

#[cfg(test)]
mod portable_tests {
    use std::path::Path;

    use super::is_inside;

    #[test]
    fn inside_matches_whole_components() {
        let system = Path::new("/System");
        assert!(is_inside(Path::new("/System"), system));
        assert!(is_inside(Path::new("/System/Library/CoreServices"), system));
        if cfg!(target_os = "linux") {
            assert!(!is_inside(Path::new("/SYSTEM/library"), system));
        } else {
            assert!(is_inside(Path::new("/SYSTEM/library"), system));
        }
        assert!(!is_inside(Path::new("/Systems"), system));
        assert!(!is_inside(Path::new("/Users/me/System"), system));
        assert!(!is_inside(Path::new("/"), system));
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn windows_folder_unindexable() {
        let windows = locked().folders[0].clone();
        assert!(is_unindexable(&windows));
        assert!(is_unindexable(
            &windows.join("System32").join("drivers").join("etc").join("hosts")
        ));
        assert!(is_unindexable(Path::new(&windows.to_string_lossy().to_uppercase())));
        assert!(is_unindexable(
            &PathBuf::from(windows.to_string_lossy().to_lowercase()).join("WinSxS")
        ));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use super::*;

    #[test]
    fn the_user_library_is_locked_except_for_cloud_folders() {
        let library = std::env::home_dir().unwrap().join("Library");
        assert!(is_app_data(&library));
        assert!(!is_app_data(&library.join("Caches")));
        assert!(is_unindexable(&library.join("Caches")));
        assert!(is_wholly_unindexable(&library.join("Caches")));
        assert!(is_unindexable(&library));
        assert!(!is_wholly_unindexable(&library));
        assert!(children_may_differ(&library));
        assert!(could_be_unindexable("Library"));
        assert!(!is_unindexable(
            &library.join("Mobile Documents/com~apple~CloudDocs/notes.pdf")
        ));
        assert!(!is_unindexable(&library.join("CloudStorage/Dropbox")));
        assert!(!children_may_differ(&library.join("Caches")));
    }
}
