use std::{
    collections::{BTreeSet, HashMap, HashSet},
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
        let wait = if state.is_idle() {
            last_sync.map_or(Duration::ZERO, |at| SYNC_EVERY.saturating_sub(at.elapsed()))
        } else {
            TICK
        };
        match news.recv_timeout(wait) {
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
            for path in state.watching.drain() {
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

#[derive(Default)]
struct Heard {
    arrived: bool,
    changed: bool,
    last: Option<Instant>,
}

impl Heard {
    fn hear(&mut self, what: What) {
        match what {
            What::Arrived => self.arrived = true,
            What::Changed => self.changed = true,
            What::Gone => {}
        }
        self.last = Some(Instant::now());
    }
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
    watching: HashSet<PathBuf>,
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
            watching: HashSet::new(),
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
        if let Some(heard) = self.heard.get_mut(&path) {
            heard.hear(what);
            return;
        }
        if !self.chores.is_indexed(&path) || !worth_a_look(&path, what) {
            return;
        }
        if self.heard.len() >= MOST_PATHS {
            self.heard.clear();
            self.rescan();
            return;
        }
        self.heard.entry(path).or_default().hear(what);
    }

    fn moved(&mut self, from: PathBuf, to: PathBuf) {
        if self.chores.is_indexed(&from) || self.chores.is_indexed(&to) {
            self.moves.push(Move {
                from,
                to,
                at: Instant::now(),
            });
        }
    }

    fn is_idle(&self) -> bool {
        self.heard.is_empty() && self.moves.is_empty() && self.moved_from.is_none()
    }

    fn rescan(&mut self) {
        if self.last_rescan.is_some_and(|at| at.elapsed() < RESCAN_EVERY) {
            return;
        }
        self.last_rescan = Some(Instant::now());
        self.chores.check_later(self.chores.roots());
    }

    fn settled(&mut self) {
        let (settle, now) = (self.settle, Instant::now());
        if let Some((from, at)) = self.moved_from.take() {
            if now.duration_since(at) >= settle {
                self.note(from, What::Gone);
            } else {
                self.moved_from = Some((from, at));
            }
        }
        let (due_moves, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.moves)
            .into_iter()
            .partition(|m| now.duration_since(m.at) >= settle);
        self.moves = waiting;
        let due: Vec<PathBuf> = self
            .heard
            .iter()
            .filter(|(_, heard)| heard.last.is_some_and(|at| now.duration_since(at) >= settle))
            .map(|(path, _)| path.clone())
            .collect();
        if due_moves.is_empty() && due.is_empty() {
            return;
        }

        let roots = self.chores.roots();
        let mut gone = Vec::new();
        let mut look = Vec::new();
        for Move { from, to, .. } in due_moves {
            if hidden_below_a_root(&to, &roots) {
                gone.push(from);
            } else {
                self.chores.moved(&from, &to);
            }
        }
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
        let roots = self.chores.roots();
        let mut wanted: HashMap<PathBuf, RecursiveMode> = HashMap::new();
        for root in &roots {
            if root.is_dir() {
                wanted.insert(root.clone(), RecursiveMode::Recursive);
            } else if let Some(parent) = root.parent().filter(|_| root.exists()) {
                wanted.entry(parent.to_owned()).or_insert(RecursiveMode::NonRecursive);
            } else {
                self.failed.entry(root.clone()).or_insert_with(Instant::now);
            }
        }
        let unwanted: Vec<PathBuf> = self
            .watching
            .iter()
            .filter(|path| !wanted.contains_key(*path))
            .cloned()
            .collect();
        for path in unwanted {
            let _ = watcher.unwatch(&path);
            self.watching.remove(&path);
        }
        self.failed
            .retain(|path, _| wanted.contains_key(path) || roots.contains(path));

        for (path, mode) in wanted {
            if self.watching.contains(&path) {
                continue;
            }
            if self.failed.get(&path).is_some_and(|at| at.elapsed() < TRY_AGAIN_AFTER) {
                continue;
            }
            match watcher.watch(&path, mode) {
                Ok(()) => {
                    self.watching.insert(path.clone());
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
