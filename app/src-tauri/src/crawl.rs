use std::{
    path::PathBuf,
    sync::{Condvar, Mutex},
    thread,
};

use crate::index::lock;

pub fn threads() -> usize {
    thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8)
}

pub fn crawl(root: PathBuf, threads: usize, list: impl Fn(PathBuf) -> Vec<PathBuf> + Sync) {
    let crawl = Crawl {
        queue: Mutex::new(Queue {
            folders: vec![root],
            busy: 0,
        }),
        changed: Condvar::new(),
        threads,
    };
    thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            scope.spawn(|| {
                while let Some(folder) = crawl.next() {
                    let busy = Busy(&crawl);
                    let subfolders = list(folder);
                    busy.finish(subfolders);
                }
            });
        }
    });
}

struct Queue {
    folders: Vec<PathBuf>,
    busy: usize,
}

struct Crawl {
    queue: Mutex<Queue>,
    changed: Condvar,
    threads: usize,
}

impl Crawl {
    fn next(&self) -> Option<PathBuf> {
        let mut queue = lock(&self.queue);
        loop {
            if let Some(folder) = queue.folders.pop() {
                queue.busy += 1;
                return Some(folder);
            }
            if queue.busy == 0 {
                return None;
            }
            queue = self.changed.wait(queue).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn done(&self, subfolders: Vec<PathBuf>) {
        let mut queue = lock(&self.queue);
        let found = subfolders.len();
        queue.folders.extend(subfolders);
        queue.busy -= 1;
        if queue.busy == 0 && queue.folders.is_empty() {
            self.changed.notify_all();
        } else {
            for _ in 0..found.min(self.threads) {
                self.changed.notify_one();
            }
        }
    }
}

struct Busy<'a>(&'a Crawl);

impl Busy<'_> {
    fn finish(self, subfolders: Vec<PathBuf>) {
        self.0.done(subfolders);
        std::mem::forget(self);
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.done(Vec::new());
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        fs,
        panic::{AssertUnwindSafe, catch_unwind},
        path::Path,
    };

    use super::*;

    fn tree(tag: &str, breadth: usize, depth: usize) -> PathBuf {
        let root = std::env::temp_dir().join(format!("mesa-crawl-{tag}"));
        let _ = fs::remove_dir_all(&root);
        fn grow(dir: &Path, breadth: usize, depth: usize) {
            fs::create_dir_all(dir).unwrap();
            if depth > 0 {
                for n in 0..breadth {
                    grow(&dir.join(format!("d{n}")), breadth, depth - 1);
                }
            }
        }
        grow(&root, breadth, depth);
        root
    }

    fn subfolders(dir: &Path) -> Vec<PathBuf> {
        fs::read_dir(dir).unwrap().flatten().map(|entry| entry.path()).collect()
    }

    #[test]
    fn visits_each_folder_once() {
        let root = tree("all", 4, 4);
        for threads in [1, 2, 8, 32] {
            let seen = Mutex::new(Vec::new());
            crawl(root.clone(), threads, |dir| {
                let below = subfolders(&dir);
                lock(&seen).push(dir);
                below
            });
            let seen = seen.into_inner().unwrap();
            assert_eq!(seen.len(), 341, "{threads} threads");
            assert_eq!(
                seen.iter().collect::<HashSet<_>>().len(),
                341,
                "{threads} threads: none twice"
            );
        }
    }

    #[test]
    fn panic_does_not_hang_walk() {
        let root = tree("panic", 3, 3);
        let result = catch_unwind(AssertUnwindSafe(|| {
            crawl(root.clone(), 4, |dir| {
                if dir.ends_with("d1") {
                    panic!("one folder goes wrong");
                }
                subfolders(&dir)
            });
        }));
        assert!(result.is_err());
    }
}
