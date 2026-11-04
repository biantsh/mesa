use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

pub const BACKGROUND_ARG: &str = "--background";
pub const SEARCH_ARG: &str = "--search";

pub fn starts_without_window() -> bool {
    in_background(std::env::args()) || wants_search(std::env::args())
}

pub fn wants_search(mut args: impl Iterator<Item = String>) -> bool {
    args.any(|arg| arg == SEARCH_ARG)
}

pub fn in_background(mut args: impl Iterator<Item = String>) -> bool {
    args.any(|arg| arg == BACKGROUND_ARG)
}

pub fn register(app: &AppHandle) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    app.autolaunch()
        .enable()
        .map_err(|e| format!("Couldn't make Mesa start when you sign in. {e}"))
}
