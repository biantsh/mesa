use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::{
    browse::{CATEGORIES, Categories},
    folder_access::Protected,
    index::lock,
    survey::{self, PerType},
};

const FRESH: Duration = Duration::from_secs(5 * 60);

pub type ByFolder = Arc<Vec<(PathBuf, PerType<u64>)>>;

struct Crawled {
    root: PathBuf,
    at: Instant,
    not_asked: Vec<PathBuf>,
    by_folder: ByFolder,
}

#[derive(Default)]
pub struct FolderCounts {
    kept: Mutex<Option<Crawled>>,
    turn: Mutex<()>,
    latest: AtomicU64,
}

impl FolderCounts {
    pub fn by_folder(&self, folder: &Path, protected: &[Protected]) -> Option<ByFolder> {
        let not_asked: Vec<PathBuf> = protected
            .iter()
            .filter(|folder| !folder.asked)
            .map(|folder| folder.path.clone())
            .collect();
        if let Some(found) = self.kept(folder, &not_asked) {
            return Some(found);
        }
        let ticket = self.latest.fetch_add(1, Ordering::SeqCst) + 1;
        let superseded = || self.latest.load(Ordering::SeqCst) != ticket;
        let _turn = lock(&self.turn);
        if let Some(found) = self.kept(folder, &not_asked) {
            return Some(found);
        }
        if superseded() {
            return None;
        }
        let survey = survey::survey(&[folder.to_path_buf()], protected, |_| superseded());
        if superseded() {
            return None;
        }
        *lock(&self.kept) = Some(Crawled {
            root: folder.to_path_buf(),
            at: Instant::now(),
            not_asked,
            by_folder: survey.by_folder.clone(),
        });
        Some(survey.by_folder)
    }

    fn kept(&self, folder: &Path, not_asked: &[PathBuf]) -> Option<ByFolder> {
        lock(&self.kept)
            .as_ref()
            .filter(|kept| kept.at.elapsed() < FRESH && folder.starts_with(&kept.root) && kept.not_asked == not_asked)
            .map(|kept| kept.by_folder.clone())
    }
}

pub fn files_in(counts: &PerType<u64>, types: Categories) -> u64 {
    CATEGORIES
        .iter()
        .filter(|&&kind| types.contains(kind))
        .map(|&kind| *counts.get(kind))
        .sum()
}

pub fn files_under(
    by_folder: &[(PathBuf, PerType<u64>)],
    folder: &Path,
    types: Categories,
    skip: impl Fn(&Path) -> bool,
) -> u64 {
    let start = by_folder.partition_point(|(dir, _)| dir.as_path() < folder);
    by_folder[start..]
        .iter()
        .take_while(|(dir, _)| dir.starts_with(folder))
        .filter(|(dir, _)| !skip(dir))
        .map(|(_, counts)| files_in(counts, types))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browse::Category;

    fn images(n: u64) -> PerType<u64> {
        PerType {
            image: n,
            ..PerType::default()
        }
    }

    #[test]
    fn sums_each_subtree() {
        let by_folder: Vec<(PathBuf, PerType<u64>)> = vec![
            (PathBuf::from("/a"), images(1)),
            (PathBuf::from("/a/b"), images(2)),
            (PathBuf::from("/a/b/c"), images(4)),
            (PathBuf::from("/a/bb"), images(8)),
            (PathBuf::from("/a/c"), images(16)),
        ];
        let types = Categories::of(&[Category::Image]);
        let under = |folder: &str| files_under(&by_folder, Path::new(folder), types, |_| false);
        assert_eq!(under("/a"), 31);
        assert_eq!(under("/a/b"), 6);
        assert_eq!(under("/a/bb"), 8);
        assert_eq!(under("/a/d"), 0);
        assert_eq!(
            files_under(&by_folder, Path::new("/a"), types, |dir| dir.starts_with("/a/b")),
            25
        );
        assert_eq!(
            files_under(&by_folder, Path::new("/a"), Categories::of(&[Category::Video]), |_| {
                false
            }),
            0
        );
    }
}
