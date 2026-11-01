use std::path::Path;

use tauri::{AppHandle, Manager, State};

use crate::{
    browse::{self, Listing},
    index::{Index, IndexState},
    progress::Progress,
};

#[tauri::command]
pub fn home_dir(app: AppHandle) -> String {
    app.path()
        .home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn list_dir(path: String, index: State<'_, Index>) -> Result<Listing, String> {
    let mut listing = tauri::async_runtime::spawn_blocking(move || browse::list(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    for entry in &mut listing.entries {
        let state = index.state(Path::new(&entry.path));
        entry.index = state.status;
        entry.droppable = state.droppable;
    }
    Ok(listing)
}

#[tauri::command(async)]
pub fn index_statuses(paths: Vec<String>, index: State<'_, Index>) -> Vec<IndexState> {
    paths.iter().map(|path| index.state(Path::new(path))).collect()
}

#[tauri::command(async)]
pub fn index_progress(index: State<'_, Index>) -> Option<Progress> {
    index.progress()
}

#[tauri::command(async)]
pub fn set_indexing_paused(paused: bool, index: State<'_, Index>) -> Result<(), String> {
    index.set_paused(paused)
}

#[tauri::command(async)]
pub fn cancel_indexing(index: State<'_, Index>) -> Result<(), String> {
    index.cancel()
}

#[tauri::command(async)]
pub fn add_to_index(path: String, index: State<'_, Index>) -> Result<(), String> {
    index.set(Path::new(&path), true)
}

#[tauri::command(async)]
pub fn drop_from_queue(path: String, index: State<'_, Index>) -> Result<(), String> {
    index.drop_job(Path::new(&path))
}

#[tauri::command(async)]
pub fn remove_from_index(path: String, index: State<'_, Index>) -> Result<(), String> {
    index.set(Path::new(&path), false)
}

#[tauri::command(async)]
pub fn clear_index(index: State<'_, Index>) -> Result<(), String> {
    index.clear()
}

#[tauri::command(async)]
pub fn index_everything(index: State<'_, Index>) -> Result<(), String> {
    index.set_everything(&browse::roots())
}
