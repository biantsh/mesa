use std::path::PathBuf;

pub fn base_dir(variable: &str, under_home: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|folder| folder.is_absolute())
        .or_else(|| std::env::home_dir().map(|home| home.join(under_home)))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn app_dir() -> Option<PathBuf> {
    let folder = std::fs::canonicalize(std::env::var_os("APPDIR")?).ok()?;
    std::env::current_exe().ok()?.starts_with(&folder).then_some(folder)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn program() -> Option<PathBuf> {
    let Some(folder) = app_dir() else {
        return std::env::current_exe().ok();
    };
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| Some(folder.join("AppRun")))
}

#[cfg(target_os = "linux")]
pub fn set_up_gstreamer(identifier: &str) {
    let Some(folder) = app_dir() else { return };
    let plugins = folder.join("usr/lib/gstreamer-1.0");
    if !plugins.is_dir() {
        return;
    }
    let helpers = folder.join("usr/lib/gstreamer1.0/gstreamer-1.0");
    let mut set = vec![
        ("GST_PLUGIN_SYSTEM_PATH_1_0", plugins.clone()),
        ("GST_PLUGIN_PATH_1_0", plugins),
        ("GST_PLUGIN_SCANNER_1_0", helpers.join("gst-plugin-scanner")),
        ("GST_PTP_HELPER_1_0", helpers.join("gst-ptp-helper")),
    ];
    if std::env::var_os("GST_REGISTRY_1_0").is_none()
        && let Some(cache) = base_dir("XDG_CACHE_HOME", ".cache")
    {
        set.push(("GST_REGISTRY_1_0", cache.join(identifier).join("gstreamer-registry.bin")));
    }
    for (variable, value) in set {
        // SAFETY: main calls this before it starts any thread.
        unsafe { std::env::set_var(variable, value) };
    }
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
