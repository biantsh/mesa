use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

use crate::index::lock;

pub const LABEL: &str = "widget";
const SHOWN_EVENT: &str = "widget-shown";
const HIDDEN_EVENT: &str = "widget-hidden";

const CANDIDATES: &[(Modifiers, Code, &str)] = &[
    (Modifiers::ALT, Code::Space, "Alt+Space"),
    (Modifiers::CONTROL.union(Modifiers::ALT), Code::Space, "Ctrl+Alt+Space"),
    (
        Modifiers::CONTROL.union(Modifiers::SHIFT),
        Code::Space,
        "Ctrl+Shift+Space",
    ),
    (Modifiers::ALT.union(Modifiers::SHIFT), Code::Space, "Alt+Shift+Space"),
    (Modifiers::CONTROL.union(Modifiers::ALT), Code::KeyM, "Ctrl+Alt+M"),
];

const BLUR_GRACE: Duration = Duration::from_millis(400);
const TOP_AT: f64 = 0.14;
static SHOWN_AT: Mutex<Option<Instant>> = Mutex::new(None);

pub fn register_hotkey(app: &AppHandle) -> Option<&'static str> {
    let keys = app.global_shortcut();
    for (modifiers, code, name) in CANDIDATES {
        match keys.register(Shortcut::new(Some(*modifiers), *code)) {
            Ok(()) => return Some(name),
            Err(e) => eprintln!("Mesa: {name} is taken, so it can't be the search shortcut: {e}"),
        }
    }
    None
}

pub fn toggle(app: &AppHandle) {
    if lock(&SHOWN_AT).is_some() {
        hide(app);
    } else {
        show(app);
    }
}

pub fn show(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    place(app, &window);
    *lock(&SHOWN_AT) = Some(Instant::now());
    let _ = window.show();
    let _ = window.set_focus();
    let _ = app.emit_to(LABEL, SHOWN_EVENT, ());
}

pub fn hide(app: &AppHandle) {
    *lock(&SHOWN_AT) = None;
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
    let _ = app.emit_to(LABEL, HIDDEN_EVENT, ());
}

pub fn lost_focus(app: &AppHandle) {
    let settled = lock(&SHOWN_AT).is_some_and(|at| at.elapsed() >= BLUR_GRACE);
    if settled {
        hide(app);
    }
}

pub fn show_soon(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1200));
        show(&app);
    });
}

fn place(app: &AppHandle, window: &WebviewWindow) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|pointer| app.monitor_from_point(pointer.x, pointer.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let (Some(monitor), Ok(size)) = (monitor, window.outer_size()) else {
        return;
    };
    let (x, y) = position(
        (monitor.position().x, monitor.position().y),
        (monitor.size().width, monitor.size().height),
        (size.width, size.height),
    );
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

fn position(screen_at: (i32, i32), screen: (u32, u32), widget: (u32, u32)) -> (i32, i32) {
    let across = (i64::from(screen.0) - i64::from(widget.0)) / 2;
    let down = (f64::from(screen.1) * TOP_AT) as i64;
    let lowest = (i64::from(screen.1) - i64::from(widget.1)).max(0);
    (
        screen_at.0 + across.max(0) as i32,
        screen_at.1 + down.min(lowest) as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_short_screen() {
        assert_eq!(position((0, 0), (1280, 600), (900, 560)), (190, 40));
        assert_eq!(position((0, 0), (800, 480), (900, 560)), (0, 0));
    }
}
