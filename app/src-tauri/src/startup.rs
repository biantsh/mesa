use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

pub const BACKGROUND_ARG: &str = "--background";
pub const SEARCH_ARG: &str = "--search";

pub fn arguments() -> Vec<String> {
    std::env::args().collect()
}

pub fn starts_without_window() -> bool {
    let args = arguments();
    in_background(&args) || wants_search(&args)
}

pub fn wants_search(args: &[String]) -> bool {
    args.iter().any(|arg| arg == SEARCH_ARG)
}

pub fn in_background(args: &[String]) -> bool {
    args.iter().any(|arg| arg == BACKGROUND_ARG)
}

pub fn register(app: &AppHandle) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    app.autolaunch()
        .enable()
        .map_err(|e| format!("Couldn't make Mesa start when you sign in. {e}"))
}
