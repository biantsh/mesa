#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod browse;
mod commands;
mod index;
mod thumbnail;

fn main() {
    let previews = thumbnail::PreviewServer::start();

    tauri::Builder::default()
        .manage(index::IndexRules::default())
        .register_asynchronous_uri_scheme_protocol("thumb", move |_ctx, request, responder| {
            previews.handle(request, responder)
        })
        .invoke_handler(tauri::generate_handler![
            commands::home_dir,
            commands::list_dir,
            commands::add_to_index,
            commands::remove_from_index,
            commands::index_everything,
            commands::clear_index,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
