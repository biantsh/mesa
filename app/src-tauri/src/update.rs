use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, Wry,
    menu::{Menu, MenuItem, PredefinedMenuItem},
};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};

use crate::{index::lock, settings::SettingsStore};

pub const STATUS_EVENT: &str = "update-status";
pub const MENU_ID: &str = "update";
const DIALOG: &str = "update";
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(20);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const RETRY_AFTER: Duration = Duration::from_secs(60 * 60);
const LOOK_EVERY: Duration = Duration::from_secs(10 * 60);
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const PROGRESS_EVERY: Duration = Duration::from_millis(250);
const OPEN_AFTER_FILE: &str = "updated-open-window";

#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum State {
    #[default]
    Idle,
    Checking,
    Available,
    Downloading,
    Installing,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub current: String,
    pub available: bool,
    pub state: State,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub done: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
    pub asks_password: bool,
}

pub struct Updates {
    data_dir: PathBuf,
    status: Mutex<Status>,
    looking: AtomicBool,
    found: Mutex<Option<Update>>,
    menu: Mutex<Option<(Menu<Wry>, Option<MenuItem<Wry>>)>>,
}

pub fn available() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        std::env::current_exe().is_ok_and(|exe| !crate::startup::not_installed(&exe) && app_bundle(&exe).is_some())
    }
    #[cfg(not(target_os = "macos"))]
    {
        tauri::utils::platform::bundle_type().is_some()
    }
}

#[cfg(target_os = "macos")]
fn app_bundle(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|part| part.extension().is_some_and(|ext| ext == "app"))
        .map(Path::to_path_buf)
}

fn asks_password(app: &AppHandle) -> bool {
    #[cfg(target_os = "linux")]
    {
        use tauri::utils::config::BundleType;
        match tauri::utils::platform::bundle_type() {
            Some(BundleType::Deb | BundleType::Rpm) => true,
            _ => app
                .env()
                .appimage
                .as_deref()
                .map(Path::new)
                .and_then(Path::parent)
                .is_some_and(|folder| !writable(folder)),
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        let bundle = std::env::current_exe().ok().and_then(|exe| app_bundle(&exe));
        bundle.is_none_or(|bundle| !writable(&bundle) || bundle.parent().is_none_or(|folder| !writable(folder)))
    }
    #[cfg(windows)]
    {
        let _ = app;
        false
    }
}

#[cfg(unix)]
fn writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
}

fn updater(app: &AppHandle) -> Result<Updater, String> {
    let builder = app.updater_builder().timeout(CHECK_TIMEOUT);
    #[cfg(windows)]
    let builder = {
        let app = app.clone();
        builder.on_before_exit(move || {
            if let Some(index) = app.try_state::<crate::index::Index>() {
                index.shut_down();
            }
            app.cleanup_before_exit();
        })
    };
    builder.build().map_err(|e| e.to_string())
}

pub fn just_updated(data_dir: &Path) -> bool {
    fs::remove_file(data_dir.join(OPEN_AFTER_FILE)).is_ok()
}

impl Updates {
    pub fn open(app: &AppHandle, data_dir: &Path) -> Self {
        let available = available();
        let status = Status {
            current: app.package_info().version.to_string(),
            available,
            asks_password: available && asks_password(app),
            ..Status::default()
        };
        Self {
            data_dir: data_dir.to_owned(),
            status: Mutex::new(status),
            looking: AtomicBool::new(false),
            found: Mutex::new(None),
            menu: Mutex::new(None),
        }
    }

    pub fn status(&self) -> Status {
        lock(&self.status).clone()
    }

    fn change(&self, app: &AppHandle, change: impl FnOnce(&mut Status)) {
        let status = {
            let mut status = lock(&self.status);
            change(&mut status);
            status.clone()
        };
        let _ = app.emit(STATUS_EVENT, status);
    }

    pub fn attach_menu(&self, menu: Menu<Wry>) {
        *lock(&self.menu) = Some((menu, None));
    }

    fn show_in_menu(&self, version: &str) {
        let mut attached = lock(&self.menu);
        let Some((menu, item)) = attached.as_mut() else { return };
        let text = format!("Update to Mesa {version}\u{2026}");
        if let Some(item) = item {
            if let Err(e) = item.set_text(&text) {
                eprintln!("Mesa: can't change the update's item in the tray's menu: {e}");
            }
            return;
        }
        let added = (|| -> tauri::Result<MenuItem<Wry>> {
            let update = MenuItem::with_id(menu.app_handle(), MENU_ID, &text, true, None::<&str>)?;
            let line = PredefinedMenuItem::separator(menu.app_handle())?;
            menu.insert(&update, 0)?;
            menu.insert(&line, 1)?;
            Ok(update)
        })();
        match added {
            Ok(update) => *item = Some(update),
            Err(e) => eprintln!("Mesa: can't add the update to the tray's menu: {e}"),
        }
    }
}

pub fn start(app: &AppHandle) {
    if !available() {
        return;
    }
    let app = app.clone();
    let spawned = thread::Builder::new().name("updates".into()).spawn(move || {
        thread::sleep(FIRST_CHECK_AFTER);
        let mut last: Option<(SystemTime, Duration)> = None;
        loop {
            let wanted = app.state::<SettingsStore>().get().check_for_updates;
            let due =
                last.is_none_or(|(at, wait)| SystemTime::now().duration_since(at).map_or(true, |since| since >= wait));
            if wanted && due {
                let looked = check(&app);
                last = Some((SystemTime::now(), if looked { CHECK_EVERY } else { RETRY_AFTER }));
            }
            thread::sleep(LOOK_EVERY);
        }
    });
    if let Err(e) = spawned {
        eprintln!("Mesa: can't look for updates: {e}");
    }
}

pub fn check_soon(app: &AppHandle) {
    if !available() {
        return;
    }
    let app = app.clone();
    let _ = thread::Builder::new().name("update-check".into()).spawn(move || {
        check(&app);
    });
}

fn check(app: &AppHandle) -> bool {
    let updates = app.state::<Updates>();
    if updates.looking.swap(true, Ordering::SeqCst) {
        return true;
    }
    let said = {
        let mut status = lock(&updates.status);
        match status.state {
            State::Idle => {
                status.state = State::Checking;
                status.error = None;
                true
            }
            State::Available => false,
            _ => {
                updates.looking.store(false, Ordering::SeqCst);
                return true;
            }
        }
    };
    if said {
        updates.change(app, |_| {});
    }
    let found =
        updater(app).and_then(|updater| tauri::async_runtime::block_on(updater.check()).map_err(|e| e.to_string()));
    updates.looking.store(false, Ordering::SeqCst);
    let offered = |state: State| matches!(state, State::Checking | State::Available);
    match found {
        Ok(Some(update)) => {
            let version = update.version.clone();
            let notes = update.body.clone().filter(|notes| !notes.trim().is_empty());
            let mut taken = false;
            updates.change(app, |status| {
                taken = !offered(status.state);
                if !taken {
                    status.state = State::Available;
                    status.version = Some(version.clone());
                    status.notes = notes;
                }
            });
            if !taken {
                *lock(&updates.found) = Some(update);
                updates.show_in_menu(&version);
            }
            true
        }
        Ok(None) => {
            updates.change(app, |status| {
                if status.state == State::Checking {
                    status.state = State::Idle;
                }
            });
            true
        }
        Err(e) => {
            eprintln!("Mesa: couldn't check for updates: {e}");
            updates.change(app, |status| {
                if status.state == State::Checking {
                    status.state = State::Idle;
                    status.error = Some("Couldn't check for updates.".into());
                }
            });
            false
        }
    }
}

pub fn open_dialog(app: &AppHandle) {
    crate::main_window::open_from_widget(app, DIALOG.into());
}

pub async fn install(app: &AppHandle) -> Result<(), String> {
    let updates = app.state::<Updates>();
    let mut update = {
        let mut status = lock(&updates.status);
        let found = lock(&updates.found).clone();
        let Some(update) = found.filter(|_| status.state == State::Available) else {
            return Err("There is no update to install.".into());
        };
        status.state = State::Downloading;
        status.done = 0;
        status.total = None;
        status.error = None;
        update
    };
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    let version = update.version.clone();
    updates.change(app, |_| {});
    let failed = |said: String| {
        updates.change(app, |status| {
            status.state = State::Available;
            status.error = Some(said.clone());
        });
        Err(said)
    };

    let mut done = 0u64;
    let mut said: Option<Instant> = None;
    let downloaded = update
        .download(
            |chunk, total| {
                done += chunk as u64;
                if said.is_none_or(|at| at.elapsed() >= PROGRESS_EVERY) {
                    said = Some(Instant::now());
                    updates.change(app, |status| {
                        status.done = done;
                        status.total = total;
                    });
                }
            },
            || {},
        )
        .await;
    let package = match downloaded {
        Ok(package) => package,
        Err(e) => {
            eprintln!("Mesa: the update to {version} couldn't be downloaded: {e}");
            let signature = matches!(
                e,
                tauri_plugin_updater::Error::Minisign(_)
                    | tauri_plugin_updater::Error::SignedVersionMismatch { .. }
                    | tauri_plugin_updater::Error::MissingSignedVersion
            );
            return failed(if signature {
                format!("The download of Mesa {version} didn't pass its signature check, so it wasn't installed.")
            } else {
                format!("Couldn't download Mesa {version}. Check your connection and try again.")
            });
        }
    };

    updates.change(app, |status| status.state = State::Installing);
    let note = updates.data_dir.join(OPEN_AFTER_FILE);
    let _ = fs::write(&note, b"");
    if let Err(e) = update.install(&package) {
        let _ = fs::remove_file(&note);
        eprintln!("Mesa: the update to {version} couldn't be installed: {e}");
        return failed(format!("Couldn't install Mesa {version}."));
    }
    app.restart();
}
