use std::path::Path;

pub fn is_unindexable(path: &Path) -> bool {
    #[cfg(windows)]
    {
        let mut below = path.components();
        windows_folder().components().all(|part| {
            below
                .next()
                .is_some_and(|c| c.as_os_str().eq_ignore_ascii_case(part.as_os_str()))
        })
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

pub fn could_be_unindexable(name: &str) -> bool {
    #[cfg(windows)]
    {
        windows_folder()
            .file_name()
            .is_some_and(|called| called.eq_ignore_ascii_case(name))
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        false
    }
}

#[cfg(windows)]
fn windows_folder() -> &'static std::path::PathBuf {
    static ROOT: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| std::env::var_os("SystemRoot").map_or_else(|| r"C:\Windows".into(), Into::into))
}

#[cfg(all(test, windows))]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn windows_folder_unindexable() {
        let windows = windows_folder().clone();
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
