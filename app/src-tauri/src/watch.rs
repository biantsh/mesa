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

use crossbeam_channel::{RecvTimeoutError, Sender, unbounded};
use notify::{
    Event, EventKind, RecursiveMode, Watcher,
    event::{ModifyKind, RenameMode},
};

use crate::{browse, index::Chores};

#[cfg_attr(target_os = "linux", allow(dead_code))]
pub const SETTLE: Duration = Duration::from_secs(4);
const MOST_DEFERRED: Duration = Duration::from_secs(60);
const BULK: usize = 16;
const TICK: Duration = Duration::from_millis(200);
const SYNC_EVERY: Duration = Duration::from_secs(1);
const TRY_AGAIN_AFTER: Duration = Duration::from_secs(30);
const RESCAN_EVERY: Duration = Duration::from_secs(60);
const MOST_PATHS: usize = 100_000;

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const LOOK_EVERY: Duration = Duration::from_secs(5 * 60);

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const LOOK_AFTER_OPENING: Duration = Duration::from_millis(1500);

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const LOOK_AT_MOST_EVERY: Duration = Duration::from_secs(60);

pub struct Watching {
    stop: Arc<AtomicBool>,
    wake: Option<Sender<()>>,
}

impl Watching {
    pub fn look_soon(&self) {
        if let Some(wake) = &self.wake {
            let _ = wake.send(());
        }
    }
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
    Watching { stop, wake: None }
}

pub fn poll(chores: Chores, every: Duration, after_asking: Duration, at_most_every: Duration) -> Watching {
    let stop = Arc::new(AtomicBool::new(false));
    let (wake, woken) = unbounded::<()>();
    let spawned = thread::Builder::new().name("rescanner".into()).spawn({
        let stop = stop.clone();
        move || {
            let mut last = Instant::now();
            loop {
                let asked = match woken.recv_timeout(every.saturating_sub(last.elapsed())) {
                    Ok(()) => true,
                    Err(RecvTimeoutError::Timeout) => false,
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                if asked {
                    if last.elapsed() < at_most_every {
                        continue;
                    }
                    thread::sleep(after_asking);
                    while woken.try_recv().is_ok() {}
                }
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                chores.check_later(chores.roots());
                last = Instant::now();
            }
        }
    });
    if let Err(e) = spawned {
        eprintln!("Mesa: can't look for changes to what is indexed: {e}");
    }
    Watching { stop, wake: Some(wake) }
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
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Heard {
    fn hear(&mut self, what: What) {
        match what {
            What::Arrived => self.arrived = true,
            What::Changed => self.changed = true,
            What::Gone => {}
        }
        let now = Instant::now();
        self.first.get_or_insert(now);
        self.last = Some(now);
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
    aliases: Vec<(PathBuf, PathBuf)>,
    undirected_renames: bool,
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
            aliases: Vec::new(),
            undirected_renames: cfg!(target_os = "macos"),
        }
    }

    fn as_watched(&self, path: PathBuf) -> PathBuf {
        for (real, watched) in &self.aliases {
            if let Ok(rest) = path.strip_prefix(real) {
                return watched.join(rest);
            }
        }
        path
    }

    fn hear(&mut self, news: notify::Result<Event>) {
        let mut event = match news {
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
        if !self.aliases.is_empty() {
            event.paths = std::mem::take(&mut event.paths)
                .into_iter()
                .map(|path| self.as_watched(path))
                .collect();
        }
        let kind = if self.undirected_renames {
            directed(event.kind, &event.paths)
        } else {
            event.kind
        };
        if let Some((from, _)) = &self.moved_from
            && !matches!(kind, EventKind::Modify(ModifyKind::Name(_)))
            && !event.paths.is_empty()
            && event.paths.iter().all(|path| path == from)
        {
            return;
        }
        let to_follows = matches!(kind, EventKind::Modify(ModifyKind::Name(RenameMode::To)));
        if !to_follows && let Some((from, _)) = self.moved_from.take() {
            self.note(from, What::Gone);
        }
        match kind {
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
        let (path, what) = match browse::package_holding(&path) {
            Some(package) => (package, What::Changed),
            None => (path, what),
        };
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
        let due = self.not_deferred(due, now);
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
                Ok(metadata) if metadata.is_dir() && !browse::is_package(&path) => {
                    if heard.arrived {
                        look.push(path);
                    }
                }
                Ok(_) => {
                    if browse::has_listed_name(&path) && (heard.arrived || heard.changed) {
                        look.push(path);
                    }
                }
            }
        }
        self.chores.forget(gone);
        let indexed = |folder: &Path| self.chores.is_indexed(folder);
        self.chores.check_later(by_folder(look, &indexed));
    }

    fn not_deferred(&self, due: Vec<PathBuf>, now: Instant) -> Vec<PathBuf> {
        if due.is_empty() {
            return due;
        }
        let mut busy: HashSet<&Path> = HashSet::new();
        for (path, heard) in &self.heard {
            if heard.last.is_some_and(|at| now.duration_since(at) >= self.settle) {
                continue;
            }
            for above in path.ancestors().skip(1) {
                if !busy.insert(above) {
                    break;
                }
            }
        }
        if busy.is_empty() {
            return due;
        }
        due.into_iter()
            .filter(|path| {
                let waiting =
                    busy.contains(path.as_path()) || path.parent().is_some_and(|folder| busy.contains(folder));
                let too_long = self
                    .heard
                    .get(path)
                    .and_then(|heard| heard.first)
                    .is_some_and(|at| now.duration_since(at) >= MOST_DEFERRED);
                !waiting || too_long
            })
            .collect()
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
        self.aliases = if cfg!(unix) {
            wanted
                .keys()
                .filter_map(|path| {
                    fs::canonicalize(path)
                        .ok()
                        .filter(|real| real != path)
                        .map(|real| (real, path.clone()))
                })
                .collect()
        } else {
            Vec::new()
        };
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

fn by_folder(look: Vec<PathBuf>, indexed: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut in_folder: HashMap<&Path, usize> = HashMap::new();
    for path in &look {
        if let Some(folder) = path.parent() {
            *in_folder.entry(folder).or_default() += 1;
        }
    }
    let many: BTreeSet<PathBuf> = in_folder
        .into_iter()
        .filter(|&(folder, n)| n >= BULK && indexed(folder))
        .map(|(folder, _)| folder.to_owned())
        .collect();
    if many.is_empty() {
        return look;
    }
    let mut out: Vec<PathBuf> = look
        .into_iter()
        .filter(|path| !path.parent().is_some_and(|folder| many.contains(folder)))
        .collect();
    out.extend(many);
    out
}

fn directed(kind: EventKind, paths: &[PathBuf]) -> EventKind {
    match (kind, paths) {
        (EventKind::Modify(ModifyKind::Name(RenameMode::Any)), [path]) => {
            let mode = if fs::symlink_metadata(path).is_ok() {
                RenameMode::To
            } else {
                RenameMode::From
            };
            EventKind::Modify(ModifyKind::Name(mode))
        }
        _ => kind,
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
        What::Changed => browse::has_listed_name(path),
        What::Arrived => browse::has_listed_name(path) || !name.contains('.') || path.is_dir(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_split_rename() {
        use notify::event::{DataChange, MetadataKind};
        let root = std::env::temp_dir().join(format!("mesa-watch-rename-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let photos = root.join("photos");
        fs::create_dir_all(&photos).unwrap();
        let index = crate::index::Index::open(
            &root.join("data"),
            std::sync::Arc::new(crate::embed::testing::Everything),
        )
        .unwrap();
        index.set(&photos, true).unwrap();
        let mut state = State::new(index.chores(), Duration::from_millis(100));
        state.undirected_renames = true;
        fs::write(photos.join("first.jpg"), "x").unwrap();
        let news = |kind: EventKind, name: &str| Ok(Event::new(kind).add_path(photos.join(name)));
        state.hear(news(EventKind::Modify(ModifyKind::Name(RenameMode::Any)), "a.jpg"));
        state.hear(news(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Extended)),
            "a.jpg",
        ));
        state.hear(news(EventKind::Modify(ModifyKind::Data(DataChange::Content)), "a.jpg"));
        state.hear(news(EventKind::Modify(ModifyKind::Name(RenameMode::Any)), "first.jpg"));
        let moves: Vec<(PathBuf, PathBuf)> = state.moves.iter().map(|m| (m.from.clone(), m.to.clone())).collect();
        assert_eq!(moves, [(photos.join("a.jpg"), photos.join("first.jpg"))]);
        assert!(
            state.heard.is_empty(),
            "and nothing is looked at as made, changed or deleted: {:?}",
            state.heard.keys()
        );

        state.moves.clear();
        state.hear(news(EventKind::Modify(ModifyKind::Name(RenameMode::Any)), "b.jpg"));
        state.hear(news(EventKind::Create(notify::event::CreateKind::File), "c.jpg"));
        assert!(state.moved_from.is_none() && state.moves.is_empty());
        assert!(state.heard.contains_key(&photos.join("b.jpg")));
        drop(index);
        let _ = fs::remove_dir_all(&root);
    }
}
