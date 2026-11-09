use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use serde::{Deserialize, Serialize};

use crate::{
    browse::{CATEGORIES, Category},
    crawl::{self, crawl},
    folder_access::Protected,
    index::lock,
    indexer,
};

pub const SAMPLES: usize = 24;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct PerType<T> {
    pub image: T,
    pub video: T,
    pub audio: T,
    pub document: T,
}

impl<T> PerType<T> {
    pub fn get(&self, category: Category) -> &T {
        match category {
            Category::Image => &self.image,
            Category::Video => &self.video,
            Category::Audio => &self.audio,
            Category::Document => &self.document,
        }
    }

    pub fn get_mut(&mut self, category: Category) -> &mut T {
        match category {
            Category::Image => &mut self.image,
            Category::Video => &mut self.video,
            Category::Audio => &mut self.audio,
            Category::Document => &mut self.document,
        }
    }
}

pub type Sample = (PathBuf, u64);

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Survey {
    pub counts: PerType<u64>,
    pub bytes: PerType<u64>,
    pub not_asked: Vec<String>,
    pub no_access: Vec<String>,
    #[serde(skip)]
    pub samples: PerType<Vec<Sample>>,
    #[serde(skip)]
    pub by_root: Vec<(PathBuf, PerType<u64>)>,
}

struct Reservoir {
    seen: u64,
    kept: Vec<Sample>,
    state: u64,
}

impl Default for Reservoir {
    fn default() -> Self {
        use std::hash::{BuildHasher, Hasher};
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(0);
        Self {
            seen: 0,
            kept: Vec::new(),
            state: hasher.finish() | 1,
        }
    }
}

impl Reservoir {
    fn offer(&mut self, sample: Sample) {
        self.seen += 1;
        if self.kept.len() < SAMPLES {
            self.kept.push(sample);
        } else {
            let at = (self.next() % self.seen) as usize;
            if at < SAMPLES {
                self.kept[at] = sample;
            }
        }
    }

    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }
}

pub fn survey(roots: &[PathBuf], protected: &[Protected], excluded: impl Fn(&Path) -> bool + Sync) -> Survey {
    let counts: [AtomicU64; CATEGORIES.len()] = Default::default();
    let bytes: [AtomicU64; CATEGORIES.len()] = Default::default();
    let reservoirs: [Mutex<Reservoir>; CATEGORIES.len()] = Default::default();
    let within = |folder: &Path| roots.iter().any(|root| folder.starts_with(root));
    let not_asked: Vec<&Protected> = protected
        .iter()
        .filter(|folder| !folder.asked && within(&folder.path) && folder.path.is_dir())
        .collect();
    let no_access = Mutex::new(Vec::new());
    let at = |category: Category| CATEGORIES.iter().position(|&each| each == category).unwrap_or(0);

    let mut by_root = Vec::new();
    for root in roots {
        let before: Vec<u64> = counts.iter().map(|count| count.load(Ordering::Relaxed)).collect();
        crawl(root.clone(), crawl::threads(), false, |dir| {
            if excluded(&dir) || not_asked.iter().any(|folder| dir.starts_with(&folder.path)) {
                return Vec::new();
            }
            if protected.iter().any(|folder| folder.path == dir) && denied(&dir) {
                lock(&no_access).push(dir);
                return Vec::new();
            }
            let mut found: [(u64, u64, Vec<Sample>); CATEGORIES.len()] = Default::default();
            let subdirs = indexer::survey_folder(&dir, |category, path, size| {
                let mine = &mut found[at(category)];
                mine.0 += 1;
                mine.1 += size;
                mine.2.push((path, size));
            });
            for (i, (files, size, samples)) in found.into_iter().enumerate() {
                if files == 0 {
                    continue;
                }
                counts[i].fetch_add(files, Ordering::Relaxed);
                bytes[i].fetch_add(size, Ordering::Relaxed);
                let mut reservoir = lock(&reservoirs[i]);
                samples.into_iter().for_each(|sample| reservoir.offer(sample));
            }
            subdirs.unwrap_or_default()
        });
        let mut here = PerType::default();
        for (i, &category) in CATEGORIES.iter().enumerate() {
            *here.get_mut(category) = counts[i].load(Ordering::Relaxed) - before[i];
        }
        by_root.push((root.clone(), here));
    }

    let no_access = no_access.into_inner().unwrap_or_else(|e| e.into_inner());
    let denied_in_order = protected
        .iter()
        .filter(|folder| no_access.contains(&folder.path))
        .map(|folder| folder.name.clone())
        .collect();
    let not_asked = not_asked.into_iter().map(|folder| folder.name.clone()).collect();
    let mut survey = Survey {
        not_asked,
        no_access: denied_in_order,
        by_root,
        ..Survey::default()
    };
    for (i, &category) in CATEGORIES.iter().enumerate() {
        *survey.counts.get_mut(category) = counts[i].load(Ordering::Relaxed);
        *survey.bytes.get_mut(category) = bytes[i].load(Ordering::Relaxed);
        *survey.samples.get_mut(category) = std::mem::take(&mut lock(&reservoirs[i]).kept);
    }
    survey
}

fn denied(dir: &Path) -> bool {
    fs::read_dir(dir).is_err_and(|e| e.kind() == io::ErrorKind::PermissionDenied)
}
