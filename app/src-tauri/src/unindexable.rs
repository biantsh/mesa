use std::path::Path;

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

fn is_system(path: &Path) -> bool {
    #[cfg(windows)]
    {
        system_folders().iter().any(|folder| is_inside(path, folder))
    }
    #[cfg(target_os = "macos")]
    {
        MACOS_FOLDERS.iter().any(|folder| is_inside(path, Path::new(folder)))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = path;
        false
    }
}

pub fn could_be_unindexable(name: &str) -> bool {
    #[cfg(windows)]
    {
        system_folders().iter().any(|folder| {
            folder
                .file_name()
                .is_some_and(|called| called.eq_ignore_ascii_case(name))
        })
    }
    #[cfg(target_os = "macos")]
    {
        MACOS_FOLDERS.iter().any(|folder| {
            Path::new(folder)
                .file_name()
                .is_some_and(|called| called.eq_ignore_ascii_case(name))
        })
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = name;
        false
    }
}

pub fn children_may_differ(folder: &Path) -> bool {
    if is_photos_library(folder) {
        return true;
    }
    let depth = folder.components().count();
    #[cfg(windows)]
    {
        system_folders()
            .iter()
            .any(|system| system.components().count() > depth)
    }
    #[cfg(target_os = "macos")]
    {
        MACOS_FOLDERS
            .iter()
            .any(|system| Path::new(system).components().count() > depth)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = depth;
        false
    }
}

#[cfg_attr(not(any(windows, target_os = "macos", test)), allow(dead_code))]
fn is_inside(path: &Path, folder: &Path) -> bool {
    let mut below = path.components();
    folder.components().all(|part| {
        below
            .next()
            .is_some_and(|c| c.as_os_str().eq_ignore_ascii_case(part.as_os_str()))
    })
}

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

#[cfg(windows)]
fn system_folders() -> &'static [std::path::PathBuf] {
    static FOLDERS: std::sync::OnceLock<Vec<std::path::PathBuf>> = std::sync::OnceLock::new();
    FOLDERS.get_or_init(|| {
        [
            ("SystemRoot", r"C:\Windows"),
            ("ProgramFiles", r"C:\Program Files"),
            ("ProgramFiles(x86)", r"C:\Program Files (x86)"),
        ]
        .into_iter()
        .map(|(variable, usual)| std::env::var_os(variable).map_or_else(|| usual.into(), Into::into))
        .collect()
    })
}

#[cfg(test)]
mod portable_tests {
    use std::path::Path;

    use super::is_inside;

    #[test]
    fn inside_matches_whole_components() {
        let system = Path::new("/System");
        assert!(is_inside(Path::new("/System"), system));
        assert!(is_inside(Path::new("/System/Library/CoreServices"), system));
        assert!(is_inside(Path::new("/SYSTEM/library"), system));
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
        let windows = system_folders()[0].clone();
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
