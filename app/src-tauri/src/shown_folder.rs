use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{RecvTimeoutError, unbounded};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher, event::ModifyKind};

pub const CHANGED_EVENT: &str = "folder-changed";
const QUIET: Duration = Duration::from_millis(250);
const EVERY: Duration = Duration::from_secs(1);

fn due(first: Instant, last: Option<Instant>) -> Instant {
    let soon = first + QUIET;
    match last {
        Some(last) => soon.max(last + EVERY),
        None => soon,
    }
}

pub struct Watching {
    _watcher: RecommendedWatcher,
}

fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned())
}

fn is_news(folder: &Path, event: &notify::Result<notify::Event>) -> bool {
    let Ok(event) = event else { return true };
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    if event.paths.is_empty() {
        return true;
    }
    let folder = plain(folder);
    event.paths.iter().map(|path| plain(path)).any(|path| {
        if path == folder {
            matches!(
                event.kind,
                EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
            )
        } else {
            path.parent() != folder.parent() || folder.parent().is_none()
        }
    })
}

pub fn watch(folder: PathBuf, tell: impl Fn(&Path) + Send + 'static) -> notify::Result<Watching> {
    let (heard, news) = unbounded::<()>();
    let own = folder.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if is_news(&own, &event) {
            let _ = heard.send(());
        }
    })?;
    watcher.watch(&folder, RecursiveMode::NonRecursive)?;
    if let Some(parent) = folder.parent() {
        let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
    }

    thread::spawn(move || {
        let mut last = None;
        while news.recv().is_ok() {
            let when = due(Instant::now(), last);
            loop {
                match news.recv_deadline(when) {
                    Ok(()) => continue,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            tell(&folder);
            last = Some(Instant::now());
        }
    });
    Ok(Watching { _watcher: watcher })
}

#[derive(Default)]
pub struct ShownFolder(Mutex<Option<(PathBuf, Watching)>>);

impl ShownFolder {
    pub fn set(&self, folder: Option<PathBuf>, tell: impl Fn(&Path) + Send + 'static) -> Result<(), String> {
        let mut shown = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if shown.as_ref().map(|(path, _)| path) == folder.as_ref() {
            return Ok(());
        }
        *shown = None;
        if let Some(folder) = folder {
            let watching = watch(folder.clone(), tell).map_err(|e| e.to_string())?;
            *shown = Some((folder, watching));
        }
        Ok(())
    }
}
