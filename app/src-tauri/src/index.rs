use std::{
    collections::{BTreeMap, HashMap},
    ops::Bound,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        mpsc::{self, Sender},
    },
    thread,
    time::Duration,
};

use serde::Serialize;

use crate::{
    indexer,
    progress::{Progress, ProgressState},
    store::{self, Store},
};

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum IndexStatus {
    NotIndexed,
    Indexing,
    Paused,
    Queued,
    Indexed,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IndexState {
    pub status: IndexStatus,
    pub droppable: bool,
}

pub struct Job {
    pub id: i64,
    pub root: PathBuf,
    pub ticket: u64,
    pub waiting: bool,
}

pub struct Shared {
    pub store: Mutex<Store>,
    pub rules: Mutex<HashMap<PathBuf, bool>>,
    pub pending: Mutex<Pending>,
    pub progress: Mutex<ProgressState>,
    pub control: Control,
}

pub struct Control {
    state: Mutex<ControlState>,
    changed: Condvar,
}

struct ControlState {
    paused: bool,
    epoch: u64,
}

impl Control {
    pub fn new(paused: bool) -> Self {
        Self {
            state: Mutex::new(ControlState { paused, epoch: 0 }),
            changed: Condvar::new(),
        }
    }

    pub fn is_paused(&self) -> bool {
        lock(&self.state).paused
    }

    pub fn set_paused(&self, paused: bool) {
        let mut state = lock(&self.state);
        state.paused = paused;
        state.epoch += 1;
        self.changed.notify_all();
    }

    pub fn poke(&self) {
        lock(&self.state).epoch += 1;
        self.changed.notify_all();
    }

    pub fn wait_while_paused(&self) {
        let mut state = lock(&self.state);
        while state.paused {
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub fn sleep(&self, time: Duration) -> bool {
        let state = lock(&self.state);
        let epoch = state.epoch;
        let (_state, result) = self
            .changed
            .wait_timeout_while(state, time, |state| !state.paused && state.epoch == epoch)
            .unwrap_or_else(|e| e.into_inner());
        result.timed_out()
    }
}

impl Shared {
    pub fn is_indexed(&self, path: &Path) -> bool {
        effective(&lock(&self.rules), path)
    }

    pub fn set_paused(&self, store: &Store, paused: bool) -> store::Result<()> {
        store.set_paused(paused)?;
        self.control.set_paused(paused);
        Ok(())
    }
}

pub struct Index {
    shared: Arc<Shared>,
    queue: Mutex<Sender<Job>>,
}

impl Index {
    pub fn open(dir: &Path) -> store::Result<Self> {
        let store = Store::open(dir)?;
        let rules = store.load_rules()?;
        let unfinished = store.unfinished_jobs()?;
        let paused = store.is_paused()?;
        if paused && unfinished.is_empty() {
            store.set_paused(false)?;
        }
        let shared = Arc::new(Shared {
            store: Mutex::new(store),
            rules: Mutex::new(rules),
            pending: Mutex::new(Pending::default()),
            progress: Mutex::new(ProgressState::default()),
            control: Control::new(paused && !unfinished.is_empty()),
        });

        let (sender, receiver) = mpsc::channel();
        let worker = shared.clone();
        thread::Builder::new()
            .name("indexer".into())
            .spawn(move || indexer::work(&worker, receiver))
            .map_err(store::Error::Io)?;

        let index = Self {
            shared,
            queue: Mutex::new(sender),
        };
        for (id, root) in unfinished {
            index.queue_job(id, root);
        }
        Ok(index)
    }

    pub fn status(&self, path: &Path) -> IndexStatus {
        if !self.shared.is_indexed(path) {
            IndexStatus::NotIndexed
        } else if lock(&self.shared.pending).touches(path) {
            if self.shared.control.is_paused() {
                IndexStatus::Paused
            } else {
                IndexStatus::Indexing
            }
        } else if lock(&self.shared.progress).is_waiting(path) {
            IndexStatus::Queued
        } else {
            IndexStatus::Indexed
        }
    }

    pub fn state(&self, path: &Path) -> IndexState {
        let status = self.status(path);
        let droppable = lock(&self.shared.progress).is_job_root(path);
        IndexState { status, droppable }
    }

    pub fn drop_job(&self, path: &Path) -> Result<(), String> {
        let id = lock(&self.shared.progress)
            .drop_job(path)
            .ok_or("That isn't in the queue any more.")?;
        lock(&self.shared.store).finish_job(id)?;
        self.set(path, false)
    }

    pub fn set(&self, path: &Path, indexed: bool) -> Result<(), String> {
        let mut store = lock(&self.shared.store);
        let mut rules = lock(&self.shared.rules).clone();
        set_rule(&mut rules, path, indexed);
        self.replace_rules(&mut store, rules)?;
        self.shared.control.poke();
        if indexed {
            self.start_job(&store, path)
        } else {
            Ok(store.remove_path(path)?)
        }
    }

    pub fn clear(&self) -> Result<(), String> {
        let mut store = lock(&self.shared.store);
        lock(&self.shared.rules).clear();
        lock(&self.shared.progress).cancel_all();
        self.shared.control.set_paused(false);
        Ok(store.reset()?)
    }

    pub fn progress(&self) -> Option<Progress> {
        let paused = self.shared.control.is_paused();
        lock(&self.shared.progress).snapshot(paused)
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), String> {
        let store = lock(&self.shared.store);
        if paused && !lock(&self.shared.progress).is_active() {
            return Ok(());
        }
        Ok(self.shared.set_paused(&store, paused)?)
    }

    pub fn cancel(&self) -> Result<(), String> {
        let cancelled = lock(&self.shared.progress).cancel_all();
        for (id, root) in &cancelled {
            lock(&self.shared.store).finish_job(*id)?;
            self.set(root, false)?;
        }
        self.set_paused(false)
    }

    pub fn set_everything(&self, roots: &[PathBuf]) -> Result<(), String> {
        let mut store = lock(&self.shared.store);
        let rules = roots.iter().map(|root| (root.clone(), true)).collect();
        self.replace_rules(&mut store, rules)?;
        roots.iter().try_for_each(|root| self.start_job(&store, root))
    }

    fn replace_rules(&self, store: &mut Store, rules: HashMap<PathBuf, bool>) -> Result<(), String> {
        store.save_rules(&rules)?;
        *lock(&self.shared.rules) = rules;
        Ok(())
    }

    fn start_job(&self, store: &Store, root: &Path) -> Result<(), String> {
        let id = store.add_job(root)?;
        if !lock(&self.shared.progress).is_active() {
            self.shared.set_paused(store, false)?;
        }
        self.queue_job(id, root.to_owned());
        Ok(())
    }

    fn queue_job(&self, id: i64, root: PathBuf) {
        let (ticket, first) = lock(&self.shared.progress).add_job(id, root.clone());
        if first {
            lock(&self.shared.pending).add(root.clone());
        }
        let job = Job {
            id,
            root,
            ticket,
            waiting: !first,
        };
        let _ = lock(&self.queue).send(job);
    }
}

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default)]
pub struct Pending {
    paths: BTreeMap<PathBuf, u32>,
}

impl Pending {
    pub fn add(&mut self, path: PathBuf) {
        *self.paths.entry(path).or_insert(0) += 1;
    }

    pub fn release(&mut self, path: &Path) {
        if let Some(count) = self.paths.get_mut(path) {
            *count -= 1;
            if *count == 0 {
                self.paths.remove(path);
            }
        }
    }

    pub fn touches(&self, path: &Path) -> bool {
        self.paths.contains_key(path)
            || path.parent().is_some_and(|parent| self.paths.contains_key(parent))
            || self
                .paths
                .range::<Path, _>((Bound::Included(path), Bound::Unbounded))
                .next()
                .is_some_and(|(below, _)| below.starts_with(path))
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

fn effective(rules: &HashMap<PathBuf, bool>, path: &Path) -> bool {
    path.ancestors().find_map(|a| rules.get(a)).copied().unwrap_or(false)
}

fn set_rule(rules: &mut HashMap<PathBuf, bool>, path: &Path, indexed: bool) {
    rules.retain(|p, _| !p.starts_with(path));
    let inherited = path.parent().is_some_and(|parent| effective(rules, parent));
    if inherited != indexed {
        rules.insert(path.to_path_buf(), indexed);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{Duration, Instant},
    };

    use super::*;

    fn rules_with(indexed: &[&str]) -> HashMap<PathBuf, bool> {
        let mut rules = HashMap::new();
        for p in indexed {
            set_rule(&mut rules, Path::new(p), true);
        }
        rules
    }

    #[test]
    fn folder_covers_descendants() {
        let rules = rules_with(&["/photos"]);
        assert!(effective(&rules, Path::new("/photos")));
        assert!(effective(&rules, Path::new("/photos/2024/a.jpg")));
        assert!(!effective(&rules, Path::new("/music")));
    }

    #[test]
    fn prefix_sibling_not_covered() {
        let rules = rules_with(&["/photos"]);
        assert!(!effective(&rules, Path::new("/photos2/a.jpg")));
    }

    struct Fixture {
        data: PathBuf,
        photos: PathBuf,
        index: Index,
        _root: PathBuf,
    }

    fn make_photos(tag: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("mesa-index-{tag}"));
        let _ = fs::remove_dir_all(&root);
        let photos = root.join("photos");
        fs::create_dir_all(photos.join("sub")).unwrap();
        for name in ["a.jpg", "b.png", "notes.txt", "setup.exe", ".hidden.jpg", "sub/c.jpg"] {
            fs::write(photos.join(name), "x").unwrap();
        }
        (root, photos)
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let (root, photos) = make_photos(tag);
            let data = root.join("data");
            let index = Index::open(&data).unwrap();
            Self {
                data,
                photos,
                index,
                _root: root,
            }
        }

        fn settle(&self) {
            let started = Instant::now();
            loop {
                let busy = !lock(&self.index.shared.pending).is_empty()
                    || !lock(&self.index.shared.store).unfinished_jobs().unwrap().is_empty();
                if !busy {
                    return;
                }
                assert!(started.elapsed() < Duration::from_secs(20));
                thread::sleep(Duration::from_millis(5));
            }
        }

        fn count(&self, table: &str) -> i64 {
            lock(&self.index.shared.store).count_rows(table)
        }
    }

    #[test]
    fn indexes_folder_recursively() {
        let f = Fixture::new("folder");
        f.index.set(&f.photos, true).unwrap();
        f.settle();

        assert_eq!(f.count("files"), 4);
        assert_eq!(f.count("embeddings"), 4);
        assert_eq!(f.index.status(&f.photos), IndexStatus::Indexed);
        assert_eq!(
            f.index.status(&f.photos.join("sub").join("c.jpg")),
            IndexStatus::Indexed
        );
        assert_eq!(
            f.index.status(&f.photos.parent().unwrap().join("elsewhere")),
            IndexStatus::NotIndexed
        );
    }

    #[test]
    fn reindex_picks_up_changes() {
        let f = Fixture::new("again");
        f.index.set(&f.photos, true).unwrap();
        f.settle();

        fs::remove_file(f.photos.join("a.jpg")).unwrap();
        fs::write(f.photos.join("new.png"), "x").unwrap();
        fs::write(f.photos.join("sub").join("newer.mp4"), "x").unwrap();
        f.index.set(&f.photos, true).unwrap();
        f.settle();

        assert_eq!(f.count("files"), 5);
        assert_eq!(f.count("embeddings"), 5);
        assert_eq!(f.count("free_slots"), 0);
    }

    fn interrupted_run(tag: &str, paused: bool) -> (PathBuf, PathBuf, PathBuf) {
        use crate::{
            browse::Category,
            store::{ScannedDir, ScannedFile},
        };
        let (root, photos) = make_photos(tag);
        let data = root.join("data");
        let mut store = Store::open(&data).unwrap();
        store.save_rules(&HashMap::from([(photos.clone(), true)])).unwrap();
        store.add_job(&photos).unwrap();
        let a = ScannedFile {
            name: "a.jpg".into(),
            category: Category::Image,
            modified_ms: 1,
            size: 1,
        };
        let scan = ScannedDir {
            path: photos.clone(),
            files: vec![a],
            complete: false,
        };
        store.write_scans(&[scan], &|_| true).unwrap();
        store.set_paused(paused).unwrap();
        (root, data, photos)
    }

    fn open(root: PathBuf, data: PathBuf, photos: PathBuf) -> Fixture {
        let index = Index::open(&data).unwrap();
        Fixture {
            data,
            photos,
            index,
            _root: root,
        }
    }

    #[test]
    fn pause_survives_restart() {
        let (root, data, photos) = interrupted_run("paused-restart", true);
        let f = open(root, data, photos);

        assert_eq!(f.index.status(&f.photos), IndexStatus::Paused);
        assert_eq!(f.index.status(&f.photos.join("b.png")), IndexStatus::Paused);
        assert!(f.index.progress().unwrap().paused);
        thread::sleep(Duration::from_millis(150));
        assert_eq!(f.count("files"), 1);

        f.index.set_paused(false).unwrap();
        f.settle();
        assert_eq!(f.count("files"), 4);
        assert_eq!(f.index.status(&f.photos), IndexStatus::Indexed);
        assert!(f.index.progress().is_none());
        assert_eq!(Index::open(&f.data).unwrap().status(&f.photos), IndexStatus::Indexed);
    }

    #[test]
    fn cancel_removes_partial_work() {
        let (root, data, photos) = interrupted_run("cancel", true);
        let f = open(root, data, photos);
        assert_eq!(f.count("files"), 1);

        f.index.cancel().unwrap();
        assert!(f.index.progress().is_none());
        f.settle();
        assert_eq!(f.count("files"), 0);
        assert_eq!(f.count("embeddings"), 0);
        assert_eq!(f.index.status(&f.photos), IndexStatus::NotIndexed);
        assert!(f.index.progress().is_none());
        assert!(!f.index.shared.control.is_paused());
        assert!(lock(&f.index.shared.store).load_rules().unwrap().is_empty());
    }

    #[test]
    fn resumes_interrupted_indexing() {
        let (root, photos) = make_photos("resume");
        let data = root.join("data");
        {
            let mut store = Store::open(&data).unwrap();
            store.save_rules(&HashMap::from([(photos.clone(), true)])).unwrap();
            store.add_job(&photos).unwrap();
        }
        let index = Index::open(&data).unwrap();
        let f = Fixture {
            data,
            photos,
            index,
            _root: root,
        };
        f.settle();
        assert_eq!(f.count("files"), 4);
    }
}
