use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    ffi::{OsStr, OsString},
    ops::Bound,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
        mpsc::{self, Sender},
    },
    thread,
    time::Duration,
};

use serde::Serialize;

use crate::{
    browse::{self, Categories},
    crawl,
    embed::{Device, Embedder, ModelStatus},
    examples::path_key,
    indexer,
    progress::{Progress, ProgressState},
    relevance::Ranked,
    rules::{self, Rules, covered, effective},
    store::{self, CloudOnly, Counts, Filter, Hit, Store},
    unindexable,
};

pub const SEARCH_RESULTS: usize = 24;

struct Ranking {
    id: u64,
    ranked: Vec<(i64, f32)>,
    close: usize,
    excluded: Vec<String>,
    reasons: HashMap<i64, crate::relevance::Reason>,
    by_words: HashMap<i64, crate::store::WordsMatch>,
    at: HashMap<i64, u64>,
    next: Mutex<usize>,
    copied: Mutex<HashSet<i64>>,
}

impl Ranking {
    fn why(&self, file: i64) -> crate::store::Why {
        crate::store::Why {
            reason: self.reasons.get(&file).copied(),
            by_words: self.by_words.get(&file).copied(),
            at: self.at.get(&file).copied(),
        }
    }
}

#[derive(Debug)]
pub struct Page {
    pub id: u64,
    pub hits: Vec<Hit>,
    pub more: bool,
    pub close: bool,
    pub more_close: bool,
}

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
    TypeNotIndexed,
    CloudOnly,
    PartlyCloudOnly,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IndexState {
    pub status: IndexStatus,
    pub queued: bool,
    pub excluded_here: bool,
    pub removable: bool,
}

pub struct Job {
    pub id: i64,
    pub root: PathBuf,
    pub ticket: u64,
}

#[derive(Default)]
pub struct Quiet {
    order: VecDeque<PathBuf>,
    queued: HashSet<PathBuf>,
}

impl Quiet {
    pub fn add(&mut self, path: PathBuf) -> bool {
        if path.ancestors().any(|above| self.queued.contains(above)) {
            return false;
        }
        self.queued.insert(path.clone());
        self.order.push_back(path);
        true
    }

    pub fn again(&mut self, path: PathBuf) {
        if self.queued.insert(path.clone()) {
            self.order.push_front(path);
        }
    }

    pub fn next(&mut self) -> Option<PathBuf> {
        let path = self.order.pop_front()?;
        self.queued.remove(&path);
        Some(path)
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

pub const QUERY_PIECES: usize = 64;
pub const DEFAULT_TYPES: [browse::Category; 2] = [browse::Category::Document, browse::Category::Image];

pub struct Shared {
    pub store: Mutex<Store>,
    pub rules: Mutex<Rules>,
    pub excluded: Mutex<BTreeSet<PathBuf>>,
    pub pending: Mutex<Pending>,
    pub progress: Mutex<ProgressState>,
    pub control: Control,
    pub quiet: Mutex<Quiet>,
    pub embedder: Arc<dyn Embedder>,
    pub counts: Arc<Counts>,
    pub indexed_types: AtomicU8,
    pub always_full_speed: AtomicBool,
    pub asked_checks: AtomicBool,
    pub breadth_first: AtomicBool,
    pub refining: Mutex<Refining>,
}

#[derive(Clone, Copy, Default)]
pub struct Refining {
    pub left: u64,
    pub total: u64,
}

pub struct Control {
    state: Mutex<ControlState>,
    changed: Condvar,
}

struct ControlState {
    paused: bool,
    epoch: u64,
    turbo: bool,
    work: u64,
    shut: bool,
}

impl Control {
    pub fn with_turbo(paused: bool, turbo: bool) -> Self {
        Self {
            state: Mutex::new(ControlState {
                paused,
                epoch: 0,
                turbo,
                work: 0,
                shut: false,
            }),
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

    pub fn is_turbo(&self) -> bool {
        lock(&self.state).turbo
    }

    pub fn set_turbo(&self, turbo: bool) {
        lock(&self.state).turbo = turbo;
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

    pub fn more_work(&self) {
        lock(&self.state).work += 1;
        self.changed.notify_all();
    }

    pub fn work_seen(&self) -> u64 {
        lock(&self.state).work
    }

    pub fn wait_for_work(&self, seen: u64) -> bool {
        let state = lock(&self.state);
        let state = self
            .changed
            .wait_while(state, |state| state.work == seen && !state.shut)
            .unwrap_or_else(|e| e.into_inner());
        state.shut
    }

    pub fn is_shut(&self) -> bool {
        lock(&self.state).shut
    }

    pub fn shut_down(&self) {
        lock(&self.state).shut = true;
        self.changed.notify_all();
    }

    pub fn wait_while_paused(&self) {
        let mut state = lock(&self.state);
        while state.paused {
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub fn wait_while_paused_unless(&self, gone: impl Fn() -> bool) {
        loop {
            let epoch = {
                let state = lock(&self.state);
                if !state.paused {
                    return;
                }
                state.epoch
            };
            if gone() {
                return;
            }
            let state = lock(&self.state);
            drop(
                self.changed
                    .wait_while(state, |state| state.paused && state.epoch == epoch)
                    .unwrap_or_else(|e| e.into_inner()),
            );
        }
    }
}

impl Shared {
    pub fn types(&self) -> Categories {
        Categories::from_bits(self.indexed_types.load(Ordering::Relaxed))
    }

    pub fn is_gentle(&self) -> bool {
        !self.control.is_turbo()
    }

    pub fn is_breadth_first(&self) -> bool {
        self.breadth_first.load(Ordering::Relaxed)
    }

    pub fn set_turbo(&self, turbo: bool) {
        if self.control.is_turbo() != turbo {
            self.embedder.set_eco(!turbo);
            self.control.set_turbo(turbo);
        }
    }

    pub fn is_busy(&self) -> bool {
        lock(&self.progress).is_active() || self.refining().left > 0
    }

    pub fn refining(&self) -> Refining {
        *lock(&self.refining)
    }

    pub fn one_more_to_refine(&self) {
        let mut refining = lock(&self.refining);
        refining.left += 1;
        refining.total += 1;
    }

    pub fn left_to_refine(&self, left: u64) {
        let mut refining = lock(&self.refining);
        refining.total = if refining.left == 0 {
            left
        } else {
            refining.total.max(left)
        };
        refining.left = left;
    }

    pub fn recount_refining(&self, store: &Store) {
        let Ok(left) = store.shallow_count() else { return };
        let mut refining = lock(&self.refining);
        let gone = refining.left.saturating_sub(left);
        refining.total = refining.total.saturating_sub(gone).max(left);
        refining.left = left;
    }

    pub fn wait_while_paused_for(&self, ticket: Option<u64>) {
        match ticket {
            Some(ticket) => self
                .control
                .wait_while_paused_unless(|| !lock(&self.progress).has(ticket)),
            None => self.control.wait_while_paused(),
        }
    }

    pub fn crawl_threads(&self) -> usize {
        if self.is_gentle() {
            crawl::GENTLE_THREADS
        } else {
            crawl::threads()
        }
    }

    pub fn is_indexed(&self, path: &Path) -> bool {
        !unindexable::is_unindexable(path) && !self.is_excluded(path) && effective(&lock(&self.rules), path)
    }

    pub fn wants(&self, path: &Path) -> bool {
        let kind = path
            .file_name()
            .and_then(|name| browse::category_of(&name.to_string_lossy()));
        kind.is_none_or(|kind| self.types().contains(kind)) && self.is_indexed(path)
    }

    pub fn has_inclusion_below(&self, path: &Path) -> bool {
        let rules = lock(&self.rules);
        inside(&rules, path).any(|below| !unindexable::is_unindexable(below) && !self.is_excluded(below))
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
    counter: Mutex<Sender<()>>,
    latest_search: Mutex<HashMap<String, Arc<Ranking>>>,
    search_ids: AtomicU64,
    watching: Mutex<Option<crate::watch::Watching>>,
    examples: crate::examples::Examples,
    lists_version: AtomicU64,
    roots: Mutex<Option<(u64, Arc<Vec<PathBuf>>)>>,
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub files: u64,
    pub bytes: u64,
}

impl Index {
    #[cfg(test)]
    pub fn open(dir: &Path, embedder: Arc<dyn Embedder>) -> store::Result<Self> {
        Self::open_with(dir, embedder, true, Some(Categories::ALL))
    }

    pub fn open_with(
        dir: &Path,
        embedder: Arc<dyn Embedder>,
        turbo: bool,
        chosen: Option<Categories>,
    ) -> store::Result<Self> {
        embedder.set_eco(!turbo);
        let mut store = Store::open(dir)?;
        let reader = embedder.clone();
        store.read_pictures_if(move || reader.reads_text());
        let rules = store.load_rules()?;
        let types = match store.indexed_types()? {
            Some(kept) => kept,
            None => {
                let types = chosen.unwrap_or(if rules.is_empty() {
                    Categories::of(&DEFAULT_TYPES)
                } else {
                    Categories::BEFORE_CODE
                });
                store.set_indexed_types(types)?;
                types
            }
        };
        let excluded = store.load_exclusions()?;
        let mut unfinished = store.unfinished_jobs()?;
        if store.recover_lost_vectors()? {
            eprintln!(
                "Mesa: the index has files and no vectors for them (the vectors were lost), so they are indexed again"
            );
        }
        if store.has_stale_files()? {
            for root in &rules {
                if !unfinished.iter().any(|(_, queued)| queued == root) {
                    unfinished.push((store.add_job(root)?, root.clone()));
                }
            }
        }
        let roots: Vec<PathBuf> = unfinished.iter().map(|(_, root)| root.clone()).collect();
        let already = store.files_under_each(&roots).unwrap_or_else(|_| vec![0; roots.len()]);
        let paused = store.is_paused()?;
        if paused && unfinished.is_empty() {
            store.set_paused(false)?;
        }
        let counts = store.counts();
        let shared = Arc::new(Shared {
            store: Mutex::new(store),
            rules: Mutex::new(rules),
            excluded: Mutex::new(excluded),
            pending: Mutex::new(Pending::default()),
            progress: Mutex::new(ProgressState::default()),
            control: Control::with_turbo(paused && !unfinished.is_empty(), turbo),
            quiet: Mutex::default(),
            embedder,
            counts,
            indexed_types: AtomicU8::new(types.bits()),
            always_full_speed: AtomicBool::new(false),
            asked_checks: AtomicBool::new(false),
            breadth_first: AtomicBool::new(false),
            refining: Mutex::default(),
        });

        let worker = shared.clone();
        thread::Builder::new()
            .name("indexer".into())
            .spawn(move || indexer::work(&worker))
            .map_err(store::Error::Io)?;
        crate::reclaim::spawn(Arc::downgrade(&shared), dir.to_owned()).map_err(store::Error::Io)?;
        let (wake, woken) = mpsc::channel();
        let counter = shared.clone();
        thread::Builder::new()
            .name("counter".into())
            .spawn(move || indexer::count_ahead(&counter, woken))
            .map_err(store::Error::Io)?;

        let index = Self {
            shared,
            counter: Mutex::new(wake),
            latest_search: Mutex::default(),
            search_ids: AtomicU64::new(0),
            watching: Mutex::new(None),
            examples: Default::default(),
            lists_version: AtomicU64::new(0),
            roots: Mutex::new(None),
        };
        for ((id, root), already) in unfinished.into_iter().zip(already) {
            index.queue_job_having_done(id, root, already);
        }
        Ok(index)
    }

    pub fn shut_down(&self) {
        self.shared.embedder.release();
    }

    pub fn status(&self, path: &Path) -> IndexStatus {
        status_of(&Asked(&self.shared), path, None)
    }

    pub fn state(&self, path: &Path) -> IndexState {
        state_of(&Asked(&self.shared), path, None)
    }

    pub fn states_of(&self, paths: &[(&Path, Option<bool>)]) -> Vec<IndexState> {
        let mut states: Vec<Option<IndexState>> = vec![None; paths.len()];
        let mut folders: HashMap<&Path, Vec<usize>> = HashMap::new();
        for (at, (path, _)) in paths.iter().enumerate() {
            match (path.parent(), path.file_name()) {
                (Some(folder), Some(_)) => folders.entry(folder).or_default().push(at),
                _ => states[at] = Some(self.state(path)),
            }
        }
        for (folder, members) in folders {
            if members.len() < CHILDREN_TOGETHER {
                for at in members {
                    states[at] = Some(self.state(paths[at].0));
                }
                continue;
            }
            let children = Children::of(self, folder);
            for at in members {
                states[at] = Some(state_of(&children, paths[at].0, paths[at].1));
            }
        }
        states
            .into_iter()
            .map(|state| state.expect("every path is looked at"))
            .collect()
    }

    pub fn folder_states(&self, folder: &Path) -> FolderStates {
        FolderStates(Children::of(self, folder))
    }

    fn drop_jobs_under(&self, paths: &[PathBuf]) -> Vec<i64> {
        let mut pending = lock(&self.shared.pending);
        let mut progress = lock(&self.shared.progress);
        let roots: Vec<PathBuf> = paths.iter().flat_map(|path| progress.roots_under(path)).collect();
        let dropped = progress.drop_jobs(&roots);
        drop(progress);
        dropped
            .iter()
            .filter(|job| !job.started)
            .for_each(|job| pending.release(&job.root));
        dropped.into_iter().map(|job| job.id).collect()
    }

    #[cfg(test)]
    pub fn set(&self, path: &Path, indexed: bool) -> Result<(), String> {
        let paths = std::slice::from_ref(&path.to_path_buf()).to_vec();
        if indexed {
            self.add_many(&paths)
        } else {
            self.remove_many(&paths)
        }
    }

    pub fn add_many(&self, paths: &[PathBuf]) -> Result<(), String> {
        self.refuse_to_add(&paths.iter().map(PathBuf::as_path).collect::<Vec<_>>())?;
        let mut store = lock(&self.shared.store);
        let mut rules = lock(&self.shared.rules).clone();
        rules::add(&mut rules, paths);
        self.replace_rules(&mut store, rules)?;
        self.shared.control.poke();
        self.start_jobs(&store, paths)
    }

    pub fn remove_many(&self, paths: &[PathBuf]) -> Result<(), String> {
        // Drop the jobs before locking the store: the store is always locked before the queue, so holding the queue here could deadlock.
        let ended = self.drop_jobs_under(paths);
        let mut store = lock(&self.shared.store);
        store.finish_jobs(&ended)?;
        self.end_pause_if_idle(&store)?;
        let (outside, mut rules): (Vec<PathBuf>, Rules) = {
            let rules = lock(&self.shared.rules);
            (
                paths.iter().filter(|path| !covered(&rules, path)).cloned().collect(),
                rules.clone(),
            )
        };
        if ended.is_empty() && outside.is_empty() {
            return Ok(());
        }
        rules::drop_under(&mut rules, &outside);
        self.replace_rules(&mut store, rules)?;
        self.shared.control.poke();
        let removed = store.remove_paths(&outside);
        self.shared.recount_refining(&store);
        drop(store);
        Ok(removed?)
    }

    fn end_pause_if_idle(&self, store: &Store) -> Result<(), String> {
        if self.shared.control.is_paused() && !lock(&self.shared.progress).is_active() {
            self.shared.set_paused(store, false)?;
        }
        Ok(())
    }

    pub fn stop_indexing(&self) -> Result<(), String> {
        self.remove_many(&self.queued_roots())
    }

    fn refuse_to_add(&self, paths: &[&Path]) -> Result<(), String> {
        if paths.iter().any(|path| unindexable::is_unindexable(path)) {
            return Err("That can't be indexed.".into());
        }
        if paths.iter().any(|path| self.shared.is_excluded(path)) {
            return Err("That is excluded. Stop excluding it first.".into());
        }
        if paths
            .iter()
            .any(|path| self.status(path) == IndexStatus::TypeNotIndexed)
        {
            return Err("That kind of file isn't being indexed. Turn it on first.".into());
        }
        Ok(())
    }

    pub fn add_completing(&self, paths: &[PathBuf], folder: &Path) -> Result<(), String> {
        let mut everything: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
        everything.push(folder);
        self.refuse_to_add(&everything)?;

        let mut store = lock(&self.shared.store);
        let mut rules = lock(&self.shared.rules).clone();
        rules::add(&mut rules, paths);
        rules::add(&mut rules, std::slice::from_ref(&folder.to_path_buf()));
        self.replace_rules(&mut store, rules)?;
        self.shared.control.poke();
        self.start_jobs(&store, paths)
    }

    pub fn exclude_many(&self, paths: &[PathBuf]) -> Result<(), String> {
        let ids = self.drop_jobs_under(paths);

        let mut store = lock(&self.shared.store);
        store.finish_jobs(&ids)?;
        self.end_pause_if_idle(&store)?;
        let mut rules = lock(&self.shared.rules).clone();
        let before = lock(&self.shared.excluded).clone();
        let mut excluded = before.clone();
        rules::drop_under(&mut rules, paths);
        let covered: HashSet<&Path> = paths.iter().map(PathBuf::as_path).collect();
        excluded.retain(|e| !e.ancestors().any(|above| covered.contains(above)));
        for path in rules::outermost(paths) {
            if !path.ancestors().skip(1).any(|above| excluded.contains(above)) {
                excluded.insert(path.clone());
            }
        }
        store.save_exclusions_changes(&before, &excluded)?;
        *lock(&self.shared.excluded) = excluded;
        self.lists_changed();
        self.replace_rules(&mut store, rules)?;
        self.shared.control.poke();
        let removed = store.remove_paths(paths);
        self.shared.recount_refining(&store);
        drop(store);
        let mut progress = lock(&self.shared.progress);
        rules::outermost(paths)
            .into_iter()
            .for_each(|path| progress.excluded(path));
        drop(progress);
        Ok(removed?)
    }

    pub fn stop_excluding_many(&self, paths: &[PathBuf]) -> Result<(), String> {
        let mut store = lock(&self.shared.store);
        let rules = lock(&self.shared.rules).clone();
        let before = lock(&self.shared.excluded).clone();
        let mut excluded = before.clone();
        let ended: Vec<&PathBuf> = paths.iter().filter(|path| excluded.remove(*path)).collect();
        if ended.is_empty() {
            return Ok(());
        }
        store.save_exclusions_changes(&before, &excluded)?;
        *lock(&self.shared.excluded) = excluded;
        self.lists_changed();
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
        paths
            .iter()
            .filter(|path| !self.shared.is_excluded(path))
            .filter(|path| effective(&rules, path) || inside(&rules, path).next().is_some())
            .count()
    }

    pub fn is_excluded(&self, path: &Path) -> bool {
        self.shared.is_excluded(path)
    }

    pub fn indexed_roots(&self) -> Vec<PathBuf> {
        let rules = lock(&self.shared.rules);
        rules
            .iter()
            .filter(|path| {
                !covered(&rules, path) && !unindexable::is_unindexable(path) && !self.shared.is_excluded(path)
            })
            .cloned()
            .collect()
    }

    fn lists_changed(&self) {
        self.lists_version.fetch_add(1, Ordering::SeqCst);
    }

    pub fn lists_version(&self) -> u64 {
        self.lists_version.load(Ordering::SeqCst)
    }

    pub fn indexed_states(&self) -> Vec<IndexState> {
        let roots = self.indexed_roots_of_now();
        self.states_of(&roots.iter().map(|root| (root.as_path(), None)).collect::<Vec<_>>())
    }

    pub fn indexed_roots_of_now(&self) -> Arc<Vec<PathBuf>> {
        let version = self.lists_version();
        if let Some((kept, roots)) = &*lock(&self.roots)
            && *kept == version
        {
            return Arc::clone(roots);
        }
        let roots = Arc::new(self.indexed_roots());
        *lock(&self.roots) = Some((version, Arc::clone(&roots)));
        roots
    }

    pub fn queued_roots(&self) -> Vec<PathBuf> {
        lock(&self.shared.progress).job_roots()
    }

    pub fn excluded_paths(&self) -> Vec<PathBuf> {
        lock(&self.shared.excluded).iter().cloned().collect()
    }

    #[cfg(test)]
    pub fn search(&self, text: &str, filter: Filter) -> Result<Page, String> {
        self.search_like(text, &[], filter)
    }

    #[cfg(test)]
    pub fn search_like(&self, text: &str, files: &[PathBuf], filter: Filter) -> Result<Page, String> {
        self.search_for("", text, files, filter)
    }

    pub fn search_for(&self, window: &str, text: &str, files: &[PathBuf], filter: Filter) -> Result<Page, String> {
        self.search_where(window, text, files, filter, browse::is_file_like)
    }

    pub fn prepare_example(&self, path: &Path) -> Result<(), String> {
        let read_text =
            lock(&self.shared.store).picture_text(path).ok().flatten().is_none() && self.shared.embedder.reads_text();
        self.examples.add_reading(&self.shared.embedder, path, read_text)
    }

    pub fn example_progress(&self, paths: &[PathBuf]) -> Vec<Option<crate::examples::Progress>> {
        self.examples.progress(paths)
    }

    fn query_vector(&self, text: &str, files: &[PathBuf]) -> Result<Option<crate::vectors::Record>, String> {
        let mut parts = Vec::new();
        if !text.trim().is_empty() {
            parts.push((self.shared.embedder.embed_query(text).map_err(|e| e.to_string())?, 1.0));
        }
        let most = crate::examples::MOST_FILES;
        if files.len() > most {
            return Err(format!("At most {most} files can be searched with at once."));
        }
        let kept = files
            .iter()
            .map(|file| self.examples.vectors(&self.shared.embedder, file))
            .collect::<Result<Vec<_>, _>>()?;
        let shares = crate::examples::shares(&kept.iter().map(|(_, slots)| *slots).collect::<Vec<_>>(), QUERY_PIECES);
        let mut made = Vec::new();
        for ((vectors, _), share) in kept.into_iter().zip(shares) {
            let weighted: Vec<_> = vectors.into_iter().take(share).map(|record| (record, 1.0)).collect();
            made.extend(crate::vectors::blend(&weighted).map(|record| (record, 1.0)));
        }
        if let Some(files) = crate::vectors::blend(&made) {
            parts.push((files, 1.0));
        }
        Ok(match parts.as_slice() {
            [] => None,
            [(only, _)] => Some(*only),
            _ => crate::vectors::blend(&parts),
        })
    }

    fn search_where(
        &self,
        window: &str,
        text: &str,
        files: &[PathBuf],
        filter: Filter,
        exists: impl Fn(&Path) -> bool,
    ) -> Result<Page, String> {
        let query = self.query_vector(text, files)?;
        let mut by_words = HashMap::new();
        let ranked = if let Some(query) = query {
            let mut store = lock(&self.shared.store);
            let mut exact = store.exact(text, filter)?;
            let in_content: HashSet<i64> = exact.iter().map(|&(file, _)| file).collect();
            let name_only: Vec<i64> = store
                .named(text, filter)?
                .into_iter()
                .filter(|file| !in_content.contains(file))
                .collect();
            exact.extend(name_only.iter().map(|&file| (file, false)));
            let terms = crate::words::terms(text).len();
            let phrase = match terms {
                _ if exact.is_empty() => Vec::new(),
                1 => exact.iter().map(|&(file, _)| file).collect(),
                _ => store.phrase(text, filter)?,
            };
            let margins = crate::words::margins(&exact, &phrase);
            let in_order: HashSet<i64> = if terms > 1 {
                phrase.iter().copied().collect()
            } else {
                HashSet::new()
            };
            by_words = margins
                .keys()
                .map(|&file| {
                    (
                        file,
                        if in_order.contains(&file) {
                            crate::store::WordsMatch::Phrase
                        } else {
                            crate::store::WordsMatch::Words
                        },
                    )
                })
                .collect();
            for file in &name_only {
                if by_words.contains_key(file) {
                    by_words.insert(*file, crate::store::WordsMatch::Name);
                }
            }
            let mut ranked = store.rank(&query, filter, &margins)?;
            if !files.is_empty() || text.contains('.') {
                let read: Vec<(PathBuf, Option<String>)> = files
                    .iter()
                    .map(|file| {
                        let indexed = store.picture_text(file).ok().flatten();
                        (file.clone(), indexed.or_else(|| self.examples.text(file)))
                    })
                    .collect();
                let clued = crate::clues::code_by_clues(&store, text, &read, filter)?;
                if !clued.is_empty() {
                    put_first(&mut ranked, &clued, &store, &mut by_words)?;
                }
            }
            if looks_like_a_name(text) {
                let named = store.named_exactly(text, filter)?;
                if !named.is_empty() {
                    put_named_first(&mut ranked, &named, &mut by_words);
                }
            }
            ranked
        } else if filter != Filter::default() {
            Ranked::all_close(lock(&self.shared.store).newest(filter)?)
        } else {
            return Ok(Page {
                id: 0,
                hits: Vec::new(),
                more: false,
                close: true,
                more_close: false,
            });
        };
        let id = self.search_ids.fetch_add(1, Ordering::SeqCst) + 1;
        let ranking = Arc::new(Ranking {
            id,
            ranked: ranked.files,
            close: ranked.close,
            excluded: files.iter().map(|file| path_key(file)).collect(),
            reasons: ranked.reasons,
            by_words,
            at: ranked.at,
            next: Mutex::new(0),
            copied: Mutex::default(),
        });
        lock(&self.latest_search).insert(window.to_owned(), ranking.clone());
        if ranking.close == 0 {
            return Ok(Page {
                id,
                hits: Vec::new(),
                more: !ranking.ranked.is_empty(),
                close: true,
                more_close: false,
            });
        }
        self.next_page(&ranking, exists)
    }

    pub fn match_details(&self, path: &Path, query: &str, at: Option<u64>) -> Result<crate::matches::Details, String> {
        if crate::embed::code::is_code_path(path) {
            return crate::matches::of_code(path, query, at);
        }
        let (text, scanned) = {
            let store = lock(&self.shared.store);
            (store.picture_text(path)?, store.page_texts(path)?)
        };
        Ok(match text {
            Some(text) => crate::matches::of_picture(&text, query),
            None => crate::matches::of_document(path, query, at, &scanned).unwrap_or_default(),
        })
    }

    pub fn search_more(&self, id: u64) -> Result<Page, String> {
        let ranking = lock(&self.latest_search)
            .values()
            .find(|ranking| ranking.id == id)
            .cloned();
        let ranking = ranking.ok_or_else(|| "That search has been replaced by a newer one.".to_owned())?;
        self.next_page(&ranking, browse::is_file_like)
    }

    fn next_page(&self, ranking: &Ranking, exists: impl Fn(&Path) -> bool) -> Result<Page, String> {
        let mut next = lock(&ranking.next);
        let close = *next < ranking.close;
        let end_of_part = if close { ranking.close } else { ranking.ranked.len() };
        let mut found = Vec::with_capacity(SEARCH_RESULTS);
        while *next < end_of_part && found.len() < SEARCH_RESULTS {
            let end = (*next + SEARCH_RESULTS).min(end_of_part);
            let batch = &ranking.ranked[*next..end];
            let hits = lock(&self.shared.store).hits(batch)?;
            let mut used = batch.len();
            for mut hit in hits {
                if !exists(&hit.path)
                    || (!ranking.excluded.is_empty() && ranking.excluded.contains(&path_key(&hit.path)))
                {
                    continue;
                }
                let file = hit.file;
                if hit.category == browse::Category::Code {
                    if lock(&ranking.copied).contains(&file) {
                        continue;
                    }
                    let copies = self.copies_of(&hit)?;
                    lock(&ranking.copied).extend(copies.iter().map(|(id, _)| *id));
                    hit.copies = copies.into_iter().map(|(_, path)| path).collect();
                }
                hit.why = ranking.why(file);
                found.push(hit);
                if found.len() == SEARCH_RESULTS {
                    used = batch
                        .iter()
                        .position(|(id, _)| *id == file)
                        .map_or(batch.len(), |at| at + 1);
                    break;
                }
            }
            *next += used;
        }
        Ok(Page {
            id: ranking.id,
            hits: found,
            more: *next < ranking.ranked.len(),
            close,
            more_close: *next < ranking.close,
        })
    }

    pub fn code_projects(&self) -> Result<Vec<(String, PathBuf)>, String> {
        let folders = lock(&self.shared.store).code_folders()?;
        let mut roots: HashMap<PathBuf, usize> = HashMap::new();
        for folder in folders {
            if let Some(root) = crate::embed::code::project_root(&folder.join("_")) {
                *roots.entry(root).or_default() += 1;
            }
        }
        let mut by_name: HashMap<String, (PathBuf, usize)> = HashMap::new();
        for (root, count) in roots {
            let name = root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let entry = by_name.entry(name).or_insert((root.clone(), 0));
            if count > entry.1 {
                *entry = (root, count);
            }
        }
        let mut out: Vec<(String, PathBuf)> = by_name.into_iter().map(|(name, (root, _))| (name, root)).collect();
        out.sort();
        Ok(out)
    }

    pub fn code_project_folder(&self, name: &str) -> Result<Option<i64>, String> {
        let Some((_, root)) = self
            .code_projects()?
            .into_iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))
        else {
            return Ok(None);
        };
        Ok(lock(&self.shared.store).folder_id(&root)?)
    }

    fn copies_of(&self, hit: &crate::store::Hit) -> Result<Vec<(i64, PathBuf)>, String> {
        let candidates = lock(&self.shared.store).code_of_size(hit.size, hit.file)?;
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let Ok(this) = std::fs::read(&hit.path) else {
            return Ok(Vec::new());
        };
        Ok(candidates
            .into_iter()
            .filter(|(_, path)| std::fs::read(path).is_ok_and(|other| other == this))
            .collect())
    }

    pub fn also_in(&self, path: &Path, line: u64) -> Result<Vec<AlsoIn>, String> {
        let found = lock(&self.shared.store).like_code(path, line, ALSO_IN, 5)?;
        Ok(found
            .into_iter()
            .map(|(other, at, score)| {
                let symbol = crate::embed::code::chunks_of(&other)
                    .ok()
                    .and_then(|chunks| {
                        chunks
                            .into_iter()
                            .find(|chunk| u64::from(chunk.start) == at)
                            .map(|chunk| chunk.symbol)
                    })
                    .unwrap_or_default();
                AlsoIn {
                    path: other.to_string_lossy().into_owned(),
                    line: at,
                    symbol,
                    similarity: score,
                }
            })
            .collect())
    }

    pub fn gpu(&self) -> Option<String> {
        self.shared.embedder.gpu()
    }

    pub fn shared_gpu(&self) -> Option<String> {
        self.shared.embedder.shared_gpu()
    }

    pub fn model_status(&self) -> ModelStatus {
        self.shared.embedder.model_status()
    }

    pub fn retry_model_setup(&self) {
        self.shared.embedder.retry_model_setup();
    }

    pub fn set_device(&self, device: Device) -> Result<(), String> {
        self.shared.embedder.set_device(device).map_err(|e| e.to_string())
    }

    pub fn index_breadth_first(&self) {
        self.shared.breadth_first.store(true, Ordering::Relaxed);
        self.shared.control.more_work();
    }

    pub fn stats(&self) -> Stats {
        Stats {
            files: self.shared.counts.files(),
            bytes: self.shared.counts.bytes(),
        }
    }

    pub fn is_empty(&self) -> bool {
        lock(&self.shared.rules).is_empty()
    }

    pub fn progress(&self) -> Option<Progress> {
        let paused = self.shared.control.is_paused();
        let turbo = self.shared.control.is_turbo();
        let indexing = lock(&self.shared.progress).snapshot(paused);
        indexing.map(|progress| Progress { turbo, ..progress }).or_else(|| {
            let Refining { left, total } = self.shared.refining();
            (left > 0).then(|| Progress {
                name: "Refining index".into(),
                queued: 0,
                counting: false,
                found: total,
                done: total - left,
                total,
                paused,
                updating: false,
                refining: true,
                folders: Vec::new(),
                turbo,
            })
        })
    }

    pub fn set_turbo(&self, turbo: bool) {
        if turbo && !self.shared.is_busy() {
            return;
        }
        self.shared.set_turbo(turbo);
    }

    pub fn indexed_types(&self) -> Categories {
        self.shared.types()
    }

    pub fn set_indexed_types(&self, types: Categories) -> Result<(), String> {
        let before = self.shared.types();
        lock(&self.shared.store)
            .set_indexed_types(types)
            .map_err(|e| format!("Couldn't save the setting. {e}"))?;
        self.shared.indexed_types.store(types.bits(), Ordering::Relaxed);
        let dropped: Vec<browse::Category> = browse::CATEGORIES
            .iter()
            .copied()
            .filter(|&kind| before.contains(kind) && !types.contains(kind))
            .collect();
        if dropped.is_empty() {
            return Ok(());
        }
        let gone: Vec<PathBuf> = self
            .indexed_roots()
            .into_iter()
            .filter(|root| {
                let kind = root
                    .file_name()
                    .and_then(|name| browse::category_of(&name.to_string_lossy()));
                types.is_empty() || (kind.is_some_and(|kind| dropped.contains(&kind)) && !root.is_dir())
            })
            .collect();
        if !gone.is_empty() {
            self.remove_many(&gone)?;
        }
        self.shared.control.poke();
        let mut store = lock(&self.shared.store);
        let removed = store.remove_categories(&dropped);
        self.shared.recount_refining(&store);
        Ok(removed?)
    }

    pub fn count_of_type(&self, kind: browse::Category) -> Result<u64, String> {
        Ok(lock(&self.shared.store).count_of(kind)?)
    }

    pub fn count_indexed_in(&self, path: &Path) -> Result<u64, String> {
        let counts = lock(&self.shared.store).files_under_each(&[path.to_path_buf()])?;
        Ok(counts.into_iter().next().unwrap_or(0))
    }

    pub fn check_for_changes(&self) {
        self.chores().check_everything();
    }

    pub fn check_for_added_types(&self) {
        self.check_for_changes();
        if self.shared.always_full_speed.load(Ordering::SeqCst) {
            self.shared.asked_checks.store(true, Ordering::SeqCst);
            self.shared.set_turbo(true);
        }
    }

    pub fn set_always_full_speed(&self, on: bool) {
        self.shared.always_full_speed.store(on, Ordering::SeqCst);
        if on && lock(&self.shared.progress).has_asked_for() {
            self.shared.set_turbo(true);
        }
    }

    pub fn start_watching(&self) {
        #[cfg(target_os = "linux")]
        {
            use crate::watch::{LOOK_AFTER_OPENING, LOOK_AT_MOST_EVERY, LOOK_EVERY};
            self.start_looking_with(LOOK_EVERY, LOOK_AFTER_OPENING, LOOK_AT_MOST_EVERY);
        }
        #[cfg(not(target_os = "linux"))]
        self.start_watching_with(crate::watch::SETTLE);
    }

    #[cfg_attr(all(target_os = "linux", not(test)), allow(dead_code))]
    pub fn start_watching_with(&self, settle: Duration) {
        *lock(&self.watching) = Some(crate::watch::start(self.chores(), settle));
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn start_looking_with(&self, every: Duration, after_asking: Duration, at_most_every: Duration) {
        *lock(&self.watching) = Some(crate::watch::poll(self.chores(), every, after_asking, at_most_every));
    }

    pub fn look_soon(&self) {
        if let Some(watching) = lock(&self.watching).as_ref() {
            watching.look_soon();
        }
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), String> {
        let store = lock(&self.shared.store);
        if paused && !self.shared.is_busy() {
            return Ok(());
        }
        self.shared.set_paused(&store, paused)?;
        if !paused {
            self.shared.control.more_work();
        }
        Ok(())
    }

    pub fn set_everything_counted(
        &self,
        roots: &[PathBuf],
        counted: impl Fn(&Path) -> Option<Vec<(PathBuf, u64)>>,
    ) -> Result<(), String> {
        let mut store = lock(&self.shared.store);
        let mut rules = lock(&self.shared.rules).clone();
        let roots: Vec<PathBuf> = roots
            .iter()
            .filter(|root| self.is_left_to_index(&rules, root))
            .cloned()
            .collect();
        rules::add(&mut rules, &roots);
        self.replace_rules(&mut store, rules)?;
        self.shared.control.poke();
        self.start_jobs_counted(&store, &roots, counted)
    }

    fn is_left_to_index(&self, rules: &Rules, root: &Path) -> bool {
        !unindexable::is_unindexable(root) && !self.shared.is_excluded(root) && !effective(rules, root)
    }

    pub fn has_everything(&self, roots: &[PathBuf]) -> bool {
        let rules = lock(&self.shared.rules);
        !roots.is_empty() && !roots.iter().any(|root| self.is_left_to_index(&rules, root))
    }

    fn replace_rules(&self, store: &mut Store, rules: Rules) -> Result<(), String> {
        let (gone, new): (Vec<PathBuf>, Vec<PathBuf>) = {
            let current = lock(&self.shared.rules);
            (
                current.difference(&rules).cloned().collect(),
                rules.difference(&current).cloned().collect(),
            )
        };
        if gone.is_empty() && new.is_empty() {
            return Ok(());
        }
        store.save_rules_diff(gone.iter(), new.iter())?;
        *lock(&self.shared.rules) = rules;
        self.lists_changed();
        Ok(())
    }

    fn start_jobs(&self, store: &Store, roots: &[PathBuf]) -> Result<(), String> {
        self.start_jobs_counted(store, roots, |_| None)
    }

    fn start_jobs_counted(
        &self,
        store: &Store,
        roots: &[PathBuf],
        counted: impl Fn(&Path) -> Option<Vec<(PathBuf, u64)>>,
    ) -> Result<(), String> {
        if roots.is_empty() {
            return Ok(());
        }
        let ids = store.add_jobs(roots)?;
        if !lock(&self.shared.progress).is_active() {
            self.shared.set_paused(store, false)?;
        }
        let already = store.files_under_each(roots).unwrap_or_else(|_| vec![0; roots.len()]);
        for ((id, root), already) in ids.into_iter().zip(roots).zip(already) {
            self.queue_job_counted(id, root.clone(), already, counted(root));
        }
        if self.shared.always_full_speed.load(Ordering::SeqCst) {
            self.shared.set_turbo(true);
        }
        Ok(())
    }

    fn queue_job_having_done(&self, id: i64, root: PathBuf, already: u64) {
        self.queue_job_counted(id, root, already, None);
    }

    fn queue_job_counted(&self, id: i64, root: PathBuf, already: u64, counted: Option<Vec<(PathBuf, u64)>>) {
        {
            let mut pending = lock(&self.shared.pending);
            let mut progress = lock(&self.shared.progress);
            pending.add(root.clone());
            let (ticket, _) = progress.add_job_having_done(id, root, already);
            if let Some(folders) = counted {
                progress.counted_already(ticket, &folders);
            }
        }
        self.shared.control.more_work();
        let _ = lock(&self.counter).send(());
    }

    pub(crate) fn chores(&self) -> Chores {
        Chores {
            shared: self.shared.clone(),
        }
    }
}

#[derive(Clone)]
pub struct Chores {
    shared: Arc<Shared>,
}

impl Chores {
    pub fn check_later(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let added = {
            let mut quiet = lock(&self.shared.quiet);
            paths.into_iter().fold(false, |added, path| quiet.add(path) | added)
        };
        if added {
            self.shared.control.more_work();
        }
    }

    pub fn forget(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let gone: Vec<PathBuf> = paths
            .into_iter()
            .filter(|path| self.shared.is_indexed(path) && indexer::is_gone(path))
            .collect();
        if gone.is_empty() {
            return;
        }
        if let Err(e) = lock(&self.shared.store).remove_paths(&gone) {
            eprintln!("Mesa: couldn't forget what has been deleted: {e}");
        }
    }

    pub fn moved(&self, from: &Path, to: &Path) {
        let wanted = self.shared.is_indexed(to);
        let followed = wanted
            && match lock(&self.shared.store).rename_path(from, to) {
                Ok(followed) => followed,
                Err(e) => {
                    eprintln!("Mesa: couldn't follow {} to {}: {e}", from.display(), to.display());
                    false
                }
            };
        if !followed && let Err(e) = lock(&self.shared.store).remove_paths(&[from.to_owned()]) {
            eprintln!("Mesa: couldn't forget {}, which has been moved: {e}", from.display());
        }
        if wanted {
            self.check_later([to.to_owned()]);
        }
    }

    pub fn roots(&self) -> BTreeSet<PathBuf> {
        lock(&self.shared.rules).clone()
    }

    pub fn check_everything(&self) {
        crate::code_scope::forget();
        self.check_later(self.roots());
    }

    pub fn is_indexed(&self, path: &Path) -> bool {
        self.shared.is_indexed(path)
    }
}

impl Drop for Index {
    fn drop(&mut self) {
        self.shared.control.shut_down();
        self.shared.embedder.release();
    }
}

const CHILDREN_TOGETHER: usize = 32;

trait Facts {
    fn unindexable(&self, path: &Path) -> bool;
    fn excluded(&self, path: &Path) -> bool;
    fn excluded_here(&self, path: &Path) -> bool;
    fn types(&self) -> Categories;
    fn indexed(&self, path: &Path) -> bool;
    fn covered(&self, path: &Path) -> bool;
    fn root_below(&self, path: &Path) -> bool;
    fn pending(&self, path: &Path) -> bool;
    fn queued(&self, path: &Path) -> bool;
    fn paused(&self) -> bool;
    fn cloud(&self, path: &Path) -> CloudOnly;
}

fn looks_like_a_name(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    match words.len() {
        0 => false,
        1 => true,
        2..=4 => words
            .iter()
            .any(|word| word.contains(['.', '_', '-']) || word.chars().any(|c| c.is_ascii_digit())),
        _ => false,
    }
}

fn put_named_first(ranked: &mut Ranked, named: &[i64], by_words: &mut HashMap<i64, crate::store::WordsMatch>) {
    let first: HashSet<i64> = named.iter().copied().collect();
    let was_close: HashSet<i64> = ranked.files[..ranked.close].iter().map(|&(file, _)| file).collect();
    let scores: HashMap<i64, f32> = ranked.files.iter().copied().collect();
    let rest: Vec<(i64, f32)> = ranked
        .files
        .iter()
        .copied()
        .filter(|(file, _)| !first.contains(file))
        .collect();
    let still_close = rest.iter().filter(|(file, _)| was_close.contains(file)).count();
    let mut files: Vec<(i64, f32)> = named
        .iter()
        .map(|&file| (file, scores.get(&file).copied().unwrap_or(0.0)))
        .collect();
    for &file in named {
        ranked.reasons.insert(file, crate::relevance::Reason::Words);
        by_words.insert(file, crate::store::WordsMatch::Name);
    }
    ranked.close = files.len() + still_close;
    files.extend(rest);
    ranked.files = files;
}

fn put_first(
    ranked: &mut Ranked,
    clued: &[crate::clues::Found],
    store: &crate::store::Store,
    by_words: &mut HashMap<i64, crate::store::WordsMatch>,
) -> Result<(), String> {
    for found in clued {
        by_words.remove(&found.file);
    }
    let first: HashSet<i64> = clued.iter().map(|found| found.file).collect();
    let was_close: HashSet<i64> = ranked.files[..ranked.close].iter().map(|&(file, _)| file).collect();
    let scores: HashMap<i64, f32> = ranked.files.iter().copied().collect();
    let rest: Vec<(i64, f32)> = ranked
        .files
        .iter()
        .copied()
        .filter(|(file, _)| !first.contains(file))
        .collect();
    let still_close = rest.iter().filter(|(file, _)| was_close.contains(file)).count();
    let mut files: Vec<(i64, f32)> = clued
        .iter()
        .map(|found| (found.file, scores.get(&found.file).copied().unwrap_or(0.0)))
        .collect();
    let paths = store.hits(&files).map_err(|e| e.to_string())?;
    for hit in &paths {
        let Some(found) = clued.iter().find(|found| found.file == hit.file) else {
            continue;
        };
        ranked.reasons.insert(found.file, crate::relevance::Reason::Clues);
        if let Some(line) = crate::clues::best_line(&hit.path, &found.phrases) {
            ranked.at.insert(found.file, line);
        }
    }
    ranked.close = files.len() + still_close;
    files.extend(rest);
    ranked.files = files;
    Ok(())
}

const ALSO_IN: f32 = 0.97;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlsoIn {
    pub path: String,
    pub line: u64,
    pub symbol: String,
    pub similarity: f32,
}

fn status_of(facts: &impl Facts, path: &Path, is_file: Option<bool>) -> IndexStatus {
    if facts.unindexable(path) {
        return IndexStatus::Unindexable;
    }
    if facts.excluded(path) {
        return IndexStatus::Excluded;
    }
    let types = facts.types();
    if types.is_empty() {
        return IndexStatus::TypeNotIndexed;
    }
    let kind = path
        .file_name()
        .and_then(|name| browse::category_of(&name.to_string_lossy()));
    if kind.is_some_and(|kind| !types.contains(kind)) && is_file.unwrap_or_else(|| browse::is_file_like(path)) {
        return IndexStatus::TypeNotIndexed;
    }
    if !facts.indexed(path) {
        return if facts.root_below(path) {
            IndexStatus::PartlyIndexed
        } else {
            IndexStatus::NotIndexed
        };
    }
    if !facts.pending(path) {
        match facts.cloud(path) {
            CloudOnly::Not => IndexStatus::Indexed,
            CloudOnly::Partly => IndexStatus::PartlyCloudOnly,
            CloudOnly::Wholly => IndexStatus::CloudOnly,
        }
    } else if facts.paused() {
        IndexStatus::Paused
    } else {
        IndexStatus::Indexing
    }
}

fn state_of(facts: &impl Facts, path: &Path, is_file: Option<bool>) -> IndexState {
    let status = status_of(facts, path, is_file);
    let queued = facts.queued(path);
    let removable = match status {
        IndexStatus::Indexed
        | IndexStatus::CloudOnly
        | IndexStatus::PartlyCloudOnly
        | IndexStatus::Indexing
        | IndexStatus::Paused => queued || !facts.covered(path),
        IndexStatus::PartlyIndexed => true,
        _ => false,
    };
    IndexState {
        status,
        queued,
        excluded_here: facts.excluded_here(path),
        removable,
    }
}

struct Asked<'a>(&'a Shared);

impl Facts for Asked<'_> {
    fn unindexable(&self, path: &Path) -> bool {
        unindexable::is_unindexable(path)
    }
    fn excluded(&self, path: &Path) -> bool {
        self.0.is_excluded(path)
    }
    fn excluded_here(&self, path: &Path) -> bool {
        lock(&self.0.excluded).contains(path)
    }
    fn types(&self) -> Categories {
        self.0.types()
    }
    fn indexed(&self, path: &Path) -> bool {
        effective(&lock(&self.0.rules), path)
    }
    fn covered(&self, path: &Path) -> bool {
        covered(&lock(&self.0.rules), path)
    }
    fn root_below(&self, path: &Path) -> bool {
        self.0.has_inclusion_below(path)
    }
    fn pending(&self, path: &Path) -> bool {
        lock(&self.0.pending).touches(path)
    }
    fn queued(&self, path: &Path) -> bool {
        lock(&self.0.progress).is_job_root(path)
    }
    fn paused(&self) -> bool {
        self.0.control.is_paused()
    }
    fn cloud(&self, path: &Path) -> CloudOnly {
        if crate::cloud::is_remote(path) {
            return CloudOnly::Wholly;
        }
        let mut store = lock(&self.0.store);
        if !crate::cloud::remote_mounts_below(path).is_empty() {
            return remote_below(&store, path);
        }
        store.cloud_of(path).unwrap_or(CloudOnly::Not)
    }
}

fn remote_below(store: &Store, path: &Path) -> CloudOnly {
    if store.files_below(path).unwrap_or(true) {
        CloudOnly::Partly
    } else {
        CloudOnly::Wholly
    }
}

#[derive(PartialEq)]
struct Children {
    unindexable: bool,
    each_may_be_unindexable: bool,
    excluded_above: bool,
    excluded: HashSet<OsString>,
    types: Categories,
    indexed_above: bool,
    roots: HashSet<OsString>,
    roots_below: HashSet<OsString>,
    pending_above: bool,
    pending: HashSet<OsString>,
    job_roots: HashSet<OsString>,
    paused: bool,
    cloud: HashMap<OsString, CloudOnly>,
}

#[derive(PartialEq)]
pub struct FolderStates(Children);

impl FolderStates {
    pub fn state(&self, path: &Path, is_file: Option<bool>) -> IndexState {
        state_of(&self.0, path, is_file)
    }
}

impl Children {
    fn of(index: &Index, folder: &Path) -> Self {
        let shared = &index.shared;
        let cloud = {
            let mut store = lock(&shared.store);
            let mut cloud: HashMap<OsString, CloudOnly> = store
                .cloud_in(folder)
                .unwrap_or_default()
                .into_iter()
                .map(|(name, cloud)| (OsString::from(name), cloud))
                .collect();
            for mount in crate::cloud::remote_mounts_below(folder) {
                let Some(first) = mount
                    .strip_prefix(folder)
                    .ok()
                    .and_then(|rest| rest.components().next())
                else {
                    continue;
                };
                let child = folder.join(first);
                let state = if child == mount {
                    CloudOnly::Wholly
                } else {
                    remote_below(&store, &child)
                };
                cloud.insert(first.as_os_str().to_owned(), state);
            }
            cloud
        };
        let may_differ = unindexable::children_may_differ(folder);
        let unindexable = unindexable::is_unindexable(folder) && !may_differ;
        let excluded_above = shared.is_excluded(folder);
        let excluded = names_in(inside(&lock(&shared.excluded), folder), folder);
        let (indexed_above, roots, roots_below) = {
            let rules = lock(&shared.rules);
            let excluded = lock(&shared.excluded);
            let is_excluded =
                |path: &Path| !excluded.is_empty() && path.ancestors().any(|above| excluded.contains(above));
            let deeper = inside(&rules, folder)
                .filter(|path| path.parent() != Some(folder))
                .filter(|path| !unindexable::is_unindexable(path) && !is_excluded(path));
            (
                effective(&rules, folder),
                names_in(inside(&rules, folder), folder),
                children_with_something(deeper, folder),
            )
        };
        let (pending_above, pending) = lock(&shared.pending).touching_children(folder);
        let job_roots = lock(&shared.progress).job_root_children(folder);
        Self {
            unindexable,
            each_may_be_unindexable: may_differ,
            excluded_above,
            excluded,
            types: shared.types(),
            indexed_above,
            roots,
            roots_below,
            pending_above,
            pending,
            job_roots,
            paused: shared.control.is_paused(),
            cloud,
        }
    }

    fn name(path: &Path) -> &OsStr {
        path.file_name().expect("an entry of a folder has a name")
    }
}

impl Facts for Children {
    fn unindexable(&self, path: &Path) -> bool {
        self.unindexable || (self.each_may_be_unindexable && unindexable::is_unindexable(path))
    }
    fn excluded(&self, path: &Path) -> bool {
        self.excluded_above || self.excluded.contains(Self::name(path))
    }
    fn excluded_here(&self, path: &Path) -> bool {
        self.excluded.contains(Self::name(path))
    }
    fn types(&self) -> Categories {
        self.types
    }
    fn indexed(&self, path: &Path) -> bool {
        self.indexed_above || self.roots.contains(Self::name(path))
    }
    fn covered(&self, _path: &Path) -> bool {
        self.indexed_above
    }
    fn root_below(&self, path: &Path) -> bool {
        self.roots_below.contains(Self::name(path))
    }
    fn pending(&self, path: &Path) -> bool {
        self.pending_above || self.pending.contains(Self::name(path))
    }
    fn queued(&self, path: &Path) -> bool {
        self.job_roots.contains(Self::name(path))
    }
    fn paused(&self) -> bool {
        self.paused
    }
    fn cloud(&self, path: &Path) -> CloudOnly {
        self.cloud.get(Self::name(path)).copied().unwrap_or(CloudOnly::Not)
    }
}

pub(crate) fn inside<'a>(set: &'a BTreeSet<PathBuf>, folder: &'a Path) -> impl Iterator<Item = &'a PathBuf> + 'a {
    set.range::<Path, _>((Bound::Excluded(folder), Bound::Unbounded))
        .take_while(move |path| path.starts_with(folder))
}

fn names_in<'a>(paths: impl Iterator<Item = &'a PathBuf>, folder: &Path) -> HashSet<OsString> {
    paths
        .filter(|path| path.parent() == Some(folder))
        .filter_map(|path| path.file_name().map(OsStr::to_owned))
        .collect()
}

pub(crate) fn strictly_below<'a, V>(
    map: &'a BTreeMap<PathBuf, V>,
    folder: &'a Path,
) -> impl Iterator<Item = &'a PathBuf> + 'a {
    map.range::<Path, _>((Bound::Excluded(folder), Bound::Unbounded))
        .take_while(move |(path, _)| path.starts_with(folder))
        .map(|(path, _)| path)
}

pub(crate) fn children_with_something<'a>(
    below: impl Iterator<Item = &'a PathBuf>,
    folder: &Path,
) -> HashSet<OsString> {
    below
        .filter_map(|path| {
            Some(
                path.strip_prefix(folder)
                    .ok()?
                    .components()
                    .next()?
                    .as_os_str()
                    .to_owned(),
            )
        })
        .collect()
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

    pub fn touching_children(&self, folder: &Path) -> (bool, HashSet<OsString>) {
        let above = folder.ancestors().any(|above| self.paths.contains_key(above));
        (
            above,
            children_with_something(strictly_below(&self.paths, folder), folder),
        )
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{Duration, Instant},
    };

    use super::*;
    use crate::browse;

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
            Self::with_embedder(tag, embedder())
        }

        fn with_embedder(tag: &str, embedder: Arc<dyn Embedder>) -> Self {
            let (root, photos) = make_photos(tag);
            let data = root.join("data");
            let index = Index::open(&data, embedder).unwrap();
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

    fn every_page(index: &Index, first: Page) -> Vec<Page> {
        let mut pages = vec![first];
        while pages.last().unwrap().more {
            pages.push(index.search_more(pages[0].id).unwrap());
        }
        pages
    }

    fn assert_well_paged(pages: &[Page]) {
        assert!(pages[0].close);
        let parts = pages.iter().position(|page| !page.close).unwrap_or(pages.len());
        assert!(pages[parts..].iter().all(|page| !page.close));
        for (at, page) in pages.iter().enumerate() {
            assert_eq!(
                page.more_close,
                at + 1 < parts,
                "page {at} of {}, {parts} close",
                pages.len()
            );
        }
        for part in [&pages[..parts], &pages[parts..]] {
            assert!(part.iter().all(|page| page.hits.len() <= SEARCH_RESULTS));
            if let Some((_, full)) = part.split_last() {
                assert!(full.iter().all(|page| page.hits.len() == SEARCH_RESULTS));
            }
        }
    }

    #[test]
    fn search_pages_through_results() {
        let f = Fixture::new("search");
        let many = f.photos.join("many");
        fs::create_dir_all(&many).unwrap();
        for n in 0..SEARCH_RESULTS + 10 {
            fs::write(many.join(format!("{n}.jpg")), "x").unwrap();
        }
        f.index.set(&f.photos, true).unwrap();
        f.settle();

        let everything = Filter::default();
        let pages = every_page(&f.index, f.index.search("a dog on a beach", everything).unwrap());
        assert_well_paged(&pages);
        let hits: Vec<_> = pages.iter().flat_map(|page| page.hits.iter()).collect();
        assert_eq!(hits.len(), SEARCH_RESULTS + 10 + 4);
        assert!(hits.iter().all(|hit| hit.path.starts_with(&f.photos)));

        let documents = Filter {
            category: Some(browse::Category::Document),
            ..everything
        };
        let only = f.index.search("a dog on a beach", documents).unwrap().hits;
        assert_eq!(
            only.iter().map(|hit| hit.path.clone()).collect::<Vec<_>>(),
            [f.photos.join("notes.txt")]
        );

        let before_any = Filter {
            to_ms: Some(1),
            ..everything
        };
        assert!(f.index.search("a dog on a beach", before_any).unwrap().hits.is_empty());
        assert!(f.index.search("   ", everything).unwrap().hits.is_empty());
    }

    #[test]
    fn exclude_file_in_indexed_folder() {
        let f = Fixture::new("exclude");
        f.index.set(&f.photos, true).unwrap();
        f.settle();

        f.index.exclude_many(&[f.photos.join("a.jpg")]).unwrap();
        assert_eq!(f.count("files"), 3);
        assert_eq!(f.index.status(&f.photos.join("a.jpg")), IndexStatus::Excluded);
        assert_eq!(f.index.status(&f.photos.join("b.png")), IndexStatus::Indexed);
        assert_eq!(f.index.status(&f.photos), IndexStatus::Indexed);

        f.index.set(&f.photos, true).unwrap();
        f.settle();
        assert_eq!(f.count("files"), 3);

        f.index.stop_excluding_many(&[f.photos.join("a.jpg")]).unwrap();
        f.settle();
        assert_eq!(f.count("files"), 4);
        assert_eq!(f.index.status(&f.photos.join("a.jpg")), IndexStatus::Indexed);
    }

    #[test]
    fn remove_indexed_folder() {
        let f = Fixture::new("remove");
        f.index.set(&f.photos, true).unwrap();
        f.settle();

        f.index.set(&f.photos.join("sub"), false).unwrap();
        assert_eq!(f.count("files"), 4);
        f.index.set(&f.photos, false).unwrap();
        assert_eq!(f.count("files"), 0);
        assert_eq!(f.count("embeddings"), 0);
        assert_eq!(f.index.status(&f.photos), IndexStatus::NotIndexed);
        assert!(f.index.indexed_roots().is_empty());
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
        store.save_rules(&BTreeSet::from([photos.clone()])).unwrap();
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
            words: None,
            pages: Vec::new(),
            words_only: false,
            depth: crate::store::Depth::Full,
        };
        let scan = ScannedDir {
            path: photos.clone(),
            files: vec![a],
            cloud: Vec::new(),
            complete: false,
            subfolders: None,
        };
        store.write_scans(&[scan], &|_| true, 0).unwrap();
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

        f.index.stop_indexing().unwrap();
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
        f.index.exclude_many(std::slice::from_ref(&sub)).unwrap();

        let reopened = Index::open(&f.data, embedder()).unwrap();
        assert_eq!(reopened.status(&sub), IndexStatus::Excluded);
        assert_eq!(reopened.excluded_paths(), vec![sub]);
    }

    #[test]
    fn resumes_interrupted_indexing() {
        let (root, photos) = make_photos("resume");
        let data = root.join("data");
        {
            let mut store = Store::open(&data).unwrap();
            store.save_rules(&BTreeSet::from([photos.clone()])).unwrap();
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

    fn wait_until(what: &str, condition: impl Fn() -> bool) {
        let started = Instant::now();
        while !condition() {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "{what}: it didn't come about"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn forgets_deleted_root_on_start() {
        let f = Fixture::new("check-root-gone");
        f.index.set(&f.photos, true).unwrap();
        f.settle();
        let Fixture {
            data,
            photos,
            index,
            _root,
        } = f;
        drop(index);
        fs::remove_dir_all(&photos).unwrap();

        let index = Index::open(&data, embedder()).unwrap();
        index.check_for_changes();
        let f = Fixture {
            data,
            photos,
            index,
            _root,
        };
        wait_until("the check", || f.count("files") == 0);
        assert_eq!(f.count("embeddings"), 0);
        assert_eq!(f.count("free_slots"), 4);
    }

    #[cfg(windows)]
    fn set_cloud_only(path: &Path, cloud: bool) {
        use windows::{
            Win32::Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_OFFLINE, SetFileAttributesW},
            core::HSTRING,
        };
        let attributes = if cloud {
            FILE_ATTRIBUTE_OFFLINE
        } else {
            FILE_ATTRIBUTE_NORMAL
        };
        unsafe { SetFileAttributesW(&HSTRING::from(path), attributes) }.unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn leaves_cloud_only_files_unread_and_marks_them() {
        let f = Fixture::new("cloud-only");
        let sub = f.photos.join("sub");
        let (a, b, c) = (f.photos.join("a.jpg"), f.photos.join("b.png"), sub.join("c.jpg"));
        set_cloud_only(&a, true);
        set_cloud_only(&c, true);
        f.index.set(&f.photos, true).unwrap();
        f.settle();

        assert_eq!(f.count("files"), 2);
        assert_eq!(f.count("embeddings"), 2);
        assert_eq!(f.count("cloud_files"), 2);
        assert_eq!(f.index.status(&a), IndexStatus::CloudOnly);
        assert_eq!(f.index.status(&b), IndexStatus::Indexed);
        assert_eq!(f.index.status(&sub), IndexStatus::CloudOnly);
        assert_eq!(f.index.status(&f.photos), IndexStatus::PartlyCloudOnly);
        assert!(f.index.state(&f.photos).removable);
        let listed = f.index.folder_states(&f.photos);
        assert_eq!(listed.state(&a, Some(true)).status, IndexStatus::CloudOnly);
        assert_eq!(listed.state(&b, Some(true)).status, IndexStatus::Indexed);
        assert_eq!(listed.state(&sub, Some(false)).status, IndexStatus::CloudOnly);

        set_cloud_only(&a, false);
        f.index.check_for_changes();
        wait_until("a.jpg is read once downloaded", || {
            f.index.status(&a) == IndexStatus::Indexed && f.count("files") == 3
        });
        assert_eq!(f.count("cloud_files"), 1);

        set_cloud_only(&b, true);
        f.index.chores().check_later([b.clone()]);
        wait_until("b.png is forgotten once freed up", || {
            f.index.status(&b) == IndexStatus::CloudOnly && f.count("files") == 2
        });

        set_cloud_only(&c, false);
        f.index.chores().check_later([c.clone()]);
        wait_until("c.jpg is read once downloaded", || {
            f.index.status(&sub) == IndexStatus::Indexed
        });
        assert_eq!(f.index.status(&f.photos), IndexStatus::PartlyCloudOnly);

        f.index.set(&f.photos, false).unwrap();
        f.settle();
        assert_eq!(f.count("cloud_files"), 0);
        assert_eq!(f.index.status(&b), IndexStatus::NotIndexed);
    }
}
