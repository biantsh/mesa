use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    ops::Bound,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        mpsc::{self, Sender},
    },
    thread,
};

use serde::Serialize;

use crate::{
    browse,
    embed::{Device, Embedder},
    indexer,
    progress::{Progress, ProgressState},
    rules::{self, Rules, effective, gaps_of, inclusions_of},
    store::{self, Store},
    unindexable,
};

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum IndexStatus {
    NotIndexed,
    Indexing,
    Paused,
    Indexed,
    PartlyIndexed,
    Unindexable,
    Excluded,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IndexState {
    pub status: IndexStatus,
    pub droppable: bool,
    pub excluded_here: bool,
}

pub struct Job {
    pub id: i64,
    pub root: PathBuf,
    pub ticket: u64,
    pub waiting: bool,
}

pub struct Shared {
    pub store: Mutex<Store>,
    pub rules: Mutex<Rules>,
    pub gaps: Mutex<BTreeSet<PathBuf>>,
    pub included: Mutex<BTreeSet<PathBuf>>,
    pub excluded: Mutex<HashSet<PathBuf>>,
    pub pending: Mutex<Pending>,
    pub progress: Mutex<ProgressState>,
    pub control: Control,
    pub embedder: Arc<dyn Embedder>,
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

    pub fn epoch(&self) -> u64 {
        lock(&self.state).epoch
    }

    pub fn changed_since(&self, epoch: u64) -> bool {
        let state = lock(&self.state);
        state.paused || state.epoch != epoch
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
}

impl Shared {
    pub fn is_indexed(&self, path: &Path) -> bool {
        !unindexable::is_unindexable(path) && !self.is_excluded(path) && effective(&lock(&self.rules), path)
    }

    pub fn has_gap_below(&self, path: &Path) -> bool {
        lock(&self.gaps)
            .range::<Path, _>((Bound::Excluded(path), Bound::Unbounded))
            .next()
            .is_some_and(|below| below.starts_with(path))
    }

    pub fn has_inclusion_below(&self, path: &Path) -> bool {
        let included = lock(&self.included);
        included
            .range::<Path, _>((Bound::Excluded(path), Bound::Unbounded))
            .take_while(|below| below.starts_with(path))
            .any(|below| !unindexable::is_unindexable(below) && !self.is_excluded(below))
    }

    pub fn is_excluded(&self, path: &Path) -> bool {
        let excluded = lock(&self.excluded);
        !excluded.is_empty() && path.ancestors().any(|above| excluded.contains(above))
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
    pub fn open(dir: &Path, embedder: Arc<dyn Embedder>) -> store::Result<Self> {
        let store = Store::open(dir)?;
        let rules = store.load_rules()?;
        let excluded = store.load_exclusions()?;
        let mut unfinished = store.unfinished_jobs()?;
        if store.has_stale_files()? {
            for (root, _) in rules.iter().filter(|(_, indexed)| **indexed) {
                if !unfinished.iter().any(|(_, queued)| queued == root) {
                    unfinished.push((store.add_job(root)?, root.clone()));
                }
            }
        }
        let paused = store.is_paused()?;
        if paused && unfinished.is_empty() {
            store.set_paused(false)?;
        }
        let shared = Arc::new(Shared {
            store: Mutex::new(store),
            gaps: Mutex::new(gaps_of(&rules)),
            included: Mutex::new(inclusions_of(&rules)),
            rules: Mutex::new(rules),
            excluded: Mutex::new(excluded),
            pending: Mutex::new(Pending::default()),
            progress: Mutex::new(ProgressState::default()),
            control: Control::new(paused && !unfinished.is_empty()),
            embedder,
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
        if unindexable::is_unindexable(path) {
            return IndexStatus::Unindexable;
        }
        if self.shared.is_excluded(path) {
            return IndexStatus::Excluded;
        }
        if !effective(&lock(&self.shared.rules), path) {
            return if self.shared.has_inclusion_below(path) {
                IndexStatus::PartlyIndexed
            } else {
                IndexStatus::NotIndexed
            };
        }
        let working = lock(&self.shared.pending).touches(path);
        if working || lock(&self.shared.progress).is_waiting(path) {
            if self.shared.control.is_paused() {
                IndexStatus::Paused
            } else {
                IndexStatus::Indexing
            }
        } else if self.shared.has_gap_below(path) {
            IndexStatus::PartlyIndexed
        } else {
            IndexStatus::Indexed
        }
    }

    pub fn state(&self, path: &Path) -> IndexState {
        let status = self.status(path);
        let droppable = lock(&self.shared.progress).is_job_root(path);
        let excluded_here = lock(&self.shared.excluded).contains(path);
        IndexState {
            status,
            droppable,
            excluded_here,
        }
    }

    pub fn drop_jobs(&self, paths: &[PathBuf]) -> Result<(), String> {
        let (mut dropped, mut ids) = (Vec::new(), Vec::new());
        for path in paths {
            let id = lock(&self.shared.progress).drop_job(path);
            if let Some(id) = id {
                ids.push(id);
                dropped.push(path.clone());
            }
        }
        if dropped.is_empty() {
            return Err("That isn't in the queue any more.".into());
        }
        lock(&self.shared.store).finish_jobs(&ids)?;
        self.set_many(&dropped, false)
    }

    #[cfg(test)]
    pub fn set(&self, path: &Path, indexed: bool) -> Result<(), String> {
        self.set_many(std::slice::from_ref(&path.to_path_buf()), indexed)
    }

    pub fn set_many(&self, paths: &[PathBuf], indexed: bool) -> Result<(), String> {
        if indexed {
            self.refuse_to_add(&paths.iter().map(PathBuf::as_path).collect::<Vec<_>>())?;
        }
        let mut store = lock(&self.shared.store);
        let mut rules = lock(&self.shared.rules).clone();
        rules::set_rules(&mut rules, paths, indexed);
        if !indexed {
            self.drop_emptied_parents(&mut rules, paths);
        }
        self.replace_rules(&mut store, rules)?;
        self.shared.control.poke();
        if indexed {
            self.start_jobs(&store, paths)
        } else {
            Ok(store.remove_paths(paths)?)
        }
    }

    fn refuse_to_add(&self, paths: &[&Path]) -> Result<(), String> {
        if paths.iter().any(|path| unindexable::is_unindexable(path)) {
            return Err("That can't be indexed.".into());
        }
        if paths.iter().any(|path| self.shared.is_excluded(path)) {
            return Err("That is excluded. Stop excluding it first.".into());
        }
        Ok(())
    }

    fn drop_emptied_parents(&self, rules: &mut Rules, below: &[PathBuf]) {
        let mut parents: Vec<PathBuf> = below
            .iter()
            .filter_map(|path| path.parent())
            .map(Path::to_path_buf)
            .collect();
        parents.sort();
        parents.dedup();

        while let Some(parent) = parents.pop() {
            let indexed =
                effective(rules, &parent) && !self.shared.is_excluded(&parent) && !unindexable::is_unindexable(&parent);
            if !indexed || self.holds_something_indexed(&parent, rules) {
                continue;
            }
            rules::set_rule(rules, &parent, false);
            parents.extend(parent.parent().map(Path::to_path_buf));
        }
    }

    fn holds_something_indexed(&self, folder: &Path, rules: &Rules) -> bool {
        let Ok(items) = fs::read_dir(folder) else { return true };
        items.flatten().any(|item| {
            let child = item.path();
            browse::classify(&item).is_some()
                && !unindexable::is_unindexable(&child)
                && !self.shared.is_excluded(&child)
                && effective(rules, &child)
        })
    }

    pub fn add_completing(&self, paths: &[PathBuf], folder: &Path) -> Result<(), String> {
        let mut everything: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
        everything.push(folder);
        self.refuse_to_add(&everything)?;

        let mut store = lock(&self.shared.store);
        let mut rules = lock(&self.shared.rules).clone();
        rules::set_rules(&mut rules, paths, true);
        rules::set_rule(&mut rules, folder, true);
        self.replace_rules(&mut store, rules)?;
        self.shared.control.poke();
        self.start_jobs(&store, paths)
    }

    pub fn exclude_many(&self, paths: &[PathBuf]) -> Result<(), String> {
        let mut ids = Vec::new();
        for path in paths {
            let roots = lock(&self.shared.progress).roots_under(path);
            for root in roots {
                let id = lock(&self.shared.progress).drop_job(&root);
                ids.extend(id);
            }
        }

        let mut store = lock(&self.shared.store);
        store.finish_jobs(&ids)?;
        let mut rules = lock(&self.shared.rules).clone();
        let mut excluded = lock(&self.shared.excluded).clone();
        rules::drop_under(&mut rules, paths);
        let covered: HashSet<&Path> = paths.iter().map(PathBuf::as_path).collect();
        excluded.retain(|e| !e.ancestors().any(|above| covered.contains(above)));
        for path in rules::outermost(paths) {
            if !path.ancestors().skip(1).any(|above| excluded.contains(above)) {
                excluded.insert(path.clone());
            }
        }
        store.save_exclusions(&excluded)?;
        *lock(&self.shared.excluded) = excluded;
        self.drop_emptied_parents(&mut rules, paths);
        self.replace_rules(&mut store, rules)?;
        self.shared.control.poke();
        Ok(store.remove_paths(paths)?)
    }

    pub fn stop_excluding_many(&self, paths: &[PathBuf]) -> Result<(), String> {
        let mut store = lock(&self.shared.store);
        let rules = lock(&self.shared.rules).clone();
        let mut excluded = lock(&self.shared.excluded).clone();
        let ended: Vec<&PathBuf> = paths.iter().filter(|path| excluded.remove(*path)).collect();
        if ended.is_empty() {
            return Ok(());
        }
        store.save_exclusions(&excluded)?;
        *lock(&self.shared.excluded) = excluded;
        self.shared.control.poke();
        let to_index: Vec<PathBuf> = ended
            .into_iter()
            .filter(|path| {
                effective(&rules, path) && !self.shared.is_excluded(path) && !unindexable::is_unindexable(path)
            })
            .cloned()
            .collect();
        self.start_jobs(&store, &to_index)
    }

    pub fn count_with_data(&self, paths: &[PathBuf]) -> usize {
        let rules = lock(&self.shared.rules);
        let included = lock(&self.shared.included);
        paths
            .iter()
            .filter(|path| !self.shared.is_excluded(path))
            .filter(|path| {
                effective(&rules, path)
                    || included
                        .range::<Path, _>((Bound::Included(path.as_path()), Bound::Unbounded))
                        .next()
                        .is_some_and(|below| below.starts_with(path))
            })
            .count()
    }

    pub fn exclusion_count(&self) -> usize {
        lock(&self.shared.excluded).len()
    }

    pub fn clear(&self) -> Result<(), String> {
        let mut store = lock(&self.shared.store);
        lock(&self.shared.rules).clear();
        lock(&self.shared.gaps).clear();
        lock(&self.shared.included).clear();
        lock(&self.shared.excluded).clear();
        lock(&self.shared.progress).cancel_all();
        self.shared.control.set_paused(false);
        Ok(store.reset()?)
    }

    pub fn gpu(&self) -> Option<String> {
        self.shared.embedder.gpu()
    }

    pub fn set_device(&self, device: Device) -> Result<(), String> {
        self.shared.embedder.set_device(device).map_err(|e| e.to_string())
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
        let ids: Vec<i64> = cancelled.iter().map(|(id, _)| *id).collect();
        let roots: Vec<PathBuf> = cancelled.into_iter().map(|(_, root)| root).collect();
        lock(&self.shared.store).finish_jobs(&ids)?;
        if !roots.is_empty() {
            self.set_many(&roots, false)?;
        }
        self.set_paused(false)
    }

    pub fn set_everything(&self, roots: &[PathBuf]) -> Result<(), String> {
        let mut store = lock(&self.shared.store);
        let rules = roots.iter().map(|root| (root.clone(), true)).collect();
        self.replace_rules(&mut store, rules)?;
        self.start_jobs(&store, roots)
    }

    fn replace_rules(&self, store: &mut Store, rules: Rules) -> Result<(), String> {
        store.save_rules(&rules)?;
        *lock(&self.shared.gaps) = gaps_of(&rules);
        *lock(&self.shared.included) = inclusions_of(&rules);
        *lock(&self.shared.rules) = rules;
        Ok(())
    }

    fn start_jobs(&self, store: &Store, roots: &[PathBuf]) -> Result<(), String> {
        if roots.is_empty() {
            return Ok(());
        }
        let ids = store.add_jobs(roots)?;
        if !lock(&self.shared.progress).is_active() {
            self.shared.set_paused(store, false)?;
        }
        for (id, root) in ids.into_iter().zip(roots) {
            self.queue_job(id, root.clone());
        }
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

impl Drop for Index {
    fn drop(&mut self) {
        self.shared.embedder.release();
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
        path.ancestors().any(|above| self.paths.contains_key(above))
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

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        fs,
        time::{Duration, Instant},
    };

    use super::*;

    fn embedder() -> Arc<dyn Embedder> {
        Arc::new(crate::embed::testing::Everything)
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
            let index = Index::open(&data, embedder()).unwrap();
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
        assert_eq!(f.count("free_slots"), 1);
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
            vectors: vec![crate::vectors::Located {
                locator: 0,
                record: crate::vectors::random_record(),
            }],
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
        let index = Index::open(&data, embedder()).unwrap();
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
        assert_eq!(
            Index::open(&f.data, embedder()).unwrap().status(&f.photos),
            IndexStatus::Indexed
        );
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
    fn exclusions_survive_restart() {
        let f = Fixture::new("exclusion-persists");
        let sub = f.photos.join("sub");
        f.index.exclude_many(&[sub.clone()]).unwrap();

        let reopened = Index::open(&f.data, embedder()).unwrap();
        assert_eq!(reopened.status(&sub), IndexStatus::Excluded);
        assert_eq!(reopened.exclusion_count(), 1);
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
        let index = Index::open(&data, embedder()).unwrap();
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
