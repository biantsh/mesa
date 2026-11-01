#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod browse;
mod commands;
mod index;
mod indexer;
mod progress;
mod store;
mod thumbnail;
mod vectors;

use tauri::Manager;

fn main() {
    let previews = thumbnail::PreviewServer::start();

    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let index =
                index::Index::open(&dir).map_err(|e| format!("Couldn't open the index in {}: {e}", dir.display()))?;
            app.manage(index);
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
            commands::set_indexing_paused,
            commands::cancel_indexing,
            commands::add_to_index,
            commands::drop_from_queue,
            commands::remove_from_index,
            commands::index_everything,
            commands::clear_index,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
