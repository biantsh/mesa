use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    ffi::{OsStr, OsString},
    ops::Bound,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicU8, AtomicU64, Ordering},
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
    store::{self, Counts, Filter, Hit, Store},
    unindexable,
};

pub const SEARCH_RESULTS: usize = 24;

struct Ranking {
    id: u64,
    ranked: Vec<(i64, f32)>,
    close: usize,
    excluded: Vec<String>,
    next: Mutex<usize>,
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

    pub fn set_turbo(&self, turbo: bool) {
        if self.control.is_turbo() != turbo {
            self.embedder.set_eco(!turbo);
            self.control.set_turbo(turbo);
        }
    }

    pub fn is_busy(&self) -> bool {
        lock(&self.progress).is_active()
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
        let store = Store::open(dir)?;
        let rules = store.load_rules()?;
        let types = match store.indexed_types()? {
            Some(kept) => kept,
            None => {
                let types = chosen.unwrap_or(if rules.is_empty() {
                    Categories::of(&DEFAULT_TYPES)
                } else {
                    Categories::ALL
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

    pub fn exclusion_count(&self) -> usize {
        lock(&self.shared.excluded).len()
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
        self.search_where(window, text, files, filter, |path| path.is_file())
    }

    pub fn prepare_example(&self, path: &Path) -> Result<(), String> {
        self.examples
            .vectors(&*self.shared.embedder, path, crate::examples::EAGER_PIECES)
            .map(|_| ())
    }

    fn query_vector(&self, text: &str, files: &[PathBuf]) -> Result<Option<crate::vectors::Record>, String> {
        let mut parts = Vec::new();
        if !text.trim().is_empty() {
            parts.push((self.shared.embedder.embed_query(text).map_err(|e| e.to_string())?, 1.0));
        }
        if files.len() > QUERY_PIECES {
            return Err(format!("At most {QUERY_PIECES} files can be searched with at once."));
        }
        let mut made = Vec::new();
        let mut left = QUERY_PIECES;
        for (n, file) in files.iter().enumerate() {
            let share = (left / (files.len() - n)).max(1);
            let vectors = self.examples.vectors(&*self.shared.embedder, file, share)?;
            left = left.saturating_sub(vectors.len());
            let weighted: Vec<_> = vectors.into_iter().map(|record| (record, 1.0)).collect();
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
        let ranked = if let Some(query) = query {
            lock(&self.shared.store).rank(&query, filter)?
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
            next: Mutex::new(0),
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

    pub fn search_more(&self, id: u64) -> Result<Page, String> {
        let ranking = lock(&self.latest_search)
            .values()
            .find(|ranking| ranking.id == id)
            .cloned();
        let ranking = ranking.ok_or_else(|| "That search has been replaced by a newer one.".to_owned())?;
        self.next_page(&ranking, |path| path.is_file())
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
            for hit in hits {
                if !exists(&hit.path)
                    || (!ranking.excluded.is_empty() && ranking.excluded.contains(&path_key(&hit.path)))
                {
                    continue;
                }
                let file = hit.file;
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

    pub fn gpu(&self) -> Option<String> {
        self.shared.embedder.gpu()
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
        lock(&self.shared.progress)
            .snapshot(paused)
            .map(|progress| Progress { turbo, ..progress })
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
        let removed = lock(&self.shared.store).remove_categories(&dropped);
        Ok(removed?)
    }

    pub fn count_of_type(&self, kind: browse::Category) -> Result<u64, String> {
        Ok(lock(&self.shared.store).count_of(kind)?)
    }

    pub fn check_for_changes(&self) {
        let chores = self.chores();
        chores.check_later(chores.roots());
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
        if paused && !lock(&self.shared.progress).is_active() {
            return Ok(());
        }
        Ok(self.shared.set_paused(&store, paused)?)
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

    fn chores(&self) -> Chores {
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
    if kind.is_some_and(|kind| !types.contains(kind)) && is_file.unwrap_or_else(|| path.is_file()) {
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
        IndexStatus::Indexed
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
        IndexStatus::Indexed | IndexStatus::Indexing | IndexStatus::Paused => queued || !facts.covered(path),
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
        let unindexable = unindexable::is_unindexable(folder);
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
            each_may_be_unindexable: !unindexable && unindexable::children_may_differ(folder),
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
        };
        let scan = ScannedDir {
            path: photos.clone(),
            files: vec![a],
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
}
