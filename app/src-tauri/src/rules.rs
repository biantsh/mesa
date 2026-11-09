use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub type Rules = HashSet<PathBuf>;

pub fn effective(rules: &Rules, path: &Path) -> bool {
    path.ancestors().any(|a| rules.contains(a))
}

pub fn covered(rules: &Rules, path: &Path) -> bool {
    path.ancestors().skip(1).any(|a| rules.contains(a))
}

pub fn add(rules: &mut Rules, paths: &[PathBuf]) {
    drop_under(rules, paths);
    for path in outermost(paths) {
        if !covered(rules, path) {
            rules.insert(path.clone());
        }
    }
}

pub fn drop_under(rules: &mut Rules, paths: &[PathBuf]) {
    let all = as_set(paths);
    rules.retain(|rule| !is_in(&all, rule));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rules_with(indexed: &[&str]) -> Rules {
        let mut rules = Rules::new();
        add(&mut rules, &indexed.iter().map(PathBuf::from).collect::<Vec<_>>());
        rules
    }

    fn paths(of: &[&str]) -> Vec<PathBuf> {
        of.iter().map(PathBuf::from).collect()
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

    #[test]
    fn parent_absorbs_child_roots() {
        let mut rules = rules_with(&["/photos/2023", "/photos/2024", "/music"]);
        add(&mut rules, &paths(&["/photos"]));
        assert_eq!(rules, rules_with(&["/photos", "/music"]));
    }

    #[test]
    fn remove_root_removes_children() {
        let mut rules = rules_with(&["/photos", "/music"]);
        drop_under(&mut rules, &paths(&["/photos"]));
        assert!(!effective(&rules, Path::new("/photos/b.jpg")));
        assert!(effective(&rules, Path::new("/music/x.mp3")));
        assert_eq!(rules.len(), 1);
    }

    fn universe() -> Vec<PathBuf> {
        let mut all = vec![PathBuf::from("/r")];
        let mut level = vec![PathBuf::from("/r")];
        for _ in 0..3 {
            level = level
                .iter()
                .flat_map(|dir| ["a", "b", "c"].map(|name| dir.join(name)))
                .collect();
            all.extend(level.clone());
        }
        all
    }

    #[test]
    fn roots_never_nested() {
        let all = universe();
        let mut random = fastrand::Rng::with_seed(7);
        for round in 0..600 {
            let mut rules = Rules::new();
            for step in 0..random.usize(1..14) {
                let batch: Vec<PathBuf> = (0..random.usize(1..6))
                    .map(|_| all[random.usize(..all.len())].clone())
                    .collect();
                let adding = random.bool();
                let before = rules.clone();
                if adding {
                    add(&mut rules, &batch);
                } else {
                    drop_under(&mut rules, &batch);
                }
                let what = format!(
                    "round {round}, step {step}: from {before:?}, {} {batch:?}",
                    if adding { "adding" } else { "removing" }
                );
                assert!(
                    rules.iter().all(|root| !covered(&rules, root)),
                    "a root is inside another: {what}"
                );
                for path in &batch {
                    if adding {
                        assert!(effective(&rules, path), "{path:?} was added and is not indexed: {what}");
                    } else if !covered(&before, path) {
                        assert!(!effective(&rules, path), "{path:?} was removed and is indexed: {what}");
                    }
                }
                if !adding {
                    let expected: Rules = before
                        .iter()
                        .filter(|root| !batch.iter().any(|gone| root.starts_with(gone)))
                        .cloned()
                        .collect();
                    assert_eq!(rules, expected, "{what}");
                }
            }
        }
    }
}
