use std::{
    collections::{HashMap, HashSet},
    fs, io, mem,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::Receiver,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossbeam_channel::bounded;

use crate::{
    browse::{self, Categories, Category, Shown},
    crawl::crawl,
    embed::{Item, Outcome},
    index::{Job, Shared, lock},
    power,
    store::{ScannedDir, ScannedFile},
    unindexable,
};

const BATCH_FILES: usize = 2000;
const BATCH_AGE: Duration = Duration::from_millis(250);
const CHUNK_FILES: usize = 512;
const RUN_MOST: usize = 512;
const LISTED_AHEAD: usize = 1024;

pub fn work(shared: &Shared) {
    loop {
        power::follow(shared.is_gentle());
        let seen = shared.control.work_seen();
        if shared.control.is_shut() {
            break;
        }
        let run = take_next(shared);
        if !run.is_empty() {
            power::keep_pace(true);
            do_run(shared, run);
            continue;
        }
        if !shared.is_busy() {
            let asked =
                shared.asked_checks.load(std::sync::atomic::Ordering::SeqCst) && !lock(&shared.quiet).is_empty();
            if !asked {
                shared.asked_checks.store(false, std::sync::atomic::Ordering::SeqCst);
                shared.set_turbo(false);
                power::follow(true);
            }
        }
        let next = lock(&shared.quiet).next();
        if let Some(path) = next {
            power::keep_pace(true);
            check(shared, &path);
            continue;
        }
        power::keep_pace(false);
        if shared.control.wait_for_work(seen) {
            break;
        }
    }
}

fn take_next(shared: &Shared) -> Vec<Job> {
    let upcoming = lock(&shared.progress).upcoming(RUN_MOST);
    let mut run: Vec<Job> = Vec::new();
    for (ticket, id, root) in upcoming {
        let is_dir = browse::is_folder(&root);
        if is_dir && !run.is_empty() {
            break;
        }
        run.push(Job { id, root, ticket });
        if is_dir {
            break;
        }
    }
    let tickets: Vec<u64> = run.iter().map(|job| job.ticket).collect();
    let begun: HashSet<u64> = lock(&shared.progress).begin_together(&tickets).into_iter().collect();
    run.retain(|job| begun.contains(&job.ticket));
    run
}

fn do_run(shared: &Shared, mut run: Vec<Job>) {
    if run.len() == 1 {
        work_on(shared, run.remove(0));
    } else {
        walk_files(shared, run);
    }
}

fn work_on(shared: &Shared, job: Job) {
    shared.wait_while_paused_for(Some(job.ticket));
    await_counts(shared, job.ticket);
    let stored = walk(shared, job.ticket, &job.root);
    let wanted = lock(&shared.progress).finish_job(job.ticket);

    let store = lock(&shared.store);
    if wanted
        && stored
        && let Err(e) = store.finish_job(job.id)
    {
        eprintln!("Mesa: couldn't mark indexing of {} as done: {e}", job.root.display());
    }

    if shared.control.is_paused() && !lock(&shared.progress).is_active() {
        let _ = shared.set_paused(&store, false);
    }
    drop(store);
}

fn walk_files(shared: &Shared, mine: Vec<Job>) {
    let Some(first) = mine.first() else { return };
    shared.wait_while_paused_for(Some(first.ticket));
    for job in &mine {
        await_counts(shared, job.ticket);
    }

    let folder = first.root.parent().unwrap_or(&first.root).to_owned();
    let mut walker = Walker::new(shared, Tracking::Run, &folder);
    for job in &mine {
        walker
            .batch
            .owners
            .entry(job.root.clone())
            .or_default()
            .push((job.ticket, job.id));
    }
    for job in &mine {
        power::follow(shared.is_gentle());
        walker.stored &= walker.batch.pause_point(shared);
        match scan_file(&job.root, shared.types()).filter(|_| shared.is_indexed(&job.root)) {
            Some(scan) => walker.add_root(scan, &job.root),
            None => walker.release(&job.root),
        }
        if walker.waiting() >= CHUNK_FILES {
            walker.embed_waiting();
        }
        if walker.batch.due() {
            walker.stored &= walker.batch.store(shared);
        }
    }
    walker.embed_waiting();
    let stored = walker.stored & walker.batch.store(shared);
    walker.batch.finish_left(shared, stored);
    drop(walker);
    end_pause_if_idle(shared);
}

fn end_pause_if_idle(shared: &Shared) {
    if shared.control.is_paused() && !lock(&shared.progress).is_active() {
        let store = lock(&shared.store);
        let _ = shared.set_paused(&store, false);
    }
}

fn check(shared: &Shared, path: &Path) {
    if is_gone(path) {
        if let Err(e) = lock(&shared.store).remove_paths(&[path.to_owned()]) {
            eprintln!("Mesa: couldn't forget {}, which has gone: {e}", path.display());
        }
        return;
    }
    shared.control.wait_while_paused();
    if !shared.is_indexed(path) {
        return;
    }
    walk_tree(shared, Tracking::Unseen, path);
    if lock(&shared.progress).has_waiting() {
        lock(&shared.quiet).again(path.to_owned());
    }
    if shared.control.is_paused() && !lock(&shared.progress).is_active() {
        let store = lock(&shared.store);
        let _ = shared.set_paused(&store, false);
    }
}

pub fn is_gone(path: &Path) -> bool {
    let missing = matches!(fs::symlink_metadata(path), Err(e) if e.kind() == io::ErrorKind::NotFound);
    missing
        && path
            .ancestors()
            .skip(1)
            .any(|above| fs::symlink_metadata(above).is_ok())
}

const COUNT_POLL: Duration = Duration::from_millis(20);

fn await_counts(shared: &Shared, ticket: u64) {
    loop {
        let ready = lock(&shared.progress).ready(ticket);
        match ready {
            Some(true) => return lock(&shared.progress).release(),
            None => return,
            Some(false) => {
                let next = lock(&shared.progress).next_to_count();
                match next {
                    Some((ticket, root)) => count_files(shared, ticket, &root),
                    None => {
                        shared.wait_while_paused_for(Some(ticket));
                        thread::sleep(COUNT_POLL);
                    }
                }
            }
        }
    }
}

pub fn count_ahead(shared: &Shared, wake: Receiver<()>) {
    while wake.recv().is_ok() {
        loop {
            let next = lock(&shared.progress).next_to_count();
            let Some((ticket, root)) = next else { break };
            count_files(shared, ticket, &root);
        }
    }
}

fn count_files(shared: &Shared, ticket: u64, root: &Path) {
    let found_in = |folder: &Path, files: u64| {
        let mut progress = lock(&shared.progress);
        if !shared.is_excluded(folder) {
            progress.found_in(ticket, folder, files);
        }
    };
    let found = supported_files(
        shared,
        root,
        |path| shared.is_indexed(path),
        || lock(&shared.progress).has(ticket),
        found_in,
    );
    if found > 0 && browse::is_file_like(root) {
        found_in(root, found);
    }
    lock(&shared.progress).finish_counting(ticket);
}

#[cfg(test)]
fn count_job(shared: &Shared, ticket: u64, root: &Path) {
    count_files(shared, ticket, root);
}

fn supported_files(
    shared: &Shared,
    root: &Path,
    counted: impl Fn(&Path) -> bool + Sync,
    go_on: impl Fn() -> bool + Sync,
    found: impl Fn(&Path, u64) + Sync,
) -> u64 {
    if browse::is_folder(root) {
        let total = AtomicU64::new(0);
        crawl(root.to_owned(), shared.crawl_threads(), shared.is_gentle(), |dir| {
            if !counted(&dir) || !go_on() {
                return Vec::new();
            }
            let mut subdirs = Vec::new();
            let mut files = 0;
            each_entry(&dir, shared.types(), |item, _, shown, cloud| match shown {
                Shown::Folder => subdirs.push(item.path()),
                Shown::File(_) if !cloud => files += 1,
                Shown::File(_) => {}
            });
            if files > 0 {
                total.fetch_add(files, Ordering::Relaxed);
                found(&dir, files);
            }
            subdirs
        });
        total.into_inner()
    } else if browse::is_file_like(root)
        && counted(root)
        && !crate::cloud::is_cloud_only_path(root)
        && root
            .file_name()
            .and_then(|name| browse::category_of(&name.to_string_lossy()))
            .is_some_and(|category| {
                shared.types().contains(category) && (category != Category::Code || crate::code_scope::allows(root))
            })
    {
        1
    } else {
        0
    }
}

fn walk(shared: &Shared, ticket: u64, root: &Path) -> bool {
    walk_tree(shared, Tracking::Job(ticket), root)
}

fn walk_tree(shared: &Shared, tracking: Tracking, root: &Path) -> bool {
    let in_progress = tracking.is_in_progress();
    let mut walker = Walker::new(shared, tracking, root);

    if browse::is_folder(root) {
        let (listed, found) = bounded::<ScannedDir>(LISTED_AHEAD);
        thread::scope(|scope| {
            scope.spawn(move || {
                crawl(root.to_owned(), shared.crawl_threads(), shared.is_gentle(), |dir| {
                    shared.wait_while_paused_for(tracking.ticket());
                    let dropped = matches!(tracking, Tracking::Job(ticket) if !lock(&shared.progress).has(ticket));

                    let found = if !dropped
                        && shared.is_indexed(&dir)
                        && (in_progress || !lock(&shared.progress).has_waiting())
                    {
                        scan_dir(&dir, shared.types())
                    } else {
                        None
                    };

                    if in_progress {
                        let mut pending = lock(&shared.pending);
                        if let Some(found) = &found {
                            found.subdirs.iter().for_each(|sub| pending.add(sub.clone()));
                            found
                                .scan
                                .files
                                .iter()
                                .for_each(|file| pending.add(dir.join(&file.name)));
                        }
                        pending.release(&dir);
                    }

                    let Some(Found { scan, subdirs }) = found else {
                        return Vec::new();
                    };
                    let _ = listed.send(scan);
                    subdirs
                });
            });

            for scan in found {
                power::follow(shared.is_gentle());
                walker.stored &= walker.batch.pause_point(shared);
                walker.add(scan);
                if walker.waiting() >= CHUNK_FILES {
                    walker.embed_waiting();
                }
                if walker.batch.due() {
                    walker.stored &= walker.batch.store(shared);
                }
            }
        });
    } else {
        match scan_file(root, shared.types()).filter(|_| shared.is_indexed(root)) {
            Some(scan) => walker.add_root(scan, root),
            None => walker.release(root),
        }
    }
    walker.embed_waiting();
    let stored = walker.stored & walker.batch.store(shared);
    walker.batch.release_marked(shared);
    if let Tracking::Seen(ticket) = walker.batch.tracking {
        lock(&shared.progress).finish_job(ticket);
    }
    stored
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tracking {
    Job(u64),
    Unseen,
    Seen(u64),
    Run,
}

impl Tracking {
    fn is_in_progress(self) -> bool {
        matches!(self, Tracking::Job(_) | Tracking::Run)
    }

    fn ticket(self) -> Option<u64> {
        match self {
            Tracking::Job(ticket) | Tracking::Seen(ticket) => Some(ticket),
            Tracking::Unseen | Tracking::Run => None,
        }
    }
}

struct Walker<'a> {
    shared: &'a Shared,
    batch: Batch,
    folders: Vec<ScannedDir>,
    waiting: Vec<(usize, usize)>,
    failed: bool,
    stored: bool,
    warmed_up: bool,
    new_files: HashSet<PathBuf>,
    rereads: HashSet<PathBuf>,
    root: PathBuf,
}

impl<'a> Walker<'a> {
    fn new(shared: &'a Shared, tracking: Tracking, root: &Path) -> Self {
        Self {
            shared,
            root: root.to_owned(),
            batch: Batch::new(tracking),
            folders: Vec::new(),
            waiting: Vec::new(),
            failed: false,
            stored: true,
            warmed_up: false,
            new_files: HashSet::new(),
            rereads: HashSet::new(),
        }
    }

    fn waiting(&self) -> usize {
        self.waiting.len()
    }

    fn show(&mut self, files: usize) {
        let mut progress = lock(&self.shared.progress);
        let ticket = match self.batch.tracking {
            Tracking::Seen(ticket) => ticket,
            Tracking::Unseen => progress.add_quiet(self.root.clone()),
            Tracking::Job(_) | Tracking::Run => return,
        };
        self.batch.tracking = Tracking::Seen(ticket);
        progress.add_total(ticket, files as u64);
    }

    fn release(&mut self, path: &Path) {
        self.batch.release(self.shared, path);
    }

    fn fail(&mut self) {
        self.failed = true;
        self.stored = false;
    }

    fn add_root(&mut self, scan: ScannedDir, root: &Path) {
        let local = !scan.files.is_empty();
        self.add(scan);
        if !local {
            self.release(root);
        }
    }

    fn add(&mut self, scan: ScannedDir) {
        if scan.files.is_empty() {
            if scan.complete || !scan.cloud.is_empty() {
                self.batch.scans.push(scan);
            }
            return;
        }
        let changes = lock(&self.shared.store).changes(&scan).ok();
        let index = self.folders.len();
        let waiting_before = self.waiting.len();
        let mut up_to_date = 0;
        for (n, file) in scan.files.iter().enumerate() {
            if changes
                .as_ref()
                .is_some_and(|changes| !changes.changed.contains(&file.name))
            {
                let path = scan.path.join(&file.name);
                if self.batch.tracking == Tracking::Run {
                    self.batch.processed_file(self.shared, &path, &scan.path);
                }
                self.batch.finished.push(path);
                up_to_date += 1;
            } else {
                if changes.as_ref().is_some_and(|changes| changes.new.contains(&file.name)) {
                    self.new_files.insert(scan.path.join(&file.name));
                }
                if changes
                    .as_ref()
                    .is_some_and(|changes| changes.reread.contains(&file.name))
                {
                    self.rereads.insert(scan.path.join(&file.name));
                }
                self.waiting.push((index, n));
            }
        }
        if up_to_date > 0 {
            self.batch.files += up_to_date;
            if let Tracking::Job(ticket) = self.batch.tracking {
                lock(&self.shared.progress).processed_in(ticket, up_to_date as u64, &scan.path);
            }
        }
        if !self.batch.tracking.is_in_progress() && self.waiting.len() > waiting_before {
            self.show(self.waiting.len() - waiting_before);
            let mut pending = lock(&self.shared.pending);
            for &(_, n) in &self.waiting[waiting_before..] {
                let path = scan.path.join(&scan.files[n].name);
                pending.add(path.clone());
                self.batch.marked.insert(path);
            }
        }
        if !self.warmed_up && !self.waiting.is_empty() {
            self.warmed_up = true;
            self.shared.embedder.warm_up();
        }
        if self.waiting.last().is_some_and(|&(last, _)| last == index) {
            self.folders.push(scan);
        } else if scan.complete {
            self.batch.scans.push(scan);
        } else if let Some(cloud) = scan.cloud_part() {
            self.batch.scans.push(cloud);
        }
    }

    fn embed_waiting(&mut self) {
        let shared = self.shared;
        let mut folders = mem::take(&mut self.folders);
        let places = mem::take(&mut self.waiting);
        let mut left: Vec<usize> = (0..places.len()).collect();
        let mut retried = false;

        let path_of = |folders: &[ScannedDir], (folder, file): (usize, usize)| {
            folders[folder].path.join(&folders[folder].files[file].name)
        };

        while !left.is_empty() {
            left.retain(|&n| {
                let path = path_of(&folders, places[n]);
                let wanted = shared.wants(&path);
                if !wanted {
                    self.release(&path);
                }
                wanted
            });
            if left.is_empty() {
                break;
            }
            if self.failed {
                for &n in &left {
                    let path = path_of(&folders, places[n]);
                    self.release(&path);
                    self.batch.processed_file(shared, &path, &folders[places[n].0].path);
                }
                break;
            }
            self.stored &= self.batch.pause_point(shared);

            let epoch = shared.control.epoch();
            let items: Vec<Item> = left
                .iter()
                .map(|&n| {
                    let path = path_of(&folders, places[n]);
                    let words_only = self.rereads.contains(&path);
                    Item { path, words_only }
                })
                .collect();
            let mut finished = vec![false; items.len()];
            let batch = &mut self.batch;
            let stored = &mut self.stored;
            let new_files = &mut self.new_files;

            let result = shared
                .embedder
                .embed(&items, &|| shared.control.changed_since(epoch), &mut |i, outcome| {
                    let (folder, file) = places[left[i]];
                    let mut file = folders[folder].files[file].clone();
                    match outcome {
                        Outcome::Vectors(vectors, words, pages) => {
                            (file.vectors, file.words, file.pages) = (vectors, words, pages)
                        }
                        Outcome::NoVector(_) => file.vectors = Vec::new(),
                        Outcome::Words(words, pages) => {
                            (file.words, file.pages, file.words_only) = (words, pages, true)
                        }
                    }
                    if new_files.remove(&items[i].path) {
                        batch.new += 1;
                        shared.counts.made();
                    }
                    batch.finished.push(items[i].path.clone());
                    batch.add_file(&folders[folder].path, file);
                    batch.files += 1;
                    batch.processed_file(shared, &items[i].path, &folders[folder].path);
                    finished[i] = true;
                    if batch.due() {
                        *stored &= batch.store(shared);
                    }
                });

            let made_progress = finished.iter().any(|&done| done);
            match result {
                Ok(()) if !made_progress && !shared.control.changed_since(epoch) => {
                    eprintln!("Mesa: the embedder returned without embedding anything");
                    self.fail();
                }
                Ok(()) => {}
                Err(e) if made_progress && !retried => {
                    eprintln!("Mesa: embedding was interrupted, trying again: {e}");
                    retried = true;
                }
                Err(e) => {
                    eprintln!("Mesa: can't embed files, so they are left for later: {e}");
                    self.fail();
                }
            }
            let mut finished = finished.into_iter();
            left.retain(|_| !finished.next().unwrap_or(false));
        }

        self.new_files.clear();
        self.rereads.clear();

        if !self.failed {
            for folder in folders.drain(..) {
                if folder.complete {
                    self.batch.files += folder.files.len();
                    self.batch.scans.push(folder);
                } else if let Some(cloud) = folder.cloud_part() {
                    self.batch.scans.push(cloud);
                }
            }
        }
    }
}

struct Batch {
    scans: Vec<ScannedDir>,
    finished: Vec<PathBuf>,
    files: usize,
    new: u64,
    started: Instant,
    tracking: Tracking,
    marked: HashSet<PathBuf>,
    owners: HashMap<PathBuf, Vec<(u64, i64)>>,
}

impl Batch {
    fn new(tracking: Tracking) -> Self {
        Self {
            scans: Vec::new(),
            finished: Vec::new(),
            files: 0,
            new: 0,
            started: Instant::now(),
            tracking,
            marked: HashSet::new(),
            owners: HashMap::new(),
        }
    }

    fn add_file(&mut self, folder: &Path, file: ScannedFile) {
        if let Some(last) = self.scans.last_mut()
            && !last.complete
            && last.subfolders.is_none()
            && last.path == folder
            && !last.files.iter().any(|kept| kept.name == file.name)
        {
            last.files.push(file);
            return;
        }
        self.scans.push(ScannedDir {
            path: folder.to_owned(),
            files: vec![file],
            cloud: Vec::new(),
            complete: false,
            subfolders: None,
        });
    }

    fn release(&mut self, shared: &Shared, path: &Path) {
        if self.tracking.is_in_progress() || self.marked.remove(path) {
            lock(&shared.pending).release(path);
        }
    }

    fn release_marked(&mut self, shared: &Shared) {
        let mut pending = lock(&shared.pending);
        self.marked.drain().for_each(|path| pending.release(&path));
    }

    fn processed_file(&self, shared: &Shared, file: &Path, folder: &Path) {
        if self.tracking != Tracking::Run {
            return self.processed_in(shared, 1, folder);
        }
        if let Some(&(ticket, _)) = self.owners.get(file).and_then(|jobs| jobs.first()) {
            lock(&shared.progress).processed_in(ticket, 1, file);
        }
    }

    fn finish(shared: &Shared, jobs: Vec<(u64, i64)>, stored: bool) {
        if jobs.is_empty() {
            return;
        }
        let ids: Vec<i64> = {
            let mut progress = lock(&shared.progress);
            jobs.into_iter()
                .filter(|&(ticket, _)| progress.finish_job(ticket) && stored)
                .map(|(_, id)| id)
                .collect()
        };
        if !ids.is_empty()
            && let Err(e) = lock(&shared.store).finish_jobs(&ids)
        {
            eprintln!("Mesa: couldn't mark indexing as done: {e}");
        }
    }

    fn finish_left(&mut self, shared: &Shared, stored: bool) {
        let left: Vec<(u64, i64)> = self.owners.drain().flat_map(|(_, jobs)| jobs).collect();
        Self::finish(shared, left, stored);
    }

    fn processed_in(&self, shared: &Shared, files: u64, folder: &Path) {
        if let Some(ticket) = self.tracking.ticket() {
            lock(&shared.progress).processed_in(ticket, files, folder);
        }
    }

    fn pause_point(&mut self, shared: &Shared) -> bool {
        if !shared.control.is_paused() {
            return true;
        }
        let stored = self.store(shared);
        shared.wait_while_paused_for(self.tracking.ticket());
        stored
    }

    fn due(&self) -> bool {
        (!self.scans.is_empty() || !self.finished.is_empty())
            && (self.files >= BATCH_FILES || self.started.elapsed() >= BATCH_AGE)
    }

    fn store(&mut self, shared: &Shared) -> bool {
        let mut stored = true;
        if !self.scans.is_empty() {
            let result =
                lock(&shared.store).write_scans(&self.scans, &|path| shared.wants(path), mem::take(&mut self.new));
            if let Err(e) = result {
                eprintln!("Mesa: couldn't store indexed files: {e}");
                stored = false;
            }
        }
        let mut pending = lock(&shared.pending);
        let mut done = Vec::new();
        for path in self.finished.drain(..) {
            if self.tracking.is_in_progress() || self.marked.remove(&path) {
                pending.release(&path);
            }
            if let Some(jobs) = self.owners.remove(&path) {
                done.extend(jobs);
            }
        }
        drop(pending);
        Self::finish(shared, done, stored);
        self.scans.clear();
        self.files = 0;
        self.started = Instant::now();
        stored
    }
}

impl Drop for Walker<'_> {
    fn drop(&mut self) {
        self.shared.counts.not_stored(mem::take(&mut self.batch.new));
    }
}

struct Found {
    scan: ScannedDir,
    subdirs: Vec<PathBuf>,
}

fn each_entry(
    dir: &Path,
    types: Categories,
    mut visit: impl FnMut(&fs::DirEntry, String, Shown, bool),
) -> Option<bool> {
    let mut complete = true;
    let mut own_code: Option<bool> = None;
    let photos_library = unindexable::is_photos_library(dir);
    let remote = crate::cloud::is_remote(dir);
    let locked = unindexable::is_unindexable(dir);
    for item in fs::read_dir(dir).ok()? {
        let Ok(item) = item else {
            complete = false;
            continue;
        };
        if photos_library && !unindexable::is_photos_originals(&item.file_name().to_string_lossy()) {
            continue;
        }
        let Some((name, shown)) = browse::classify(&item) else {
            continue;
        };
        if (locked || (matches!(shown, Shown::Folder) && unindexable::could_be_unindexable(&name)))
            && unindexable::is_wholly_unindexable(&item.path())
        {
            continue;
        }
        if matches!(shown, Shown::File(category) if !types.contains(category)) {
            continue;
        }
        if matches!(shown, Shown::File(Category::Code)) {
            let own = *own_code.get_or_insert_with(|| crate::code_scope::allows_folder(dir));
            if !own || crate::code_scope::is_ignored(&item.path()) {
                continue;
            }
        }
        if matches!(shown, Shown::Folder)
            && (item.file_type().map_or(true, |kind| kind.is_symlink())
                || crate::code_scope::is_others_folder(&item.path()))
        {
            continue;
        }
        let cloud = matches!(shown, Shown::File(_))
            && (remote || metadata_of(&item).is_some_and(|metadata| crate::cloud::is_cloud_only(&metadata)));
        visit(&item, name, shown, cloud);
    }
    Some(complete)
}

pub fn survey_folder(dir: &Path, mut file: impl FnMut(Category, PathBuf, u64)) -> Option<Vec<PathBuf>> {
    let mut subdirs = Vec::new();
    each_entry(dir, Categories::ALL, |item, _, shown, cloud| match shown {
        Shown::Folder => subdirs.push(item.path()),
        Shown::File(category) if !cloud => file(category, item.path(), facts(item).map_or(0, |(_, size)| size)),
        Shown::File(_) => {}
    })?;
    Some(subdirs)
}

fn scan_dir(dir: &Path, types: Categories) -> Option<Found> {
    let mut files = Vec::new();
    let mut cloud = Vec::new();
    let mut subdirs = Vec::new();
    let mut unreadable_file = false;
    let all_listed = each_entry(dir, types, |item, name, shown, in_cloud| match shown {
        Shown::Folder => subdirs.push(item.path()),
        Shown::File(category) if in_cloud => cloud.push((name, category)),
        Shown::File(category) => match facts(item) {
            Some((modified_ms, size)) => files.push(ScannedFile {
                name,
                category,
                modified_ms,
                size,
                vectors: Vec::new(),
                words: None,
                pages: Vec::new(),
                words_only: false,
            }),
            None => unreadable_file = true,
        },
    })?;
    let complete = all_listed && !unreadable_file;
    let subfolders = complete.then(|| {
        subdirs
            .iter()
            .filter_map(|sub| sub.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect()
    });
    Some(Found {
        scan: ScannedDir {
            path: dir.to_owned(),
            files,
            cloud,
            complete,
            subfolders,
        },
        subdirs,
    })
}

fn scan_file(path: &Path, types: Categories) -> Option<ScannedDir> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let category = browse::category_of_path(path, &name).filter(|&category| types.contains(category))?;
    if crate::code_scope::in_others(path) || (category == Category::Code && !crate::code_scope::allows(path)) {
        return None;
    }
    if crate::cloud::is_cloud_only_path(path) {
        return Some(ScannedDir {
            path: path.parent()?.to_owned(),
            files: Vec::new(),
            cloud: vec![(name, category)],
            complete: false,
            subfolders: None,
        });
    }
    let (modified_ms, size) = facts_of_path(path)?;
    Some(ScannedDir {
        path: path.parent()?.to_owned(),
        files: vec![ScannedFile {
            name,
            category,
            modified_ms,
            size,
            vectors: Vec::new(),
            words: None,
            pages: Vec::new(),
            words_only: false,
        }],
        cloud: Vec::new(),
        complete: false,
        subfolders: None,
    })
}

fn metadata_of(item: &fs::DirEntry) -> Option<fs::Metadata> {
    if item.file_type().ok()?.is_symlink() {
        fs::metadata(item.path()).ok()
    } else {
        item.metadata().ok()
    }
}

pub fn facts(item: &fs::DirEntry) -> Option<(i64, u64)> {
    let metadata = metadata_of(item)?;
    if metadata.is_dir() && browse::is_package(&item.path()) {
        return package_facts(&item.path());
    }
    facts_of(&metadata)
}

pub fn package_facts(path: &Path) -> Option<(i64, u64)> {
    let (size, newest) = browse::package_state(path)?;
    Some((unix_millis(newest), size))
}

fn facts_of_path(path: &Path) -> Option<(i64, u64)> {
    if browse::is_package(path) {
        package_facts(path)
    } else {
        facts_of(&fs::metadata(path).ok()?)
    }
}

pub fn facts_of(metadata: &fs::Metadata) -> Option<(i64, u64)> {
    Some((unix_millis(metadata.modified().ok()?), metadata.len()))
}

fn unix_millis(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_millis() as i64,
        Err(before) => -(before.duration().as_millis() as i64),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeSet, VecDeque},
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
    };

    use super::*;

    use crate::{
        embed::{Embedder, Error},
        index::{Control, Pending},
        progress::ProgressState,
        store::Store,
        vectors::{self, Located},
    };

    #[test]
    fn times_before_1970() {
        assert_eq!(unix_millis(UNIX_EPOCH + Duration::from_millis(1500)), 1500);
        assert_eq!(unix_millis(UNIX_EPOCH - Duration::from_millis(1500)), -1500);
    }

    #[derive(Default)]
    struct Fake {
        delay: Duration,
        calls: AtomicUsize,
        seen: Mutex<Vec<PathBuf>>,
        always_fails: bool,
        fails_after: Mutex<VecDeque<usize>>,
        warm_ups: AtomicUsize,
        releases: AtomicUsize,
    }

    impl Embedder for Fake {
        fn embed(
            &self,
            items: &[Item],
            stop: &(dyn Fn() -> bool + Sync),
            done: &mut dyn FnMut(usize, Outcome),
        ) -> Result<(), Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.always_fails {
                return Err(Error("no model".into()));
            }
            let fails_after = lock(&self.fails_after).pop_front();
            for (i, item) in items.iter().enumerate() {
                if fails_after.is_some_and(|n| i >= n) {
                    return Err(Error("the model fell over".into()));
                }
                let mut waited = Duration::ZERO;
                while waited < self.delay && !stop() {
                    thread::sleep(Duration::from_millis(5));
                    waited += Duration::from_millis(5);
                }
                if stop() {
                    return Ok(());
                }
                lock(&self.seen).push(item.path.clone());
                let unreadable = item.path.file_name().unwrap().to_string_lossy().starts_with("clip");
                done(
                    i,
                    if unreadable {
                        Outcome::NoVector("not read yet".into())
                    } else {
                        Outcome::Vectors(
                            vec![Located {
                                locator: 0,
                                record: vectors::record_from(&[1.0; vectors::DIMS]).unwrap(),
                            }],
                            None,
                            Vec::new(),
                        )
                    },
                );
            }
            Ok(())
        }

        fn warm_up(&self) {
            self.warm_ups.fetch_add(1, Ordering::SeqCst);
        }

        fn release(&self) {
            self.releases.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Fixture {
        shared: Arc<Shared>,
        embedder: Arc<Fake>,
        folder: PathBuf,
        ticket: u64,
    }

    fn fixture(tag: &str, files: &[&str], embedder: Fake) -> Fixture {
        let data = std::env::temp_dir().join(format!("mesa-indexer-{tag}"));
        let _ = fs::remove_dir_all(&data);
        let folder = data.join("photos");
        for name in files {
            let path = folder.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
        let embedder = Arc::new(embedder);
        let store = Store::open(&data.join("index")).unwrap();
        let shared = Arc::new(Shared {
            counts: store.counts(),
            store: Mutex::new(store),
            rules: Mutex::new(BTreeSet::from([folder.clone()])),
            excluded: Mutex::new(BTreeSet::new()),
            pending: Mutex::new(Pending::default()),
            progress: Mutex::new(ProgressState::default()),
            indexed_types: std::sync::atomic::AtomicU8::new(Categories::ALL.bits()),
            control: Control::with_turbo(false, true),
            quiet: Default::default(),
            embedder: embedder.clone(),
            always_full_speed: Default::default(),
            asked_checks: Default::default(),
        });
        let ticket = {
            let mut progress = lock(&shared.progress);
            let ticket = progress.add_job(0, folder.clone()).0;
            progress.begin(ticket);
            ticket
        };
        lock(&shared.pending).add(folder.clone());
        Fixture {
            shared,
            embedder,
            folder,
            ticket,
        }
    }

    impl Fixture {
        fn run(&self) -> bool {
            count_job(&self.shared, self.ticket, &self.folder);
            walk(&self.shared, self.ticket, &self.folder)
        }

        fn seen(&self) -> usize {
            lock(&self.embedder.seen).len()
        }
    }

    #[test]
    fn skips_unchanged_and_unreadable() {
        let f = fixture("again", &["a.jpg", "sub/b.jpg", "clip.mp4"], Fake::default());
        assert!(f.run());
        assert_eq!(f.seen(), 3);

        lock(&f.shared.pending).add(f.folder.clone());
        assert!(f.run());
        assert_eq!(f.seen(), 3);
        assert!(lock(&f.shared.pending).is_empty());
    }

    #[test]
    fn reembeds_changed_file() {
        let f = fixture("changed", &["a.jpg", "b.jpg"], Fake::default());
        assert!(f.run());
        fs::write(f.folder.join("a.jpg"), "now longer").unwrap();

        lock(&f.shared.pending).add(f.folder.clone());
        assert!(f.run());
        assert_eq!(f.seen(), 3);
        assert_eq!(lock(&f.embedder.seen).last().unwrap().file_name().unwrap(), "a.jpg");
    }
}
