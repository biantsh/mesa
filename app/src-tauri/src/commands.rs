use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::{
    browse::{self, Listing},
    index::{Index, IndexState},
    progress::Progress,
    settings::{Settings, SettingsStore},
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
        entry.excluded_here = state.excluded_here;
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
pub fn add_to_index(paths: Vec<String>, completing: Option<String>, index: State<'_, Index>) -> Result<(), String> {
    match completing {
        Some(folder) => index.add_completing(&to_paths(paths), Path::new(&folder)),
        None => index.set_many(&to_paths(paths), true),
    }
}

#[tauri::command(async)]
pub fn drop_from_queue(paths: Vec<String>, index: State<'_, Index>) -> Result<(), String> {
    index.drop_jobs(&to_paths(paths))
}

#[tauri::command(async)]
pub fn remove_from_index(paths: Vec<String>, index: State<'_, Index>) -> Result<(), String> {
    index.set_many(&to_paths(paths), false)
}

#[tauri::command(async)]
pub fn open_file(path: String, app: AppHandle) -> Result<(), String> {
    if !browse::is_listed_file(Path::new(&path)) {
        return Err("That isn't a file Mesa opens.".into());
    }
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn show_in_folder(path: String, app: AppHandle) -> Result<(), String> {
    if !Path::new(&path).exists() {
        return Err("That isn't there any more.".into());
    }
    app.opener().reveal_item_in_dir(path).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn exclude_from_index(paths: Vec<String>, index: State<'_, Index>) -> Result<(), String> {
    index.exclude_many(&to_paths(paths))
}

#[tauri::command(async)]
pub fn stop_excluding(paths: Vec<String>, index: State<'_, Index>) -> Result<(), String> {
    index.stop_excluding_many(&to_paths(paths))
}

#[tauri::command(async)]
pub fn count_with_data(paths: Vec<String>, index: State<'_, Index>) -> usize {
    index.count_with_data(&to_paths(paths))
}

#[tauri::command(async)]
pub fn exclusion_count(index: State<'_, Index>) -> usize {
    index.exclusion_count()
}

fn to_paths(paths: Vec<String>) -> Vec<PathBuf> {
    paths.into_iter().map(PathBuf::from).collect()
}

#[tauri::command(async)]
pub fn clear_index(index: State<'_, Index>) -> Result<(), String> {
    index.clear()
}

#[tauri::command(async)]
pub fn index_everything(index: State<'_, Index>) -> Result<(), String> {
    index.set_everything(&browse::roots())
}

#[tauri::command]
pub fn get_settings(settings: State<'_, SettingsStore>) -> Settings {
    settings.get()
}

#[tauri::command(async)]
pub fn embedding_gpu(index: State<'_, Index>) -> Option<String> {
    index.gpu()
}

#[tauri::command(async)]
pub fn set_gpu_acceleration(
    enabled: bool,
    settings: State<'_, SettingsStore>,
    index: State<'_, Index>,
) -> Result<(), String> {
    let before = settings.get();
    let after = Settings {
        gpu_acceleration: enabled,
        ..before
    };
    index.set_device(after.device())?;
    if let Err(e) = settings.set(after) {
        let _ = index.set_device(before.device());
        return Err(format!("Couldn't save the setting. {e}"));
    }
    Ok(())
}
