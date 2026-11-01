use std::path::Path;

use tauri::{AppHandle, Manager, State};

use crate::{
    browse::{self, Listing},
    index::IndexRules,
};

#[tauri::command]
pub fn home_dir(app: AppHandle) -> String {
    app.path()
        .home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn list_dir(path: String, rules: State<'_, IndexRules>) -> Result<Listing, String> {
    let mut listing = tauri::async_runtime::spawn_blocking(move || browse::list(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    for entry in &mut listing.entries {
        entry.index = rules.status(Path::new(&entry.path));
    }
    Ok(listing)
}

#[tauri::command]
pub fn add_to_index(path: String, rules: State<'_, IndexRules>) {
    rules.set(Path::new(&path), true);
}

#[tauri::command]
pub fn remove_from_index(path: String, rules: State<'_, IndexRules>) {
    rules.set(Path::new(&path), false);
}

#[tauri::command]
pub fn clear_index(rules: State<'_, IndexRules>) {
    rules.clear();
}

#[tauri::command]
pub async fn index_everything(rules: State<'_, IndexRules>) -> Result<(), String> {
    let roots = tauri::async_runtime::spawn_blocking(browse::roots)
        .await
        .map_err(|e| e.to_string())?;
    rules.set_everything(&roots);
    Ok(())
}
