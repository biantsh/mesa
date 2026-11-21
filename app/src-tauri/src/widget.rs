use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

#[cfg(not(target_os = "macos"))]
use tauri::PhysicalPosition;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use crate::index::lock;

pub const LABEL: &str = "widget";
const SHOWN_EVENT: &str = "widget-shown";
const HIDDEN_EVENT: &str = "widget-hidden";
const FOCUS_SEARCH_EVENT: &str = "focus-search";
static HAS_HAD_FOCUS: AtomicBool = AtomicBool::new(false);
const TOP_AT: f64 = 0.14;
static SHOWN: AtomicBool = AtomicBool::new(false);
static PICKING: AtomicBool = AtomicBool::new(false);
static VISIBLE: Mutex<Option<Vec<[f64; 4]>>> = Mutex::new(None);

#[cfg(target_os = "macos")]
static FRAME: Mutex<Option<[f64; 4]>> = Mutex::new(None);
static WATCH: AtomicU64 = AtomicU64::new(0);
const WATCH_EVERY: Duration = Duration::from_millis(16);

pub fn set_visible_boxes(window: &WebviewWindow, boxes: Vec<[f64; 4]>) {
    *lock(&VISIBLE) = Some(boxes.clone());
    #[cfg(target_os = "linux")]
    if SHOWN.load(Ordering::SeqCst) {
        input_region(window, Some(boxes));
    }
    #[cfg(not(target_os = "linux"))]
    let _ = boxes;
    #[cfg(not(target_os = "linux"))]
    let _ = window;
}

#[cfg(target_os = "linux")]
fn input_region(window: &WebviewWindow, boxes: Option<Vec<[f64; 4]>>) {
    let scale = crate::page_scale::factor();
    let boxes = boxes.map(|boxes| boxes.into_iter().map(|box_| box_.map(|side| side * scale)).collect::<Vec<_>>());
    let on_window = window.clone();
    let _ = window.run_on_main_thread(move || {
        use gtk::{
            cairo::{RectangleInt, Region},
            prelude::*,
        };
        let Ok(gtk_window) = on_window.gtk_window() else { return };
        let Some(surface) = gtk_window.window() else { return };
        let rectangles: Vec<RectangleInt> = match boxes {
            Some(boxes) => boxes
                .into_iter()
                .map(|[left, top, right, bottom]| {
                    let (left, top) = (left.floor() as i32, top.floor() as i32);
                    RectangleInt::new(
                        left,
                        top,
                        (right.ceil() as i32 - left).max(0),
                        (bottom.ceil() as i32 - top).max(0),
                    )
                })
                .collect(),
            None => vec![RectangleInt::new(0, 0, i32::MAX / 2, i32::MAX / 2)],
        };
        surface.input_shape_combine_region(&Region::create_rectangles(&rectangles), 0, 0);
    });
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn page_place(pointer: (f64, f64), origin: (i32, i32), scale: f64) -> (f64, f64) {
    (
        (pointer.0 - f64::from(origin.0)) / scale,
        (pointer.1 - f64::from(origin.1)) / scale,
    )
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn page_place_on_mac(mouse: (f64, f64), frame: [f64; 4]) -> (f64, f64) {
    let [left, bottom, _, height] = frame;
    (mouse.0 - left, bottom + height - mouse.1)
}

fn in_box(at: (f64, f64), rect: [f64; 4]) -> bool {
    at.0 >= rect[0] && at.0 <= rect[2] && at.1 >= rect[1] && at.1 <= rect[3]
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
fn in_any_box(at: (f64, f64), boxes: &[[f64; 4]]) -> bool {
    boxes.iter().any(|&rect| in_box(at, rect))
}

#[cfg(target_os = "macos")]
fn pointer_in_page(_app: &AppHandle, _window: &WebviewWindow) -> Option<(f64, f64)> {
    let frame = (*lock(&FRAME))?;
    let mouse = objc2_app_kit::NSEvent::mouseLocation();
    Some(page_place_on_mac((mouse.x, mouse.y), frame))
}

#[cfg(not(target_os = "macos"))]
fn pointer_in_page(app: &AppHandle, window: &WebviewWindow) -> Option<(f64, f64)> {
    let (Ok(pointer), Ok(origin), Ok(scale)) = (app.cursor_position(), window.inner_position(), window.scale_factor())
    else {
        return None;
    };
    Some(page_place((pointer.x, pointer.y), (origin.x, origin.y), scale))
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
fn watch_pointer(app: &AppHandle, watch: u64) {
    let mut letting_through = false;
    while WATCH.load(Ordering::SeqCst) == watch {
        std::thread::sleep(WATCH_EVERY);
        let Some(window) = app.get_webview_window(LABEL) else {
            return;
        };
        let Some(at) = pointer_in_page(app, &window) else {
            continue;
        };
        let over_it = match &*lock(&VISIBLE) {
            Some(boxes) => in_any_box(at, boxes),
            None => true,
        };
        if over_it == letting_through && WATCH.load(Ordering::SeqCst) == watch {
            letting_through = !over_it;
            let _ = window.set_ignore_cursor_events(letting_through);
        }
    }
}

pub fn float_over_everything(app: &AppHandle) {
    if fills_screen()
        && let Some(window) = app.get_webview_window(LABEL)
    {
        fill_the_screen(&window);
    }
    #[cfg(target_os = "macos")]
    if let Some(window) = app.get_webview_window(LABEL) {
        let on_window = window.clone();
        let _ = window.run_on_main_thread(move || {
            use objc2_app_kit::{NSStatusWindowLevel, NSWindow, NSWindowCollectionBehavior};
            let Ok(pointer) = on_window.ns_window() else { return };
            let ns_window = unsafe { &*pointer.cast::<NSWindow>() };
            ns_window.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary
                    | NSWindowCollectionBehavior::IgnoresCycle,
            );
            ns_window.setLevel(NSStatusWindowLevel);
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

pub fn toggle(app: &AppHandle) {
    if SHOWN.load(Ordering::SeqCst) {
        hide(app);
    } else if main_window_is_in_front(app) {
        let _ = app.emit_to("main", FOCUS_SEARCH_EVENT, ());
    } else {
        show(app);
    }
}

fn main_window_is_in_front(app: &AppHandle) -> bool {
    app.get_webview_window("main").is_some_and(|window| {
        window.is_visible().unwrap_or(false)
            && !window.is_minimized().unwrap_or(false)
            && window.is_focused().unwrap_or(false)
    })
}

pub fn fills_screen() -> bool {
    #[cfg(target_os = "linux")]
    {
        crate::wayland::is_wayland()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

fn fill_the_screen(window: &WebviewWindow) {
    let _ = window.set_resizable(true);
    let _ = window.maximize();
}

pub fn show(app: &AppHandle) {
    if let Some(index) = app.try_state::<crate::index::Index>() {
        index.look_soon();
    }
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    #[cfg(target_os = "macos")]
    {
        *lock(&FRAME) = None;
    }
    if fills_screen() {
        fill_the_screen(&window);
    } else {
        place(app, &window);
    }
    SHOWN.store(true, Ordering::SeqCst);
    HAS_HAD_FOCUS.store(false, Ordering::SeqCst);
    *lock(&VISIBLE) = None;
    let _ = window.set_ignore_cursor_events(false);
    #[cfg(target_os = "linux")]
    input_region(&window, None);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = app.emit_to(LABEL, SHOWN_EVENT, ());
    let watch = WATCH.fetch_add(1, Ordering::SeqCst) + 1;
    #[cfg(not(target_os = "linux"))]
    {
        let app = app.clone();
        std::thread::spawn(move || watch_pointer(&app, watch));
    }
    #[cfg(target_os = "linux")]
    let _ = watch;
}

pub fn hide(app: &AppHandle) {
    SHOWN.store(false, Ordering::SeqCst);
    PICKING.store(false, Ordering::SeqCst);
    WATCH.fetch_add(1, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.set_ignore_cursor_events(false);
        #[cfg(target_os = "linux")]
        input_region(&window, None);
        let _ = window.hide();
    }
    let _ = app.emit_to(LABEL, HIDDEN_EVENT, ());
}

pub fn set_picking(app: &AppHandle, picking: bool) {
    PICKING.store(picking, Ordering::SeqCst);
    if !picking
        && SHOWN.load(Ordering::SeqCst)
        && let Some(window) = app.get_webview_window(LABEL)
    {
        let _ = window.set_focus();
    }
}

pub fn got_focus() {
    HAS_HAD_FOCUS.store(true, Ordering::SeqCst);
}

pub fn lost_focus(app: &AppHandle) {
    if SHOWN.load(Ordering::SeqCst) && HAS_HAD_FOCUS.load(Ordering::SeqCst) && !PICKING.load(Ordering::SeqCst) {
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

#[cfg(target_os = "macos")]
fn place(_app: &AppHandle, window: &WebviewWindow) {
    let on_window = window.clone();
    let _ = window.run_on_main_thread(move || {
        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSEvent, NSScreen, NSWindow};
        use objc2_foundation::NSPoint;

        let Some(mtm) = MainThreadMarker::new() else { return };
        let Ok(pointer) = on_window.ns_window() else { return };
        let ns_window = unsafe { &*pointer.cast::<NSWindow>() };
        let mouse = NSEvent::mouseLocation();
        let screens = NSScreen::screens(mtm);
        let Some(screen) = screens
            .iter()
            .find(|screen| {
                let f = screen.frame();
                mouse.x >= f.origin.x
                    && mouse.x < f.origin.x + f.size.width
                    && mouse.y >= f.origin.y
                    && mouse.y < f.origin.y + f.size.height
            })
            .or_else(|| screens.firstObject())
        else {
            return;
        };
        let frame = screen.frame();
        let size = ns_window.frame().size;
        let (across, down) = position(
            (0, 0),
            (frame.size.width as u32, frame.size.height as u32),
            (size.width as u32, size.height as u32),
        );
        ns_window.setFrameTopLeftPoint(NSPoint::new(
            frame.origin.x + f64::from(across),
            frame.origin.y + frame.size.height - f64::from(down),
        ));
        let placed = ns_window.frame();
        *lock(&FRAME) = Some([placed.origin.x, placed.origin.y, placed.size.width, placed.size.height]);
    });
}

#[cfg(not(target_os = "macos"))]
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

    #[test]
    fn takes_the_pointer_over_a_popup_below_the_bar() {
        let boxes = [[100.0, 20.0, 900.0, 140.0], [120.0, 130.0, 320.0, 390.0]];
        assert!(in_any_box((500.0, 80.0), &boxes));
        assert!(in_any_box((200.0, 320.0), &boxes));
        assert!(!in_any_box((600.0, 320.0), &boxes));
        assert!(!in_any_box((50.0, 80.0), &boxes));
    }
}
