use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::index::lock;

const DEPENDENCY_FOLDERS: &[&str] = &[
    "node_modules",
    "bower_components",
    "site-packages",
    "dist-packages",
    "venv",
    "virtualenv",
    "__pycache__",
    "target",
    "dist",
    "build",
    "Pods",
    "Carthage",
    "DerivedData",
    "vendor",
    "third_party",
    "third-party",
    "deps",
    "_deps",
    "obj",
];
const NEVER_THE_USERS: &[&str] = &[
    "node_modules",
    "bower_components",
    "jspm_packages",
    "site-packages",
    "dist-packages",
    "__pycache__",
    "__pypackages__",
    "Pods",
    "Carthage",
    "DerivedData",
    "dist-newstyle",
    "elm-stuff",
    "_opam",
    "_build",
    "zig-cache",
    "zig-out",
    "vcpkg_installed",
    "CMakeFiles",
    "_deps",
];

const OTHERS_MARKERS: &[&str] = &["conda-meta", "pyvenv.cfg", "CACHEDIR.TAG"];

const MADE_BESIDE: &[(&str, &[&str])] = &[
    ("target", &["Cargo.toml", "pom.xml"]),
    (
        "build",
        &[
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
            "pubspec.yaml",
        ],
    ),
    ("deps", &["mix.exs"]),
];

pub fn is_others_folder(dir: &Path) -> bool {
    let Some(name) = dir.file_name().map(|name| name.to_string_lossy().into_owned()) else {
        return false;
    };
    if NEVER_THE_USERS.contains(&name.as_str()) || OTHERS_MARKERS.iter().any(|marker| dir.join(marker).exists()) {
        return true;
    }
    let parent = dir.parent();
    let beside = |files: &[&str]| parent.is_some_and(|parent| files.iter().any(|file| parent.join(file).is_file()));
    let parent_named = |wanted: &str| parent.and_then(Path::file_name).is_some_and(|parent| parent == wanted);
    let grandparent_has = |file: &str| {
        parent
            .and_then(Path::parent)
            .is_some_and(|above| above.join(file).is_file())
    };
    match name.as_str() {
        "vendor" => dir.join("autoload.php").is_file() || dir.join("modules.txt").is_file(),
        "bundle" => parent_named("vendor") && grandparent_has("Gemfile"),
        "library" => parent_named("renv") && grandparent_has("renv.lock"),
        "mod" => parent_named("pkg") && dir.join("cache").join("download").is_dir(),
        _ => MADE_BESIDE.iter().any(|(made, files)| *made == name && beside(files)),
    }
}

pub fn in_others(path: &Path) -> bool {
    path.ancestors()
        .skip(1)
        .any(|folder| folder.parent().is_some() && is_others_folder(folder))
}

pub const INSTALLED_FILES: usize = 5000;
const INSTALLED_MARKERS: &[&str] = &[".vcpkg-root", "emsdk.py", "gclient.py", "packages/flutter_tools"];

#[derive(Default)]
struct Cache {
    marked: HashMap<PathBuf, bool>,
    installed_roots: HashMap<PathBuf, bool>,
    units: HashMap<PathBuf, usize>,
    gitignores: HashMap<PathBuf, Option<Arc<Gitignore>>>,
    repositories: HashMap<PathBuf, Option<PathBuf>>,
    projects: HashMap<PathBuf, bool>,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);

pub fn forget() {
    *lock(&CACHE) = Cache::default();
}

pub fn allows_folder(dir: &Path) -> bool {
    !in_dependency(dir) && !in_marked(dir) && !in_installed(dir)
}

pub fn allows(path: &Path) -> bool {
    path.parent().is_some_and(allows_folder) && !is_ignored(path)
}

fn in_dependency(dir: &Path) -> bool {
    dir.components()
        .any(|part| DEPENDENCY_FOLDERS.contains(&part.as_os_str().to_string_lossy().as_ref()))
}

fn in_marked(dir: &Path) -> bool {
    dir.ancestors().any(|folder| {
        if let Some(&known) = lock(&CACHE).marked.get(folder) {
            return known;
        }
        let is = OTHERS_MARKERS.iter().any(|marker| folder.join(marker).exists());
        lock(&CACHE).marked.insert(folder.to_owned(), is);
        is
    })
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

struct Place {
    top: PathBuf,
    unit: Option<PathBuf>,
}

fn place_of(dir: &Path, home: Option<&Path>) -> Option<Place> {
    if let Some(home) = home.filter(|home| dir.starts_with(home)) {
        let mut inside = dir.strip_prefix(home).ok()?.components();
        let top = home.join(inside.next()?);
        let unit = inside.next().map(|second| top.join(second));
        return Some(Place { top, unit });
    }
    let top = top_outside_home(dir)?;
    Some(Place {
        unit: Some(top.clone()),
        top,
    })
}

fn top_outside_home(dir: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let mut parts = dir.components();
        let prefix = parts.next()?;
        let root = parts.next()?;
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        if prefix.as_os_str().to_string_lossy().eq_ignore_ascii_case(&system) {
            return None;
        }
        let first = parts.next()?;
        Some(
            PathBuf::from(prefix.as_os_str())
                .join(root.as_os_str())
                .join(first.as_os_str()),
        )
    }
    #[cfg(not(windows))]
    {
        for (mount, depth) in [("/Volumes", 2), ("/media", 3), ("/run/media", 3), ("/mnt", 2)] {
            if let Ok(inside) = dir.strip_prefix(mount) {
                let parts: Vec<_> = inside.components().take(depth).collect();
                if parts.len() == depth {
                    return Some(parts.iter().fold(PathBuf::from(mount), |path, part| path.join(part)));
                }
            }
        }
        None
    }
}

fn in_installed(dir: &Path) -> bool {
    installed_below(dir, home().as_deref(), INSTALLED_FILES)
}

fn installed_below(dir: &Path, home: Option<&Path>, most: usize) -> bool {
    let Some(Place { top, unit }) = place_of(dir, home) else {
        return false;
    };
    if dir
        .ancestors()
        .take_while(|folder| folder.starts_with(&top))
        .any(is_installed_root)
    {
        return true;
    }
    if repository_of(dir).is_some_and(|repository| repository.starts_with(&top)) {
        return false;
    }
    let Some(unit) = unit else { return false };
    if let Some(&count) = lock(&CACHE).units.get(&unit) {
        return count > most;
    }
    let count = count_code(&unit, most + 1);
    lock(&CACHE).units.insert(unit, count);
    count > most
}

fn is_installed_root(folder: &Path) -> bool {
    if let Some(&known) = lock(&CACHE).installed_roots.get(folder) {
        return known;
    }
    let is = INSTALLED_MARKERS.iter().any(|marker| folder.join(marker).exists());
    lock(&CACHE).installed_roots.insert(folder.to_owned(), is);
    is
}

fn count_code(top: &Path, most: usize) -> usize {
    let mut count = 0;
    let mut stack = vec![top.to_owned()];
    while let Some(folder) = stack.pop() {
        if in_marked(&folder) {
            continue;
        }
        for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                let path = entry.path();
                if !DEPENDENCY_FOLDERS.contains(&name.as_str()) && !path.join(".git").exists() {
                    stack.push(path);
                }
            } else if kind.is_file() && crate::embed::code::is_code_name(&name) {
                count += 1;
                if count >= most {
                    return count;
                }
            }
        }
    }
    count
}

pub fn in_project(path: &Path) -> bool {
    let Some(dir) = path.parent() else { return false };
    if let Some(&known) = lock(&CACHE).projects.get(dir) {
        return known;
    }
    let is = crate::embed::code::project_root(path).is_some();
    lock(&CACHE).projects.insert(dir.to_owned(), is);
    is
}

fn repository_of(dir: &Path) -> Option<PathBuf> {
    if let Some(known) = lock(&CACHE).repositories.get(dir) {
        return known.clone();
    }
    let found = dir
        .ancestors()
        .find(|folder| folder.join(".git").exists())
        .map(Path::to_path_buf);
    lock(&CACHE).repositories.insert(dir.to_owned(), found.clone());
    found
}

fn gitignore_in(folder: &Path) -> Option<Arc<Gitignore>> {
    if let Some(known) = lock(&CACHE).gitignores.get(folder) {
        return known.clone();
    }
    let file = folder.join(".gitignore");
    let read = file.is_file().then(|| {
        let mut builder = GitignoreBuilder::new(folder);
        builder.add(&file);
        builder.build().ok().map(Arc::new)
    });
    let read = read.flatten();
    lock(&CACHE).gitignores.insert(folder.to_owned(), read.clone());
    read
}

pub fn is_ignored(path: &Path) -> bool {
    let Some(dir) = path.parent() else { return false };
    let Some(repository) = repository_of(dir) else {
        return false;
    };
    for folder in dir.ancestors() {
        if let Some(gitignore) = gitignore_in(folder) {
            let matched = gitignore.matched_path_or_any_parents(path, false);
            if matched.is_ignore() {
                return true;
            }
            if matched.is_whitelist() {
                return false;
            }
        }
        if folder == repository {
            break;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mesa-code-scope-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn skips_dependencies_and_virtualenvs() {
        let root = temp("deps");
        assert!(in_dependency(&root.join("app/node_modules/react/lib")));
        assert!(in_dependency(&root.join("tool/target/debug/build")));
        assert!(!in_dependency(&root.join("app/src/components")));
        fs::create_dir_all(root.join("miniforge3/conda-meta")).unwrap();
        fs::create_dir_all(root.join("miniforge3/lib/python3.14/json")).unwrap();
        assert!(in_marked(&root.join("miniforge3/lib/python3.14/json")));
        fs::create_dir_all(root.join("project/.venv")).unwrap();
        fs::write(root.join("project/.venv/pyvenv.cfg"), "home = /usr/bin").unwrap();
        assert!(in_marked(&root.join("project/.venv/lib")));
        assert!(!in_marked(&root.join("project/src")));
    }

    #[test]
    fn respects_gitignore() {
        let root = temp("gitignore");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("generated")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join(".gitignore"), "generated/\n*.gen.ts\n").unwrap();
        assert!(is_ignored(&root.join("generated/api.py")));
        assert!(is_ignored(&root.join("src/schema.gen.ts")));
        assert!(!is_ignored(&root.join("src/main.ts")));
    }

    fn code_files(dir: &Path, count: usize) {
        fs::create_dir_all(dir).unwrap();
        for n in 0..count {
            fs::write(dir.join(format!("m{n}.py")), "print()").unwrap();
        }
    }

    #[test]
    fn tells_installed_software_from_the_users_code() {
        const MOST: usize = 50;
        let home = temp("installed");
        let installed = |dir: &str| installed_below(&home.join(dir), Some(&home), MOST);
        fs::create_dir_all(home.join(".git")).unwrap();

        for repository in ["app", "api", "tools"] {
            fs::create_dir_all(home.join("Projects").join(repository).join(".git")).unwrap();
            code_files(&home.join("Projects").join(repository).join("src"), 20);
        }
        assert!(
            !installed("Projects/app/src"),
            "repositories that add up past the limit"
        );
        assert!(!installed("Projects"), "files right in a top folder");

        for repository in ["web", "cli"] {
            fs::create_dir_all(home.join("Work/clients").join(repository).join(".git")).unwrap();
            code_files(&home.join("Work/clients").join(repository), 30);
        }
        code_files(&home.join("Work/clients/notes"), 2);
        assert!(
            !installed("Work/clients/notes"),
            "repositories don't count toward the folder around them"
        );

        for course in 1..=6 {
            code_files(&home.join(format!("Courses/week{course}")), 10);
        }
        assert!(
            !installed("Courses/week1"),
            "small projects without git that add up past the limit"
        );

        code_files(&home.join("scoop/apps/python/current/Lib"), MOST + 1);
        assert!(
            installed("scoop/apps/python/current/Lib"),
            "software, even with a repository around the home folder"
        );
        code_files(&home.join("Downloads/linux-6.1/kernel"), MOST + 1);
        assert!(
            installed("Downloads/linux-6.1/kernel"),
            "a big folder of code that isn't a repository"
        );

        fs::create_dir_all(home.join("dev/vcpkg/.git")).unwrap();
        fs::write(home.join("dev/vcpkg/.vcpkg-root"), "").unwrap();
        code_files(&home.join("dev/vcpkg/ports/zlib"), 1);
        assert!(installed("dev/vcpkg/ports/zlib"), "an SDK installed with git clone");
        fs::create_dir_all(home.join("dev/flutter/.git")).unwrap();
        fs::create_dir_all(home.join("dev/flutter/packages/flutter_tools")).unwrap();
        code_files(&home.join("dev/flutter/packages/flutter/lib"), 1);
        assert!(installed("dev/flutter/packages/flutter/lib"));
        fs::create_dir_all(home.join("dev/game/.git")).unwrap();
        code_files(&home.join("dev/game/src"), 1);
        assert!(!installed("dev/game/src"), "a repository beside an SDK");
    }

    #[test]
    fn looks_again_once_forgotten() {
        const MOST: usize = 10;
        let home = temp("forgotten");
        let installed = || installed_below(&home.join("Downloads/tool/src"), Some(&home), MOST);
        code_files(&home.join("Downloads/tool/src"), MOST + 1);
        assert!(installed());
        fs::create_dir_all(home.join("Downloads/tool/.git")).unwrap();
        assert!(installed(), "kept until forgotten");
        forget();
        assert!(!installed());
    }
}
