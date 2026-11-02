#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod browse;
mod commands;
mod crawl;
mod embed;
mod index;
mod indexer;
mod progress;
mod rules;
mod settings;
mod store;
mod thumbnail;
mod unindexable;
mod vectors;

use tauri::Manager;

fn show_window_if_the_page_does_not(app: &tauri::App) {
    const GIVE_UP: std::time::Duration = std::time::Duration::from_secs(6);
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    std::thread::spawn(move || {
        std::thread::sleep(GIVE_UP);
        let _ = window.show();
    });
}

fn main() {
    let previews = thumbnail::PreviewServer::start();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = match std::env::var_os("MESA_DATA_DIR").filter(|dir| !dir.is_empty()) {
                Some(dir) => std::path::PathBuf::from(dir),
                None => app.path().app_data_dir()?,
            };
            let resources = app.path().resource_dir().ok();
            let settings = settings::SettingsStore::open(&dir);
            let embedder = embed::for_this_platform(&dir, resources.as_deref(), settings.get().device());
            embedder.warm_up();
            let index = index::Index::open(&dir, embedder)
                .map_err(|e| format!("Couldn't open the index in {}: {e}", dir.display()))?;
            app.manage(index);
            app.manage(settings);
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
            commands::open_project_page,
            commands::show_in_folder,
            commands::index_everything,
            commands::clear_index,
            commands::get_settings,
            commands::embedding_gpu,
            commands::model_status,
            commands::retry_model_setup,
            commands::set_gpu_acceleration,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
