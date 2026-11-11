use std::path::PathBuf;

pub fn base_dir(variable: &str, under_home: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|folder| folder.is_absolute())
        .or_else(|| std::env::home_dir().map(|home| home.join(under_home)))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn program() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::current_exe().ok())
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}
