#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod browse;
mod clipboard;
mod commands;
mod crawl;
mod embed;
mod index;
mod indexer;
mod power;
mod progress;
mod rules;
mod settings;
mod startup;
mod store;
mod surface;
mod thumbnail;
#[cfg(windows)]
mod toast;
mod unindexable;
mod vectors;
mod watch;
mod widget;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{
    Emitter, Manager, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

const HIDDEN_EVENT: &str = "main-hidden";
pub static KEEP_HIDDEN: AtomicBool = AtomicBool::new(false);

pub fn bring_up(window: &tauri::WebviewWindow) {
    if RESET_ON_SHOW.swap(false, Ordering::SeqCst) {
        reset_window(window);
    }
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

static SHOWN_ONCE: AtomicBool = AtomicBool::new(false);

pub fn show_at_start(window: &tauri::WebviewWindow) {
    if !KEEP_HIDDEN.load(Ordering::SeqCst) && !SHOWN_ONCE.swap(true, Ordering::SeqCst) {
        bring_up(window);
    }
}

fn show_window(app: &tauri::AppHandle) {
    KEEP_HIDDEN.store(false, Ordering::SeqCst);
    SHOWN_ONCE.store(true, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window("main") {
        bring_up(&window);
    }
}

static RESET_ON_SHOW: AtomicBool = AtomicBool::new(false);

fn reset_window(window: &tauri::WebviewWindow) {
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

fn add_tray_icon(app: &tauri::App, search_key: Option<&str>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Mesa", true, None::<&str>)?;
    let search = MenuItem::with_id(
        app,
        "search",
        search_key.map_or_else(|| "Search".to_owned(), |key| format!("Search ({key})")),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit Mesa", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &search, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .tooltip("Mesa")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_window(app),
            "search" => widget::show(app),
            "quit" => {
                app.state::<index::Index>().shut_down();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn show_window_if_the_page_does_not(app: &tauri::App) {
    const GIVE_UP: std::time::Duration = std::time::Duration::from_secs(6);
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    std::thread::spawn(move || {
        std::thread::sleep(GIVE_UP);
        show_at_start(&window);
    });
}

fn main() {
    KEEP_HIDDEN.store(startup::starts_without_window(), Ordering::SeqCst);
    let previews = thumbnail::PreviewServer::start();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if startup::wants_search(&args) {
                widget::show(app);
            } else if !startup::in_background(&args) {
                show_window(app);
            }
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        widget::toggle(app);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![startup::BACKGROUND_ARG]),
        ))
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| match (window.label(), event) {
            (widget::LABEL, WindowEvent::CloseRequested { api, .. }) => {
                api.prevent_close();
                widget::hide(window.app_handle());
            }
            (widget::LABEL, WindowEvent::Focused(false)) => widget::lost_focus(window.app_handle()),
            (_, WindowEvent::CloseRequested { api, .. }) => {
                api.prevent_close();
                KEEP_HIDDEN.store(true, Ordering::SeqCst);
                let _ = window.hide();
                RESET_ON_SHOW.store(true, Ordering::SeqCst);
                let _ = window.emit_to(window.label(), HIDDEN_EVENT, ());
            }
            _ => {}
        })
        .setup(|app| {
            let dir = match std::env::var_os("MESA_DATA_DIR").filter(|dir| !dir.is_empty()) {
                Some(dir) => std::path::PathBuf::from(dir),
                None => app.path().app_data_dir()?,
            };
            let resources = app.path().resource_dir().ok();
            let settings = settings::SettingsStore::open(&dir);
            let embedder = embed::for_this_platform(&dir, resources.as_deref(), settings.get().device());
            let _ = embedder.set_eco(!settings.get().turbo_indexing);
            embedder.warm_up();
            let index = index::Index::open_with(&dir, embedder, settings.get().turbo_indexing)
                .map_err(|e| format!("Couldn't open the index in {}: {e}", dir.display()))?;
            index.check_for_changes();
            index.start_watching();
            app.manage(index);
            surface::start(app.handle().clone(), dir.clone());
            if let Err(e) = startup::register(app.handle()) {
                eprintln!("Mesa: {e}");
            }
            app.manage(settings);
            let search_key = widget::register_hotkey(app.handle());
            add_tray_icon(app, search_key)?;
            if startup::wants_search(&startup::arguments()) {
                widget::show_soon(app.handle());
            }
            show_window_if_the_page_does_not(app);
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("thumb", move |_ctx, request, responder| {
            previews.handle(request, responder)
        })
        .invoke_handler(tauri::generate_handler![
            commands::home_dir,
            commands::list_dir,
            commands::index_statuses,
            commands::index_progress,
            commands::index_overview,
            commands::search,
            commands::search_more,
            commands::set_indexing_paused,
            commands::add_to_index,
            commands::drop_from_queue,
            commands::remove_from_index,
            commands::exclude_from_index,
            commands::stop_excluding,
            commands::count_with_data,
            commands::exclusion_count,
            commands::indexed_items,
            commands::excluded_items,
            commands::open_file,
            commands::copy_files,
            commands::open_project_page,
            commands::show_in_folder,
            commands::index_everything,
            commands::get_settings,
            commands::embedding_gpu,
            commands::model_status,
            commands::retry_model_setup,
            commands::set_gpu_acceleration,
            commands::set_turbo_indexing,
            commands::page_painted,
            commands::hide_widget,
            commands::open_main,
            commands::show_main,
            commands::set_widget_box,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
