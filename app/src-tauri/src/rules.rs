use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
};

pub type Rules = HashMap<PathBuf, bool>;

pub fn effective(rules: &Rules, path: &Path) -> bool {
    path.ancestors().find_map(|a| rules.get(a)).copied().unwrap_or(false)
}

pub fn set_rule(rules: &mut Rules, path: &Path, indexed: bool) {
    set_rules(rules, std::slice::from_ref(&path.to_path_buf()), indexed);
}

pub fn set_rules(rules: &mut Rules, paths: &[PathBuf], indexed: bool) {
    drop_under(rules, paths);
    for path in outermost(paths) {
        let inherited = path.parent().is_some_and(|parent| effective(rules, parent));
        if inherited != indexed {
            rules.insert(path.clone(), indexed);
        }
    }
}

pub fn drop_under(rules: &mut Rules, paths: &[PathBuf]) {
    let all = as_set(paths);
    rules.retain(|rule, _| !is_in(&all, rule));
}

pub fn outermost(paths: &[PathBuf]) -> Vec<&PathBuf> {
    let all = as_set(paths);
    paths
        .iter()
        .filter(|path| !path.ancestors().skip(1).any(|above| all.contains(above)))
        .collect()
}

fn as_set(paths: &[PathBuf]) -> HashSet<&Path> {
    paths.iter().map(PathBuf::as_path).collect()
}

fn is_in(set: &HashSet<&Path>, path: &Path) -> bool {
    path.ancestors().any(|above| set.contains(above))
}

pub fn gaps_of(rules: &Rules) -> BTreeSet<PathBuf> {
    rules
        .iter()
        .filter(|(_, indexed)| !**indexed)
        .map(|(path, _)| path.clone())
        .collect()
}

pub fn inclusions_of(rules: &Rules) -> BTreeSet<PathBuf> {
    rules
        .iter()
        .filter(|(_, indexed)| **indexed)
        .map(|(path, _)| path.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules_with(indexed: &[&str]) -> Rules {
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
