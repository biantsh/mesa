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
mod thumbnail;
mod unindexable;
mod vectors;
mod watch;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{
    Manager, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

pub static KEEP_HIDDEN: AtomicBool = AtomicBool::new(false);

fn show_window(app: &tauri::AppHandle) {
    KEEP_HIDDEN.store(false, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn add_tray_icon(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Mesa", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Mesa", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .tooltip("Mesa")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_window(app),
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
        if !KEEP_HIDDEN.load(Ordering::SeqCst) {
            let _ = window.show();
        }
    });
}

fn main() {
    KEEP_HIDDEN.store(startup::started_in_background(), Ordering::SeqCst);
    let previews = thumbnail::PreviewServer::start();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !startup::in_background(args.into_iter()) {
                show_window(app);
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![startup::BACKGROUND_ARG]),
        ))
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                KEEP_HIDDEN.store(true, Ordering::SeqCst);
                let _ = window.hide();
            }
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
            let index = index::Index::open_with(&dir, embedder, settings.get().tuning())
                .map_err(|e| format!("Couldn't open the index in {}: {e}", dir.display()))?;
            index.check_for_changes();
            index.start_watching();
            app.manage(index);
            if let Err(e) = startup::register(app.handle()) {
                eprintln!("Mesa: {e}");
            }
            app.manage(settings);
            add_tray_icon(app)?;
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
            commands::cancel_indexing,
            commands::add_to_index,
            commands::drop_from_queue,
            commands::remove_from_index,
            commands::exclude_from_index,
            commands::stop_excluding,
            commands::count_with_data,
            commands::exclusion_count,
            commands::open_file,
            commands::copy_files,
            commands::open_project_page,
            commands::show_in_folder,
            commands::index_everything,
            commands::clear_index,
            commands::get_settings,
            commands::embedding_gpu,
            commands::model_status,
            commands::retry_model_setup,
            commands::set_gpu_acceleration,
            commands::set_turbo_indexing,
            commands::page_painted,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
