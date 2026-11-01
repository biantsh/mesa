use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use serde::Serialize;

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum IndexStatus {
    NotIndexed,
    #[allow(dead_code)]
    Indexing,
    Indexed,
}

#[derive(Default)]
pub struct IndexRules {
    rules: Mutex<HashMap<PathBuf, bool>>,
}

impl IndexRules {
    fn lock(&self) -> MutexGuard<'_, HashMap<PathBuf, bool>> {
        self.rules.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self, path: &Path) -> IndexStatus {
        if effective(&self.lock(), path) {
            IndexStatus::Indexed
        } else {
            IndexStatus::NotIndexed
        }
    }

    pub fn set(&self, path: &Path, indexed: bool) {
        set_rule(&mut self.lock(), path, indexed);
    }

    pub fn clear(&self) {
        self.lock().clear();
    }

    pub fn set_everything(&self, roots: &[PathBuf]) {
        let mut rules = self.lock();
        rules.clear();
        rules.extend(roots.iter().map(|root| (root.clone(), true)));
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
}
