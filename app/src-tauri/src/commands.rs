use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::{
    browse::{self, Categories, Category},
    embed::ModelStatus,
    estimate::{Estimate, Estimator},
    folder_access::{FolderAccess, Pane},
    hotkey::{self, Hotkey},
    index::{Index, IndexStatus, Page, Stats},
    main_window,
    progress::Progress,
    settings::{Settings, SettingsStore},
    shown_folder::{self, ShownFolder},
    shown_listing::{self, ShownListing},
    startup,
    store::{Filter, Hit},
    survey::{self, Survey},
    thumbnail::{FolderPreviews, PreviewServer},
};

#[tauri::command]
pub fn home_dir(app: AppHandle) -> String {
    app.path()
        .home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn list_dir(path: String, app: AppHandle) -> Result<shown_listing::Said, String> {
    app.state::<FolderAccess>().reading(Path::new(&path));
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<ShownListing>()
            .read(&path, &app.state::<Index>(), &app.state::<FolderAccess>())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn refresh_folder(path: String, generation: u64, app: AppHandle) -> Result<shown_listing::Refreshed, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<ShownListing>()
            .refresh(&path, generation, &app.state::<Index>(), &app.state::<FolderAccess>())
    })
    .await
    .map_err(|e| e.to_string())?
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub empty: bool,
    pub progress: Option<Progress>,
    pub everything_indexed: bool,
    #[serde(flatten)]
    pub stats: Stats,
}

fn overview(app: &AppHandle, index: &Index) -> Overview {
    let progress = index.progress();
    Overview {
        empty: progress.is_none() && index.is_empty(),
        progress,
        everything_indexed: index.has_everything(&everything_roots(app)),
        stats: index.stats(),
    }
}

#[derive(Serialize)]
pub struct IndexView {
    overview: Overview,
    folder: Option<shown_listing::Statuses>,
    lists: Option<ListStates>,
}

#[tauri::command(async)]
pub fn index_view(
    listing: Option<u64>,
    lists: Option<u64>,
    list_states: bool,
    app: AppHandle,
    index: State<'_, Index>,
    shown: State<'_, ShownListing>,
) -> IndexView {
    let folder = listing.map(|generation| shown.statuses(generation, &index));
    let lists = lists.map(|version| list_states_of(version, list_states, &index));
    IndexView {
        overview: overview(&app, &index),
        folder,
        lists,
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
    #[serde(flatten)]
    pub why: crate::store::Why,
    pub copies: Vec<String>,
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
            why: hit.why,
            copies: hit
                .copies
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        }
    }
}

#[tauri::command(async)]
pub fn match_details(
    path: String,
    query: String,
    at: Option<u64>,
    index: State<'_, Index>,
) -> Result<crate::matches::Details, String> {
    let path = Path::new(&path);
    if !browse::is_listed_file(path) {
        return Err("That isn't a file Mesa lists.".into());
    }
    index.match_details(path, &query, at)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub id: u64,
    pub hits: Vec<SearchHit>,
    pub more: bool,
    pub close: bool,
    pub more_close: bool,
}

impl From<Page> for SearchPage {
    fn from(page: Page) -> Self {
        Self {
            id: page.id,
            hits: page.hits.into_iter().map(SearchHit::from).collect(),
            more: page.more,
            close: page.close,
            more_close: page.more_close,
        }
    }
}

#[derive(Deserialize)]
pub struct SearchFilter {
    category: Option<Category>,
    from: Option<i64>,
    to: Option<i64>,
    language: Option<String>,
    project: Option<String>,
    tests: Option<bool>,
}

#[tauri::command(async)]
pub fn search(
    query: String,
    filter: SearchFilter,
    files: Option<Vec<String>>,
    window: tauri::WebviewWindow,
    index: State<'_, Index>,
) -> Result<SearchPage, String> {
    let project = match filter.project {
        Some(name) => Some(index.code_project_folder(&name)?.unwrap_or(-1)),
        None => None,
    };
    let language = filter.language.as_deref().and_then(crate::embed::code::language_named);
    let code = crate::store::CodeFilter {
        language,
        project,
        tests: filter.tests.unwrap_or(false),
    };
    let filter = Filter {
        category: filter.category,
        from_ms: filter.from,
        to_ms: filter.to,
        code,
    };
    Ok(index
        .search_for(window.label(), &query, &to_paths(files.unwrap_or_default()), filter)?
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
        None => index.add_many(&paths),
    }
}

#[tauri::command(async)]
pub fn remove_from_index(paths: Vec<String>, index: State<'_, Index>) -> Result<(), String> {
    index.remove_many(&to_paths(paths))
}

#[tauri::command(async)]
pub fn open_file(path: String, app: AppHandle) -> Result<(), String> {
    let file = Path::new(&path);
    if !browse::is_listed_file(file) {
        return Err("That isn't a file Mesa opens.".into());
    }
    if browse::category_of(&file.file_name().unwrap_or_default().to_string_lossy()) == Some(browse::Category::Code) {
        if crate::editor::open_at(file, 1)? {
            return Ok(());
        }
        return crate::editor::open_as_text(file);
    }
    let kind = file.extension().map_or_else(
        || "this kind of".to_owned(),
        |extension| format!(".{}", extension.to_string_lossy().to_lowercase()),
    );
    #[cfg(target_os = "macos")]
    if !has_app_for(file) {
        return Err(format!("No app on this Mac opens {kind} files."));
    }
    app.opener().open_path(path.as_str(), None::<&str>).map_err(|e| {
        let why = e.to_string();
        if why.starts_with("Launcher") {
            format!("Nothing on this computer opened it: no app may be set to open {kind} files.")
        } else {
            why
        }
    })
}

#[tauri::command(async)]
pub fn code_peek(path: String, line: Option<u64>) -> Result<Vec<Vec<crate::highlight::Part>>, String> {
    let file = Path::new(&path);
    if !browse::is_listed_file(file) {
        return Err("That isn't a file Mesa lists.".into());
    }
    crate::highlight::peek(file, line)
}

#[tauri::command(async)]
pub fn code_projects(index: State<'_, Index>) -> Result<Vec<String>, String> {
    Ok(index.code_projects()?.into_iter().map(|(name, _)| name).collect())
}

#[tauri::command(async)]
pub fn also_in(path: String, line: u64, index: State<'_, Index>) -> Result<Vec<crate::index::AlsoIn>, String> {
    if !browse::is_listed_file(Path::new(&path)) {
        return Err("That isn't a file Mesa lists.".into());
    }
    index.also_in(Path::new(&path), line)
}

#[tauri::command(async)]
pub fn code_file(path: String, query: String) -> Result<crate::highlight::CodeFile, String> {
    let file = Path::new(&path);
    if !browse::is_listed_file(file) {
        return Err("That isn't a file Mesa lists.".into());
    }
    crate::highlight::code_file(file, &query)
}

#[tauri::command(async)]
pub fn open_code(path: String, line: u32, app: AppHandle) -> Result<(), String> {
    if !browse::is_listed_file(Path::new(&path)) {
        return Err("That isn't a file Mesa opens.".into());
    }
    let _ = app;
    if crate::editor::open_at(Path::new(&path), line.max(1))? {
        return Ok(());
    }
    crate::editor::open_as_text(Path::new(&path))
}

#[cfg(target_os = "macos")]
fn has_app_for(path: &Path) -> bool {
    let url = objc2_foundation::NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(&path.to_string_lossy()));
    objc2_app_kit::NSWorkspace::sharedWorkspace()
        .URLForApplicationToOpenURL(&url)
        .is_some()
}

#[tauri::command(async)]
pub fn paging(path: String) -> Option<crate::document_preview::Paging> {
    if !browse::is_listed_file(Path::new(&path)) {
        return None;
    }
    crate::document_preview::paging(&path)
}

#[tauri::command]
pub fn allow_playing(path: String, app: AppHandle) -> Result<(), String> {
    let file = Path::new(&path);
    if !browse::is_listed_file(file)
        || browse::category_of(&file.file_name().unwrap_or_default().to_string_lossy()) != Some(Category::Video)
    {
        return Err("That isn't a video Mesa plays.".into());
    }
    app.asset_protocol_scope().allow_file(file).map_err(|e| e.to_string())
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
    #[cfg(target_os = "linux")]
    {
        crate::clipboard::copy_files(&window, &files)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
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
pub fn notices() -> tauri::ipc::Response {
    tauri::ipc::Response::new(crate::notices::NOTICES.to_owned())
}

#[tauri::command(async)]
pub fn open_notice_link(url: String, app: AppHandle) -> Result<(), String> {
    if !crate::notices::links_to(&url) {
        return Err(format!("{url} isn't a link of the licenses"));
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
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
    pub slot: usize,
}

async fn listed(items: Vec<PathBuf>, index: &Index) -> Result<Vec<Listed>, String> {
    let described = tauri::async_runtime::spawn_blocking(move || {
        let mut described: Vec<Listed> = browse::map_in_parallel(&items, |slot, path| {
            let (entry, missing) = browse::describe(path);
            Listed { entry, missing, slot }
        });
        described.sort_by_cached_key(|item| (browse::listing_order(&item.entry), item.entry.path.clone()));
        described
    })
    .await
    .map_err(|e| e.to_string())?;
    let asked: Vec<(&Path, Option<bool>)> = described
        .iter()
        .map(|item| (Path::new(&item.entry.path), Some(item.entry.kind == browse::Kind::File)))
        .collect();
    let states = index.states_of(&asked);
    Ok(described
        .into_iter()
        .zip(states)
        .map(|(mut item, state)| {
            item.entry.index = state.status;
            item.entry.queued = state.queued;
            item.entry.excluded_here = state.excluded_here;
            item.entry.removable = state.removable;
            item
        })
        .collect())
}

#[derive(Serialize)]
pub struct Lists {
    version: u64,
    indexed: ListedColumns,
    excluded: ListedColumns,
}

#[derive(Serialize)]
pub struct ListedColumns {
    #[serde(flatten)]
    entries: shown_listing::Columns,
    missing: String,
    slots: Vec<usize>,
}

impl ListedColumns {
    fn of(items: &[Listed]) -> Self {
        Self {
            entries: shown_listing::columns_of(&items.iter().map(|item| &item.entry).collect::<Vec<_>>()),
            missing: items.iter().map(|item| if item.missing { '1' } else { '0' }).collect(),
            slots: items.iter().map(|item| item.slot).collect(),
        }
    }
}

#[tauri::command]
pub async fn index_lists(index: State<'_, Index>) -> Result<Lists, String> {
    let version = index.lists_version();
    let indexed = listed(index.indexed_roots_of_now().to_vec(), &index).await?;
    let excluded = listed(index.excluded_paths(), &index).await?;
    Ok(Lists {
        version,
        indexed: ListedColumns::of(&indexed),
        excluded: ListedColumns::of(&excluded),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListStates {
    version: u64,
    stale: bool,
    states: Option<Vec<(IndexStatus, bool, bool)>>,
}

fn list_states_of(version: u64, states: bool, index: &Index) -> ListStates {
    let now = index.lists_version();
    let stale = now != version;
    let read = (!stale && states).then(|| {
        index
            .indexed_states()
            .into_iter()
            .map(|s| (s.status, s.queued, s.removable))
            .collect()
    });
    let stale = stale || index.lists_version() != now;
    ListStates {
        version: now,
        stale,
        states: read.filter(|_| !stale),
    }
}

#[tauri::command(async)]
pub fn stop_indexing(index: State<'_, Index>) -> Result<(), String> {
    index.stop_indexing()
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
    let counted = estimator.counted(index.indexed_types(), &index.excluded_paths());
    access.indexing(roots.iter().map(PathBuf::as_path));
    index.set_everything_counted(&roots, |root| {
        let folders = counted.as_ref()?;
        Some(
            folders
                .iter()
                .filter(|(folder, _)| folder.starts_with(root))
                .cloned()
                .collect(),
        )
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
    let excluded = index.excluded_paths();
    let survey = match estimator.recent_survey(&not_asked, &excluded) {
        Some(survey) => survey,
        None => {
            let survey = survey::survey(&everything_roots(&app), &folders, |path| index.is_excluded(path));
            estimator.keep(survey.clone(), not_asked, excluded);
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

#[tauri::command(async)]
pub fn get_settings(app: AppHandle, settings: State<'_, SettingsStore>) -> Settings {
    startup::current(&app, &settings)
}

#[tauri::command]
pub fn get_indexed_types(index: State<'_, Index>) -> Vec<Category> {
    index.indexed_types().to_vec()
}

#[tauri::command(async)]
pub fn set_indexed_types(types: Vec<Category>, index: State<'_, Index>) -> Result<Vec<Category>, String> {
    let chosen = Categories::of(&types);
    let before = index.indexed_types();
    if chosen == before {
        return Ok(chosen.to_vec());
    }
    let empty = index.is_empty();
    index.set_indexed_types(chosen)?;
    if !empty && !before.includes(chosen) {
        index.check_for_added_types();
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingGpu {
    name: String,
    shared: bool,
}

#[tauri::command(async)]
pub fn embedding_gpu(index: State<'_, Index>) -> Option<EmbeddingGpu> {
    if let Some(name) = index.gpu() {
        return Some(EmbeddingGpu { name, shared: false });
    }
    index.shared_gpu().map(|name| EmbeddingGpu { name, shared: true })
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

#[tauri::command]
pub fn set_always_full_speed(
    enabled: bool,
    settings: State<'_, SettingsStore>,
    index: State<'_, Index>,
) -> Result<(), String> {
    settings
        .set(Settings {
            always_full_speed: enabled,
            ..settings.get()
        })
        .map_err(|e| format!("Couldn't save the setting. {e}"))?;
    index.set_always_full_speed(enabled);
    Ok(())
}

#[tauri::command(async)]
pub fn set_start_at_sign_in(enabled: bool, app: AppHandle, settings: State<'_, SettingsStore>) -> Result<(), String> {
    startup::set(&app, &settings, enabled)
}

#[tauri::command]
pub fn update_status(updates: State<'_, crate::update::Updates>) -> crate::update::Status {
    updates.status()
}

#[tauri::command]
pub fn check_for_updates(app: AppHandle) {
    crate::update::check_soon(&app);
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    crate::update::install(&app).await
}

#[tauri::command]
pub fn set_check_for_updates(enabled: bool, settings: State<'_, SettingsStore>) -> Result<(), String> {
    settings
        .set(Settings {
            check_for_updates: enabled,
            ..settings.get()
        })
        .map_err(|e| format!("Couldn't save the setting. {e}"))
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
pub fn report_page_error(window: tauri::WebviewWindow, message: String) {
    eprintln!("Mesa: the page of the {} window failed: {message}", window.label());
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
pub fn example_progress(paths: Vec<String>, index: State<'_, Index>) -> Vec<Option<crate::examples::Progress>> {
    index.example_progress(&paths.into_iter().map(PathBuf::from).collect::<Vec<_>>())
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
pub fn set_widget_boxes(window: tauri::WebviewWindow, boxes: Vec<[f64; 4]>) {
    if window.label() == crate::widget::LABEL {
        crate::widget::set_visible_boxes(&window, boxes);
    }
}

#[tauri::command]
pub fn widget_fills_screen() -> bool {
    crate::widget::fills_screen()
}

#[tauri::command]
pub fn show_main(app: AppHandle) {
    crate::main_window::page_is_ready(&app);
}

#[tauri::command(async)]
pub fn show_folder_previews(
    generation: u64,
    folder: Option<FolderPreviews>,
    previews: State<'_, PreviewServer>,
    shown: State<'_, ShownListing>,
) {
    let folder = folder.and_then(
        |FolderPreviews {
             shape,
             order,
             width,
             height,
         }| Some((shown.previews(shape, order.as_deref())?, width, height)),
    );
    previews.show_folder(generation, folder);
}

#[tauri::command(async)]
pub fn show_preview_window(generation: u64, start: usize, end: usize, warm: bool, previews: State<'_, PreviewServer>) {
    previews.show_window(generation, start, end, warm);
}
