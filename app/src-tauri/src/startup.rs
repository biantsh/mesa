use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

pub const BACKGROUND_ARG: &str = "--background";

pub fn started_in_background() -> bool {
    in_background(std::env::args())
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
