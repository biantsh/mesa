use std::{
    collections::HashSet,
    fs, mem,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, mpsc::Receiver},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossbeam_channel::bounded;

use crate::{
    browse::{self, Shown},
    crawl::{self, crawl},
    embed::{Item, Outcome},
    index::{Job, Shared, lock},
    store::{ScannedDir, ScannedFile},
    unindexable,
};

const BATCH_FILES: usize = 2000;
const BATCH_AGE: Duration = Duration::from_millis(250);
const CHUNK_FILES: usize = 512;
const LISTED_AHEAD: usize = 1024;

pub fn work(shared: &Shared, jobs: Receiver<Job>) {
    for job in jobs {
        if job.waiting {
            lock(&shared.pending).add(job.root.clone());
        }
        if !lock(&shared.progress).begin(job.ticket) {
            lock(&shared.pending).release(&job.root);
            continue;
        }
        shared.control.wait_while_paused();
        count_files(shared, job.ticket, &job.root);
        let stored = walk(shared, job.ticket, &job.root);
        let wanted = lock(&shared.progress).finish_job(job.ticket);

        let store = lock(&shared.store);
        if wanted
            && stored
            && let Err(e) = store.finish_job(job.id)
        {
            eprintln!("Mesa: couldn't mark indexing of {} as done: {e}", job.root.display());
        }

        let active = lock(&shared.progress).is_active();
        if shared.control.is_paused() && !active {
            let _ = shared.set_paused(&store, false);
        }
        drop(store);
    }
}

fn count_files(shared: &Shared, ticket: u64, root: &Path) {
    if root.is_dir() {
        crawl(root.to_owned(), crawl::threads(), |dir| {
            shared.control.wait_while_paused();
            if !shared.is_indexed(&dir) {
                return Vec::new();
            }
            let mut subdirs = Vec::new();
            let mut files = 0;
            each_entry(&dir, |item, _, shown| match shown {
                Shown::Folder => subdirs.push(item.path()),
                Shown::File(_) => files += 1,
            });
            if files > 0 {
                lock(&shared.progress).found(ticket, files);
            }
            subdirs
        });
    } else if root.is_file()
        && shared.is_indexed(root)
        && root
            .file_name()
            .is_some_and(|name| browse::category_of(&name.to_string_lossy()).is_some())
    {
        lock(&shared.progress).found(ticket, 1);
    }
    lock(&shared.progress).finish_counting(ticket);
}

fn walk(shared: &Shared, ticket: u64, root: &Path) -> bool {
    let mut walker = Walker::new(shared, ticket);

    if root.is_dir() {
        let (listed, found) = bounded::<ScannedDir>(LISTED_AHEAD);
        thread::scope(|scope| {
            scope.spawn(move || {
                crawl(root.to_owned(), crawl::threads(), |dir| {
                    shared.control.wait_while_paused();

                    let found = if shared.is_indexed(&dir) { scan_dir(&dir) } else { None };

                    {
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
        match scan_file(root).filter(|_| shared.is_indexed(root)) {
            Some(scan) => walker.add(scan),
            None => lock(&shared.pending).release(root),
        }
    }
    walker.embed_waiting();
    walker.stored & walker.batch.store(shared)
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
}

impl<'a> Walker<'a> {
    fn new(shared: &'a Shared, ticket: u64) -> Self {
        Self {
            shared,
            batch: Batch::new(ticket),
            folders: Vec::new(),
            waiting: Vec::new(),
            failed: false,
            stored: true,
            warmed_up: false,
            new_files: HashSet::new(),
        }
    }

    fn waiting(&self) -> usize {
        self.waiting.len()
    }

    fn fail(&mut self) {
        self.failed = true;
        self.stored = false;
    }

    fn add(&mut self, scan: ScannedDir) {
        if scan.files.is_empty() {
            if scan.complete {
                self.batch.scans.push(scan);
            }
            return;
        }
        let changes = lock(&self.shared.store).changes(&scan).ok();
        let index = self.folders.len();
        let mut up_to_date = 0;
        for (n, file) in scan.files.iter().enumerate() {
            if changes
                .as_ref()
                .is_some_and(|changes| !changes.changed.contains(&file.name))
            {
                self.batch.finished.push(scan.path.join(&file.name));
                up_to_date += 1;
            } else {
                if changes.as_ref().is_some_and(|changes| changes.new.contains(&file.name)) {
                    self.new_files.insert(scan.path.join(&file.name));
                }
                self.waiting.push((index, n));
            }
        }
        if up_to_date > 0 {
            self.batch.files += up_to_date;
            lock(&self.shared.progress).processed(self.batch.ticket, up_to_date as u64);
        }
        if !self.warmed_up && !self.waiting.is_empty() {
            self.warmed_up = true;
            self.shared.embedder.warm_up();
        }
        if self.waiting.last().is_some_and(|&(last, _)| last == index) {
            self.folders.push(scan);
        } else if scan.complete {
            self.batch.scans.push(scan);
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
                let wanted = shared.is_indexed(&path);
                if !wanted {
                    lock(&shared.pending).release(&path);
                }
                wanted
            });
            if left.is_empty() {
                break;
            }
            if self.failed {
                for &n in &left {
                    lock(&shared.pending).release(&path_of(&folders, places[n]));
                }
                lock(&shared.progress).processed(self.batch.ticket, left.len() as u64);
                break;
            }
            self.stored &= self.batch.pause_point(shared);

            let epoch = shared.control.epoch();
            let items: Vec<Item> = left
                .iter()
                .map(|&n| Item {
                    path: path_of(&folders, places[n]),
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
                    file.vectors = match outcome {
                        Outcome::Vectors(vectors) => vectors,
                        Outcome::NoVector(_) => Vec::new(),
                    };
                    if new_files.remove(&items[i].path) {
                        batch.new += 1;
                        shared.finished_new.fetch_add(1, Ordering::SeqCst);
                    }
                    batch.finished.push(items[i].path.clone());
                    batch.scans.push(ScannedDir {
                        path: folders[folder].path.clone(),
                        files: vec![file],
                        complete: false,
                    });
                    batch.files += 1;
                    lock(&shared.progress).processed(batch.ticket, 1);
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

        if !self.failed {
            for folder in folders.drain(..).filter(|folder| folder.complete) {
                self.batch.files += folder.files.len();
                self.batch.scans.push(folder);
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
    ticket: u64,
}

impl Batch {
    fn new(ticket: u64) -> Self {
        Self {
            scans: Vec::new(),
            finished: Vec::new(),
            files: 0,
            new: 0,
            started: Instant::now(),
            ticket,
        }
    }

    fn pause_point(&mut self, shared: &Shared) -> bool {
        if !shared.control.is_paused() {
            return true;
        }
        let stored = self.store(shared);
        shared.control.wait_while_paused();
        stored
    }

    fn due(&self) -> bool {
        (!self.scans.is_empty() || !self.finished.is_empty())
            && (self.files >= BATCH_FILES || self.started.elapsed() >= BATCH_AGE)
    }

    fn store(&mut self, shared: &Shared) -> bool {
        let mut stored = true;
        if !self.scans.is_empty() {
            let mut store = lock(&shared.store);
            let result = store.write_scans(&self.scans, &|path| shared.is_indexed(path));
            if let Err(e) = result {
                eprintln!("Mesa: couldn't store indexed files: {e}");
                stored = false;
            }
            shared.landed_new.fetch_add(mem::take(&mut self.new), Ordering::SeqCst);
            drop(store);
        }
        let mut pending = lock(&shared.pending);
        self.finished.drain(..).for_each(|path| pending.release(&path));
        drop(pending);
        self.scans.clear();
        self.files = 0;
        self.started = Instant::now();
        stored
    }
}

impl Drop for Walker<'_> {
    fn drop(&mut self) {
        self.shared
            .landed_new
            .fetch_add(mem::take(&mut self.batch.new), Ordering::SeqCst);
    }
}

struct Found {
    scan: ScannedDir,
    subdirs: Vec<PathBuf>,
}

fn each_entry(dir: &Path, mut visit: impl FnMut(&fs::DirEntry, String, Shown)) -> Option<bool> {
    let mut complete = true;
    for item in fs::read_dir(dir).ok()? {
        let Ok(item) = item else {
            complete = false;
            continue;
        };
        let Some((name, shown)) = browse::classify(&item) else {
            continue;
        };
        if matches!(shown, Shown::Folder)
            && (item.file_type().map_or(true, |kind| kind.is_symlink())
                || (unindexable::could_be_unindexable(&name) && unindexable::is_unindexable(&item.path())))
        {
            continue;
        }
        visit(&item, name, shown);
    }
    Some(complete)
}

fn scan_dir(dir: &Path) -> Option<Found> {
    let mut files = Vec::new();
    let mut subdirs = Vec::new();
    let mut unreadable_file = false;
    let all_listed = each_entry(dir, |item, name, shown| match shown {
        Shown::Folder => subdirs.push(item.path()),
        Shown::File(category) => match facts(item) {
            Some((modified_ms, size)) => files.push(ScannedFile {
                name,
                category,
                modified_ms,
                size,
                vectors: Vec::new(),
            }),
            None => unreadable_file = true,
        },
    })?;
    let complete = all_listed && !unreadable_file;
    Some(Found {
        scan: ScannedDir {
            path: dir.to_owned(),
            files,
            complete,
        },
        subdirs,
    })
}

fn scan_file(path: &Path) -> Option<ScannedDir> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let category = browse::category_of(&name)?;
    let (modified_ms, size) = facts_of(&fs::metadata(path).ok()?)?;
    Some(ScannedDir {
        path: path.parent()?.to_owned(),
        files: vec![ScannedFile {
            name,
            category,
            modified_ms,
            size,
            vectors: Vec::new(),
        }],
        complete: false,
    })
}

pub fn facts(item: &fs::DirEntry) -> Option<(i64, u64)> {
    let metadata = if item.file_type().ok()?.is_symlink() {
        fs::metadata(item.path()).ok()?
    } else {
        item.metadata().ok()?
    };
    facts_of(&metadata)
}

fn facts_of(metadata: &fs::Metadata) -> Option<(i64, u64)> {
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
        collections::{BTreeSet, HashMap, HashSet, VecDeque},
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
                        Outcome::Vectors(vec![Located {
                            locator: 0,
                            record: vectors::record_from(&[1.0; vectors::DIMS]).unwrap(),
                        }])
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
        let shared = Arc::new(Shared {
            store: Mutex::new(Store::open(&data.join("index")).unwrap()),
            rules: Mutex::new(HashMap::from([(folder.clone(), true)])),
            gaps: Mutex::new(BTreeSet::new()),
            included: Mutex::new(BTreeSet::new()),
            excluded: Mutex::new(HashSet::new()),
            pending: Mutex::new(Pending::default()),
            progress: Mutex::new(ProgressState::default()),
            finished_new: std::sync::atomic::AtomicU64::new(0),
            landed_new: std::sync::atomic::AtomicU64::new(0),
            control: Control::new(false),
            embedder: embedder.clone(),
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
            count_files(&self.shared, self.ticket, &self.folder);
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
