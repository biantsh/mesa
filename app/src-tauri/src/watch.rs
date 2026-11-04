use std::{
    collections::{BTreeSet, HashMap},
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{RecvTimeoutError, unbounded};
use notify::{
    Event, EventKind, RecursiveMode, Watcher,
    event::{ModifyKind, RenameMode},
};

use crate::{browse, index::Chores};

pub const SETTLE: Duration = Duration::from_secs(4);
const TICK: Duration = Duration::from_millis(200);
const SYNC_EVERY: Duration = Duration::from_secs(1);
const TRY_AGAIN_AFTER: Duration = Duration::from_secs(30);
const RESCAN_EVERY: Duration = Duration::from_secs(60);
const MOST_PATHS: usize = 100_000;

pub struct Watching {
    stop: Arc<AtomicBool>,
}

impl Drop for Watching {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

pub fn start(chores: Chores, settle: Duration) -> Watching {
    let stop = Arc::new(AtomicBool::new(false));
    let spawned = thread::Builder::new().name("watcher".into()).spawn({
        let stop = stop.clone();
        move || run(chores, settle, &stop)
    });
    if let Err(e) = spawned {
        eprintln!("Mesa: can't watch for changes to what is indexed: {e}");
    }
    Watching { stop }
}

fn run(chores: Chores, settle: Duration, stop: &AtomicBool) {
    let (news_in, news) = unbounded::<notify::Result<Event>>();
    let mut watcher = match notify::recommended_watcher(move |event| {
        let _ = news_in.send(event);
    }) {
        Ok(watcher) => watcher,
        Err(e) => {
            eprintln!("Mesa: can't watch for changes to what is indexed: {e}");
            return;
        }
    };
    let mut state = State::new(chores, settle);
    let mut last_sync: Option<Instant> = None;
    while !stop.load(Ordering::SeqCst) {
        match news.recv_timeout(TICK) {
            Ok(event) => {
                state.hear(event);
                while let Ok(event) = news.try_recv() {
                    state.hear(event);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        state.settled();
        if state.lost_watches {
            state.lost_watches = false;
            for path in state.watching.drain().map(|(path, _)| path) {
                let _ = watcher.unwatch(&path);
            }
            last_sync = None;
        }
        if last_sync.is_none_or(|at| at.elapsed() >= SYNC_EVERY) {
            state.sync(&mut watcher);
            last_sync = Some(Instant::now());
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Heard {
    arrived: bool,
    changed: bool,
    gone: bool,
    last: Option<Instant>,
}

struct Move {
    from: PathBuf,
    to: PathBuf,
    at: Instant,
}

struct State {
    chores: Chores,
    settle: Duration,
    heard: HashMap<PathBuf, Heard>,
    moves: Vec<Move>,
    moved_from: Option<(PathBuf, Instant)>,
    watching: HashMap<PathBuf, RecursiveMode>,
    failed: HashMap<PathBuf, Instant>,
    last_rescan: Option<Instant>,
    lost_watches: bool,
}

impl State {
    fn new(chores: Chores, settle: Duration) -> Self {
        Self {
            chores,
            settle,
            heard: HashMap::new(),
            moves: Vec::new(),
            moved_from: None,
            watching: HashMap::new(),
            failed: HashMap::new(),
            last_rescan: None,
            lost_watches: false,
        }
    }

    fn hear(&mut self, news: notify::Result<Event>) {
        let event = match news {
            Ok(event) => event,
            Err(e) => {
                eprintln!("Mesa: the system's news of changes broke off, so everything is looked at again: {e}");
                self.lost_watches = true;
                self.rescan();
                return;
            }
        };
        if event.need_rescan() {
            self.rescan();
        }
        let to_follows = matches!(event.kind, EventKind::Modify(ModifyKind::Name(RenameMode::To)));
        if !to_follows && let Some((from, _)) = self.moved_from.take() {
            self.note(from, What::Gone);
        }
        match event.kind {
            EventKind::Create(_) => event.paths.into_iter().for_each(|path| self.note(path, What::Arrived)),
            EventKind::Remove(_) => event.paths.into_iter().for_each(|path| self.note(path, What::Gone)),
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                self.moved_from = event.paths.into_iter().next().map(|path| (path, Instant::now()));
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                let Some(to) = event.paths.into_iter().next() else {
                    return;
                };
                match self.moved_from.take() {
                    Some((from, _)) => self.moved(from, to),
                    None => self.note(to, What::Arrived),
                }
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                let mut paths = event.paths.into_iter();
                if let (Some(from), Some(to)) = (paths.next(), paths.next()) {
                    self.moved(from, to);
                }
            }
            EventKind::Modify(ModifyKind::Name(_)) => {
                event.paths.into_iter().for_each(|path| self.note(path, What::Arrived));
            }
            EventKind::Modify(_) => event.paths.into_iter().for_each(|path| self.note(path, What::Changed)),
            _ => {}
        }
    }

    fn note(&mut self, path: PathBuf, what: What) {
        if !self.chores.is_indexed(&path) || !worth_a_look(&path, what) {
            return;
        }
        if self.heard.len() >= MOST_PATHS {
            self.heard.clear();
            self.rescan();
            return;
        }
        let heard = self.heard.entry(path).or_default();
        match what {
            What::Arrived => heard.arrived = true,
            What::Changed => heard.changed = true,
            What::Gone => heard.gone = true,
        }
        heard.last = Some(Instant::now());
    }

    fn moved(&mut self, from: PathBuf, to: PathBuf) {
        if hidden_below_a_root(&to, &self.chores.roots()) {
            self.note(from, What::Gone);
        } else if self.chores.is_indexed(&from) || self.chores.is_indexed(&to) {
            self.moves.push(Move {
                from,
                to,
                at: Instant::now(),
            });
        }
    }

    fn rescan(&mut self) {
        if self.last_rescan.is_some_and(|at| at.elapsed() < RESCAN_EVERY) {
            return;
        }
        self.last_rescan = Some(Instant::now());
        self.chores.check_later(self.chores.roots());
    }

    fn settled(&mut self) {
        let settle = self.settle;
        if let Some((from, at)) = self.moved_from.take() {
            if at.elapsed() >= settle {
                self.note(from, What::Gone);
            } else {
                self.moved_from = Some((from, at));
            }
        }

        let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.moves)
            .into_iter()
            .partition(|m| m.at.elapsed() >= settle);
        self.moves = waiting;
        for Move { from, to, .. } in due {
            self.chores.moved(&from, &to);
        }

        let due: Vec<PathBuf> = self
            .heard
            .iter()
            .filter(|(_, heard)| heard.last.is_some_and(|at| at.elapsed() >= settle))
            .map(|(path, _)| path.clone())
            .collect();
        if due.is_empty() {
            return;
        }
        let mut gone = Vec::new();
        let mut look = Vec::new();
        let roots = self.chores.roots();
        for path in due {
            let Some(heard) = self.heard.remove(&path) else {
                continue;
            };
            match fs::symlink_metadata(&path) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => gone.push(path),
                Err(_) => {}
                Ok(_) if hidden_below_a_root(&path, &roots) => {}
                Ok(metadata) if metadata.is_dir() => {
                    if heard.arrived {
                        look.push(path);
                    }
                }
                Ok(_) => {
                    let supported = path
                        .file_name()
                        .is_some_and(|name| browse::category_of(&name.to_string_lossy()).is_some());
                    if supported && (heard.arrived || heard.changed) {
                        look.push(path);
                    }
                }
            }
        }
        self.chores.forget(gone);
        self.chores.check_later(look);
    }

    fn sync(&mut self, watcher: &mut impl Watcher) {
        let mut wanted: HashMap<PathBuf, RecursiveMode> = HashMap::new();
        for root in self.chores.roots() {
            if root.is_dir() {
                wanted.insert(root, RecursiveMode::Recursive);
            } else if let Some(parent) = root.parent().filter(|_| root.exists()) {
                wanted.entry(parent.to_owned()).or_insert(RecursiveMode::NonRecursive);
            } else {
                self.failed.entry(root).or_insert_with(Instant::now);
            }
        }
        let unwanted: Vec<PathBuf> = self
            .watching
            .keys()
            .filter(|path| !wanted.contains_key(*path))
            .cloned()
            .collect();
        for path in unwanted {
            let _ = watcher.unwatch(&path);
            self.watching.remove(&path);
        }
        self.failed
            .retain(|path, _| wanted.contains_key(path) || self.chores.roots().contains(path));

        for (path, mode) in wanted {
            if self.watching.contains_key(&path) {
                continue;
            }
            if self.failed.get(&path).is_some_and(|at| at.elapsed() < TRY_AGAIN_AFTER) {
                continue;
            }
            match watcher.watch(&path, mode) {
                Ok(()) => {
                    self.watching.insert(path.clone(), mode);
                    if self.failed.remove(&path).is_some() {
                        self.chores.check_later([path]);
                    }
                }
                Err(e) => {
                    if self.failed.insert(path.clone(), Instant::now()).is_none() {
                        eprintln!("Mesa: can't watch {} for changes: {e}", path.display());
                    }
                }
            }
        }
    }
}

fn hidden_below_a_root(path: &Path, roots: &BTreeSet<PathBuf>) -> bool {
    let Some(root) = roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())
    else {
        return false;
    };
    path.ancestors()
        .take_while(|above| *above != root.as_path())
        .any(browse::is_hidden_path)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum What {
    Arrived,
    Changed,
    Gone,
}

fn worth_a_look(path: &Path, what: What) -> bool {
    let Some(name) = path.file_name().map(|name| name.to_string_lossy()) else {
        return false;
    };
    match what {
        What::Gone => true,
        What::Changed => browse::category_of(&name).is_some(),
        What::Arrived => browse::category_of(&name).is_some() || !name.contains('.') || path.is_dir(),
    }
}
