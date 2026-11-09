#[cfg_attr(not(windows), allow(dead_code))]
pub const EVENT: &str = "system-menu-key";

#[cfg(windows)]
pub use imp::{install, set_recording};

#[cfg(not(windows))]
pub fn install(_app: &tauri::AppHandle) {}

#[cfg(not(windows))]
pub fn set_recording(_on: bool) {}

#[cfg(windows)]
mod imp {
    use std::sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    };

    use tauri::{AppHandle, Emitter, Manager};
    use windows::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::Shell::{DefSubclassProc, SetWindowSubclass},
    };

    use super::EVENT;

    const WM_SYSCOMMAND: u32 = 0x0112;
    const SC_KEYMENU: usize = 0xF100;
    const SPACE: isize = 0x20;
    const SUBCLASS: usize = 0x4d65_7361;

    static RECORDING: AtomicBool = AtomicBool::new(false);
    static APP: OnceLock<AppHandle> = OnceLock::new();

    pub fn set_recording(on: bool) {
        RECORDING.store(on, Ordering::SeqCst);
    }

    pub fn install(app: &AppHandle) {
        let _ = APP.set(app.clone());
        for (label, window) in app.webview_windows() {
            let hwnd = match window.hwnd() {
                Ok(hwnd) => hwnd,
                Err(e) => {
                    eprintln!("Mesa: no handle for the {label} window, so Alt+Space can't be recorded in it: {e}");
                    continue;
                }
            };
            let installed = unsafe { SetWindowSubclass(HWND(hwnd.0), Some(watch), SUBCLASS, 0) };
            if !installed.as_bool() {
                eprintln!("Mesa: Alt+Space can't be recorded in the {label} window.");
            }
        }
    }

    unsafe extern "system" fn watch(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if message == WM_SYSCOMMAND
            && wparam.0 & 0xFFF0 == SC_KEYMENU
            && lparam.0 == SPACE
            && RECORDING.load(Ordering::SeqCst)
        {
            if let Some(app) = APP.get() {
                let _ = app.emit(EVENT, ());
            }
            return LRESULT(0);
        }
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }
}
