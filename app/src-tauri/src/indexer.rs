use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::Receiver,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    browse::{self, Shown},
    index::{Job, Shared, lock},
    store::{ScannedDir, ScannedFile},
};

const BATCH_FILES: usize = 2000;
const BATCH_AGE: Duration = Duration::from_millis(250);

const SLOW_DOWN_PER_FILE: Duration = if cfg!(test) {
    Duration::ZERO
} else {
    Duration::from_secs(1)
};

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
        if wanted && stored {
            if let Err(e) = store.finish_job(job.id) {
                eprintln!("Mesa: couldn't mark indexing of {} as done: {e}", job.root.display());
            }
        }

        if shared.control.is_paused() && !lock(&shared.progress).is_active() {
            let _ = shared.set_paused(&store, false);
        }
    }
}

fn count_files(shared: &Shared, ticket: u64, root: &Path) {
    if root.is_dir() {
        let mut stack = vec![root.to_owned()];
        while let Some(dir) = stack.pop() {
            shared.control.wait_while_paused();
            if !shared.is_indexed(&dir) {
                continue;
            }
            let mut files = 0;
            each_entry(&dir, |item, _, shown| match shown {
                Shown::Folder => stack.push(item.path()),
                Shown::File(_) => files += 1,
            });
            lock(&shared.progress).found(ticket, files);
        }
    } else if root.is_file()
        && root
            .file_name()
            .is_some_and(|name| browse::category_of(&name.to_string_lossy()).is_some())
    {
        lock(&shared.progress).found(ticket, 1);
    }
    lock(&shared.progress).finish_counting(ticket);
}

fn walk(shared: &Shared, ticket: u64, root: &Path) -> bool {
    let mut batch = Batch::new(ticket);
    let mut stored = true;

    if root.is_dir() {
        let mut stack = vec![root.to_owned()];
        while let Some(dir) = stack.pop() {
            stored &= batch.pause_point(shared);

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

            if let Some(found) = found {
                stack.extend(found.subdirs);
                stored &= store_files(shared, &mut batch, found.scan);
            }
            if batch.due() {
                stored &= batch.store(shared);
            }
        }
    } else {
        match scan_file(root) {
            Some(scan) => stored &= store_files(shared, &mut batch, scan),
            None => lock(&shared.pending).release(root),
        }
    }
    stored & batch.store(shared)
}

fn store_files(shared: &Shared, batch: &mut Batch, scan: ScannedDir) -> bool {
    if SLOW_DOWN_PER_FILE.is_zero() {
        batch
            .finished
            .extend(scan.files.iter().map(|file| scan.path.join(&file.name)));
        batch.files += scan.files.len();
        lock(&shared.progress).processed(batch.ticket, scan.files.len() as u64);
        batch.scans.push(scan);
        true
    } else {
        store_slowly(shared, batch, scan, SLOW_DOWN_PER_FILE)
    }
}

fn store_slowly(shared: &Shared, batch: &mut Batch, scan: ScannedDir, delay: Duration) -> bool {
    let changed = lock(&shared.store).changed(&scan).unwrap_or_default();
    let ScannedDir { path, files, complete } = scan;
    let mut stored = true;
    for (done, file) in files.iter().enumerate() {
        let needs_work = changed.contains(&file.name);
        loop {
            if needs_work {
                stored &= batch.pause_point(shared);
            }
            if !shared.is_indexed(&path) {
                let mut pending = lock(&shared.pending);
                files[done..]
                    .iter()
                    .for_each(|file| pending.release(&path.join(&file.name)));
                return stored;
            }
            if !needs_work || shared.control.sleep(delay) {
                break;
            }
        }
        batch.scans.push(ScannedDir {
            path: path.clone(),
            files: vec![file.clone()],
            complete: false,
        });
        batch.finished.push(path.join(&file.name));
        batch.files += 1;
        lock(&shared.progress).processed(batch.ticket, 1);
        if needs_work || batch.due() {
            stored &= batch.store(shared);
        }
    }
    if complete {
        batch.scans.push(ScannedDir { path, files, complete });
        stored &= batch.store(shared);
    }
    stored
}

struct Batch {
    scans: Vec<ScannedDir>,
    finished: Vec<PathBuf>,
    files: usize,
    started: Instant,
    ticket: u64,
}

impl Batch {
    fn new(ticket: u64) -> Self {
        Self {
            scans: Vec::new(),
            finished: Vec::new(),
            files: 0,
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
        !self.scans.is_empty() && (self.files >= BATCH_FILES || self.started.elapsed() >= BATCH_AGE)
    }

    fn store(&mut self, shared: &Shared) -> bool {
        let mut stored = true;
        if !self.scans.is_empty() {
            let result = lock(&shared.store).write_scans(&self.scans, &|path| shared.is_indexed(path));
            if let Err(e) = result {
                eprintln!("Mesa: couldn't store indexed files: {e}");
                stored = false;
            }
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
        if matches!(shown, Shown::Folder) && item.file_type().map_or(true, |kind| kind.is_symlink()) {
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
        }],
        complete: false,
    })
}

fn facts(item: &fs::DirEntry) -> Option<(i64, u64)> {
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
    use super::*;

    #[test]
    fn times_before_1970() {
        assert_eq!(unix_millis(UNIX_EPOCH + Duration::from_millis(1500)), 1500);
        assert_eq!(unix_millis(UNIX_EPOCH - Duration::from_millis(1500)), -1500);
    }
}
