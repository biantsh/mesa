use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use tauri::{AppHandle, Emitter, Manager, WebviewWindow, Window};

pub const LABEL: &str = "main";
const HIDDEN_EVENT: &str = "main-hidden";
const NAVIGATE_EVENT: &str = "navigate";

#[cfg(target_os = "macos")]
const SHOWN_EVENT: &str = "main-shown";

#[cfg(target_os = "macos")]
const REVEAL_GIVE_UP: Duration = Duration::from_millis(150);
const OPENING_GIVE_UP: Duration = Duration::from_millis(600);
const PAGE_GIVE_UP: Duration = Duration::from_secs(6);
static KEEP_HIDDEN: AtomicBool = AtomicBool::new(false);
static SHOWN_ONCE: AtomicBool = AtomicBool::new(false);
static RESET_ON_SHOW: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
static REVEALING: AtomicBool = AtomicBool::new(false);
static OPENING: AtomicBool = AtomicBool::new(false);

pub fn keep_hidden_at_start(hidden: bool) {
    KEEP_HIDDEN.store(hidden, Ordering::SeqCst);
}

fn dock_icon(app: &AppHandle, shown: bool) {
    #[cfg(target_os = "macos")]
    {
        use tauri::ActivationPolicy::{Accessory, Regular};
        let _ = app.set_activation_policy(if shown { Regular } else { Accessory });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, shown);
}

const FULL_SCREEN_EVENT: &str = "full-screen";

pub fn watch_full_screen(app: &tauri::App) {
    #[cfg(target_os = "macos")]
    if let Some(window) = app.get_webview_window("main") {
        let on_window = window.clone();
        let _ = window.run_on_main_thread(move || {
            use objc2_app_kit::{
                NSWindow, NSWindowDidEnterFullScreenNotification, NSWindowDidExitFullScreenNotification,
                NSWindowWillEnterFullScreenNotification, NSWindowWillExitFullScreenNotification,
            };
            use objc2_foundation::{NSNotification, NSNotificationCenter};
            use std::ptr::NonNull;

            let Ok(pointer) = on_window.ns_window() else { return };
            let ns_window = unsafe { &*pointer.cast::<NSWindow>() };
            let center = NSNotificationCenter::defaultCenter();
            let names = unsafe {
                [
                    (NSWindowWillEnterFullScreenNotification, true),
                    (NSWindowWillExitFullScreenNotification, false),
                    (NSWindowDidEnterFullScreenNotification, true),
                    (NSWindowDidExitFullScreenNotification, false),
                ]
            };
            for (name, full_screen) in names {
                let app = on_window.app_handle().clone();
                let told = block2::RcBlock::new(move |_: NonNull<NSNotification>| {
                    let _ = app.emit_to("main", FULL_SCREEN_EVENT, full_screen);
                });
                let observer = unsafe {
                    center.addObserverForName_object_queue_usingBlock(Some(name), Some(ns_window), None, &told)
                };
                std::mem::forget(observer);
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

pub fn set_up_dock(app: &mut tauri::App) {
    #[cfg(target_os = "macos")]
    if KEEP_HIDDEN.load(Ordering::SeqCst) {
        app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

fn bring_up(window: &WebviewWindow) {
    let was_closed = RESET_ON_SHOW.swap(false, Ordering::SeqCst);
    if was_closed {
        reset(window);
    }
    dock_icon(window.app_handle(), true);
    #[cfg(target_os = "macos")]
    if was_closed {
        hide_until_painted(window);
    }
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    #[cfg(target_os = "macos")]
    if was_closed {
        let _ = window.emit_to(window.label(), SHOWN_EVENT, ());
    }
}

#[cfg(target_os = "macos")]
fn hide_until_painted(window: &WebviewWindow) {
    set_alpha(window, 0.0);
    REVEALING.store(true, Ordering::SeqCst);
    let app = window.app_handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(REVEAL_GIVE_UP);
        reveal(&app);
    });
}

pub fn reveal(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    if REVEALING.swap(false, Ordering::SeqCst)
        && let Some(window) = app.get_webview_window(LABEL)
    {
        set_alpha(&window, 1.0);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

#[cfg(target_os = "macos")]
fn set_alpha(window: &WebviewWindow, alpha: f64) {
    let on_window = window.clone();
    let _ = window.run_on_main_thread(move || {
        use objc2_app_kit::NSWindow;
        let Ok(pointer) = on_window.ns_window() else { return };
        unsafe { &*pointer.cast::<NSWindow>() }.setAlphaValue(alpha);
    });
}

pub fn show_at_start(window: &WebviewWindow) {
    if !KEEP_HIDDEN.load(Ordering::SeqCst) && !SHOWN_ONCE.swap(true, Ordering::SeqCst) {
        bring_up(window);
    }
}

pub fn show(app: &AppHandle) {
    KEEP_HIDDEN.store(false, Ordering::SeqCst);
    SHOWN_ONCE.store(true, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(LABEL) {
        bring_up(&window);
    }
}

pub fn show_if_the_page_does_not(app: &tauri::App) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    std::thread::spawn(move || {
        std::thread::sleep(PAGE_GIVE_UP);
        show_at_start(&window);
    });
}

pub fn closed(window: &Window) {
    KEEP_HIDDEN.store(true, Ordering::SeqCst);
    let _ = window.hide();
    dock_icon(window.app_handle(), false);
    RESET_ON_SHOW.store(true, Ordering::SeqCst);
    let _ = window.emit_to(window.label(), HIDDEN_EVENT, ());
}

fn reset(window: &WebviewWindow) {
    let _ = window.unmaximize();
    let config = window.app_handle().config();
    if let Some(first) = config
        .app
        .windows
        .iter()
        .find(|configured| configured.label == window.label())
    {
        let _ = window.set_size(tauri::LogicalSize::new(first.width, first.height));
    }
    let _ = window.center();
}

pub fn open_from_widget(app: &AppHandle, screen: String) {
    OPENING.store(true, Ordering::SeqCst);
    let _ = app.emit_to(LABEL, NAVIGATE_EVENT, screen);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(OPENING_GIVE_UP);
        if OPENING.swap(false, Ordering::SeqCst) {
            show(&app);
        }
    });
}

pub fn page_is_ready(app: &AppHandle) {
    if OPENING.swap(false, Ordering::SeqCst) {
        show(app);
    }
}
