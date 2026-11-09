use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::{
    browse::{self, Categories, Category, Listing},
    embed::ModelStatus,
    estimate::{Estimate, Estimator},
    folder_access::{FolderAccess, Pane},
    hotkey::{self, Hotkey},
    index::{Index, IndexState, Page, Stats},
    main_window,
    progress::Progress,
    settings::{Settings, SettingsStore},
    shown_folder::{self, ShownFolder},
    store::{Filter, Hit},
    survey::{self, Survey},
};

#[tauri::command]
pub fn home_dir(app: AppHandle) -> String {
    app.path()
        .home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn list_dir(
    path: String,
    index: State<'_, Index>,
    access: State<'_, FolderAccess>,
) -> Result<Listing, String> {
    access.reading(Path::new(&path));
    let mut listing = tauri::async_runtime::spawn_blocking(move || browse::list(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| browse::explain(&e))?;
    for entry in &mut listing.entries {
        let state = index.state(Path::new(&entry.path));
        entry.index = state.status;
        entry.droppable = state.droppable;
        entry.excluded_here = state.excluded_here;
        entry.removable = state.removable;
        if !entry.readable {
            entry.privacy = access.pane_for(Path::new(&entry.path));
        }
    }
    Ok(listing)
}

#[tauri::command]
pub fn watch_shown_folder(path: Option<String>, app: AppHandle, shown: State<'_, ShownFolder>) -> Result<(), String> {
    shown.set(path.filter(|path| !path.is_empty()).map(PathBuf::from), move |folder| {
        let _ = app.emit_to(
            main_window::LABEL,
            shown_folder::CHANGED_EVENT,
            folder.to_string_lossy(),
        );
    })
}

#[tauri::command(async)]
pub fn index_statuses(paths: Vec<String>, index: State<'_, Index>) -> Vec<IndexState> {
    paths.iter().map(|path| index.state(Path::new(path))).collect()
}

#[tauri::command(async)]
pub fn index_progress(index: State<'_, Index>) -> Option<Progress> {
    index.progress()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub empty: bool,
    pub progress: Option<Progress>,
    pub everything_indexed: bool,
    #[serde(flatten)]
    pub stats: Stats,
}

#[tauri::command(async)]
pub fn index_overview(app: AppHandle, index: State<'_, Index>) -> Overview {
    let progress = index.progress();
    Overview {
        empty: progress.is_none() && index.is_empty(),
        progress,
        everything_indexed: index.has_everything(&everything_roots(&app)),
        stats: index.stats(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub path: String,
    pub name: String,
    pub folder: String,
    pub category: Category,
    pub modified: i64,
    pub size: u64,
    pub score: f32,
}

impl From<Hit> for SearchHit {
    fn from(hit: Hit) -> Self {
        Self {
            name: hit
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: hit.path.to_string_lossy().into_owned(),
            folder: hit.folder,
            category: hit.category,
            modified: hit.modified_ms,
            size: hit.size,
            score: hit.score,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub id: u64,
    pub hits: Vec<SearchHit>,
    pub more: bool,
}

impl From<Page> for SearchPage {
    fn from(page: Page) -> Self {
        Self {
            id: page.id,
            hits: page.hits.into_iter().map(SearchHit::from).collect(),
            more: page.more,
        }
    }
}

#[tauri::command(async)]
pub fn search(
    query: String,
    category: Option<Category>,
    from: Option<i64>,
    to: Option<i64>,
    files: Option<Vec<String>>,
    index: State<'_, Index>,
) -> Result<SearchPage, String> {
    let filter = Filter {
        category,
        from_ms: from,
        to_ms: to,
    };
    Ok(index
        .search_like(&query, &to_paths(files.unwrap_or_default()), filter)?
        .into())
}

#[tauri::command(async)]
pub fn search_more(id: u64, index: State<'_, Index>) -> Result<SearchPage, String> {
    Ok(index.search_more(id)?.into())
}

#[tauri::command(async)]
pub fn set_indexing_paused(paused: bool, index: State<'_, Index>) -> Result<(), String> {
    index.set_paused(paused)
}

#[tauri::command]
pub fn set_indexing_turbo(enabled: bool, index: State<'_, Index>) {
    index.set_turbo(enabled);
}

#[tauri::command(async)]
pub fn add_to_index(
    paths: Vec<String>,
    completing: Option<String>,
    index: State<'_, Index>,
    access: State<'_, FolderAccess>,
) -> Result<(), String> {
    let paths = to_paths(paths);
    access.indexing(paths.iter().map(PathBuf::as_path));
    match completing {
        Some(folder) => index.add_completing(&paths, Path::new(&folder)),
        None => index.set_many(&paths, true),
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
pub fn copy_files(paths: Vec<String>, window: tauri::WebviewWindow) -> Result<(), String> {
    let files: Vec<PathBuf> = to_paths(paths)
        .into_iter()
        .filter(|path| browse::is_listed_file(path))
        .collect();
    if files.is_empty() {
        return Err("Those files aren't there any more.".into());
    }
    #[cfg(windows)]
    {
        let owner = window.hwnd().map_err(|e| e.to_string())?;
        crate::clipboard::copy_files(owner, &files)
    }
    #[cfg(target_os = "macos")]
    {
        let _ = window;
        crate::clipboard::copy_files(&files)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = window;
        Err("Copying files isn't supported on this platform yet.".into())
    }
}

const PROJECT_URL: &str = "https://github.com/biantsh/mesa";

#[tauri::command(async)]
pub fn open_project_page(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(PROJECT_URL, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_privacy_settings(pane: Pane, app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(pane.settings_url(), None::<&str>)
        .map_err(|e| e.to_string())
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    #[serde(flatten)]
    pub entry: browse::Entry,
    pub missing: bool,
}

async fn listed(items: Vec<PathBuf>, index: &Index) -> Result<Vec<Listed>, String> {
    let described = tauri::async_runtime::spawn_blocking(move || {
        let mut described: Vec<Listed> = items
            .into_iter()
            .map(|path| {
                let (entry, missing) = browse::describe(&path);
                Listed { entry, missing }
            })
            .collect();
        described.sort_by_cached_key(|item| (browse::listing_order(&item.entry), item.entry.path.clone()));
        described
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(described
        .into_iter()
        .map(|mut item| {
            let state = index.state(Path::new(&item.entry.path));
            item.entry.index = state.status;
            item.entry.droppable = state.droppable;
            item.entry.excluded_here = state.excluded_here;
            item.entry.removable = state.removable;
            item
        })
        .collect())
}

#[tauri::command]
pub async fn indexed_items(index: State<'_, Index>) -> Result<Vec<Listed>, String> {
    listed(index.indexed_roots(), &index).await
}

#[tauri::command]
pub async fn excluded_items(index: State<'_, Index>) -> Result<Vec<Listed>, String> {
    listed(index.excluded_paths(), &index).await
}

fn to_paths(paths: Vec<String>) -> Vec<PathBuf> {
    paths.into_iter().map(PathBuf::from).collect()
}

fn everything_roots(app: &AppHandle) -> Vec<PathBuf> {
    static FOUND: Mutex<Option<(Instant, Vec<PathBuf>)>> = Mutex::new(None);
    const FOR: Duration = Duration::from_secs(30);
    let mut found = FOUND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match &*found {
        Some((at, roots)) if at.elapsed() < FOR => roots.clone(),
        _ => {
            let roots = browse::user_roots(app.path().home_dir().ok());
            *found = Some((Instant::now(), roots.clone()));
            roots
        }
    }
}

#[tauri::command(async)]
pub fn index_everything(
    app: AppHandle,
    index: State<'_, Index>,
    access: State<'_, FolderAccess>,
    estimator: State<'_, Arc<Estimator>>,
) -> Result<(), String> {
    if index.indexed_types().is_empty() {
        return Err("No file types chosen to index.".into());
    }
    let roots = everything_roots(&app);
    if roots.is_empty() {
        return Err("Couldn't find your folders.".into());
    }
    let counted = estimator.counted(index.indexed_types());
    access.indexing(roots.iter().map(PathBuf::as_path));
    index.set_everything_counted(&roots, |root| {
        counted
            .as_ref()?
            .iter()
            .find(|(counted_root, _)| counted_root == root)
            .map(|&(_, files)| files)
    })
}

#[tauri::command(async)]
pub fn survey_user_files(
    include_protected: bool,
    app: AppHandle,
    index: State<'_, Index>,
    access: State<'_, FolderAccess>,
    settings: State<'_, SettingsStore>,
    estimator: State<'_, Arc<Estimator>>,
) -> Survey {
    if include_protected {
        access.asking_all();
    }
    let _turn = estimator.one_survey_at_a_time();
    let folders = access.folders();
    let not_asked: Vec<String> = folders
        .iter()
        .filter(|folder| !folder.asked)
        .map(|folder| folder.name.clone())
        .collect();
    let survey = match estimator.recent_survey(&not_asked) {
        Some(survey) => survey,
        None => {
            let survey = survey::survey(&everything_roots(&app), &folders, |path| index.is_excluded(path));
            estimator.keep(survey.clone(), not_asked);
            survey
        }
    };
    estimator.measure_missing(settings.get().device());
    survey
}

#[tauri::command]
pub fn index_estimate(settings: State<'_, SettingsStore>, estimator: State<'_, Arc<Estimator>>) -> Estimate {
    estimator.estimate(settings.get().device())
}

#[tauri::command]
pub fn get_settings(settings: State<'_, SettingsStore>) -> Settings {
    settings.get()
}

#[tauri::command]
pub fn get_indexed_types(index: State<'_, Index>) -> Vec<Category> {
    index.indexed_types().to_vec()
}

#[tauri::command(async)]
pub fn set_indexed_types(
    types: Vec<Category>,
    settings: State<'_, SettingsStore>,
    index: State<'_, Index>,
) -> Result<Vec<Category>, String> {
    let chosen = Categories::of(&types);
    let before = index.indexed_types();
    if chosen == before {
        return Ok(chosen.to_vec());
    }
    let empty = index.is_empty();
    settings
        .set(Settings {
            indexed_types: chosen.to_vec(),
            ..settings.get()
        })
        .map_err(|e| format!("Couldn't save the setting. {e}"))?;
    index.set_indexed_types(chosen)?;
    if !empty && !before.includes(chosen) {
        index.check_for_changes();
    }
    Ok(chosen.to_vec())
}

#[tauri::command(async)]
pub fn count_indexed_of_type(kind: Category, index: State<'_, Index>) -> Result<u64, String> {
    index.count_of_type(kind)
}

#[tauri::command]
pub fn model_status(index: State<'_, Index>) -> ModelStatus {
    index.model_status()
}

#[tauri::command]
pub fn retry_model_setup(index: State<'_, Index>) {
    index.retry_model_setup();
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
    let after = Settings {
        gpu_acceleration: enabled,
        ..settings.get()
    };
    apply_setting(&settings, after, |s| index.set_device(s.device()))
}

fn apply_setting(
    settings: &SettingsStore,
    after: Settings,
    apply: impl Fn(Settings) -> Result<(), String>,
) -> Result<(), String> {
    let before = settings.get();
    apply(after.clone())?;
    if let Err(e) = settings.set(after) {
        let _ = apply(before);
        return Err(format!("Couldn't save the setting. {e}"));
    }
    Ok(())
}

#[tauri::command]
pub fn get_search_hotkey(hotkey: State<'_, Hotkey>) -> hotkey::Status {
    hotkey.status()
}

#[tauri::command(async)]
pub fn set_search_hotkey(
    app: AppHandle,
    shortcut: Option<String>,
    settings: State<'_, SettingsStore>,
    hotkey: State<'_, Hotkey>,
) -> Result<hotkey::Status, hotkey::Error> {
    let old = hotkey.change(&app, shortcut.as_deref())?;
    let chosen = shortcut
        .as_deref()
        .and_then(|text| hotkey::parse(text).ok())
        .map(|key| hotkey::canonical(&key));
    let after = Settings {
        search_hotkey: chosen,
        ..settings.get()
    };
    if let Err(e) = settings.set(after) {
        hotkey.restore(&app, old);
        return Err(hotkey::Error::Failed(format!("Couldn't save the setting. {e}")));
    }
    Ok(hotkey.status())
}

#[tauri::command(async)]
pub fn begin_hotkey_recording(app: AppHandle, hotkey: State<'_, Hotkey>) {
    hotkey.pause(&app);
}

#[tauri::command(async)]
pub fn end_hotkey_recording(app: AppHandle, hotkey: State<'_, Hotkey>) {
    hotkey.resume(&app);
}

#[tauri::command]
pub fn page_painted(window: tauri::WebviewWindow) {
    if window.label() == "main" {
        crate::main_window::show_at_start(&window);
    }
}

#[tauri::command]
pub fn reveal_main(app: AppHandle) {
    crate::main_window::reveal(&app);
}

#[tauri::command]
pub fn hide_widget(app: AppHandle) {
    crate::widget::hide(&app);
}

#[tauri::command(async)]
pub fn prepare_example(path: String, index: State<'_, Index>) -> Result<(), String> {
    index.prepare_example(Path::new(&path))
}

#[tauri::command]
pub fn folders_among(paths: Vec<String>) -> Vec<bool> {
    paths.iter().map(|path| Path::new(path).is_dir()).collect()
}

#[tauri::command]
pub fn set_picking(app: AppHandle, window: tauri::WebviewWindow, picking: bool) {
    if window.label() == crate::widget::LABEL {
        crate::widget::set_picking(&app, picking);
    }
}

#[tauri::command]
pub fn open_main(app: AppHandle, screen: String) {
    crate::widget::hide(&app);
    crate::main_window::open_from_widget(&app, screen);
}

#[tauri::command]
pub fn set_widget_box(window: tauri::WebviewWindow, left: f64, top: f64, right: f64, bottom: f64) {
    if window.label() == crate::widget::LABEL {
        crate::widget::set_visible_box([left, top, right, bottom]);
    }
}

#[tauri::command]
pub fn show_main(app: AppHandle) {
    crate::main_window::page_is_ready(&app);
}
