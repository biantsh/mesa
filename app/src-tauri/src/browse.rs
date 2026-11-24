use std::{
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};

use crate::{index::IndexStatus, indexer, unindexable};

#[cfg(windows)]
const COMPUTER_LABEL: &str = "This PC";
#[cfg(not(windows))]
const COMPUTER_LABEL: &str = "Computer";

const INDEXABLE: &[(Category, &[&str])] = &[
    (
        Category::Image,
        &[
            "jpg", "jpeg", "jfif", "jpe", "png", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "avif", "dng",
            "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "orf", "rw2", "pef", "raf", "srw",
        ],
    ),
    (
        Category::Video,
        &[
            "mp4", "m4v", "mov", "mkv", "avi", "webm", "3gp", "3g2", "mpg", "mpeg", "mts", "m2ts", "wmv",
        ],
    ),
    (
        Category::Audio,
        &[
            "mp3", "wav", "wave", "flac", "m4a", "m4b", "aac", "aiff", "aif", "ogg", "opus", "caf", "amr", "wma",
        ],
    ),
    (
        Category::Document,
        &[
            "pdf", "txt", "md", "markdown", "html", "htm", "xhtml", "docx", "docm", "xlsx", "xlsm", "pptx", "pptm",
            "ppsx", "odt", "ods", "odp", "pages", "numbers", "key", "rtf", "epub", "dotx", "dotm", "xltx", "xltm",
            "potx", "potm", "ott", "ots", "otp",
        ],
    ),
];

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Drive,
    Folder,
    File,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Image,
    Video,
    Audio,
    Document,
    Code,
}

pub const CATEGORIES: [Category; 5] = [
    Category::Image,
    Category::Video,
    Category::Audio,
    Category::Document,
    Category::Code,
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Categories(u8);

impl Categories {
    pub const ALL: Self = Self(0b1_1111);
    pub const BEFORE_CODE: Self = Self(0b1111);

    fn bit(category: Category) -> u8 {
        match category {
            Category::Image => 0b0001,
            Category::Video => 0b0010,
            Category::Audio => 0b0100,
            Category::Document => 0b1000,
            Category::Code => 0b1_0000,
        }
    }

    pub fn of(categories: &[Category]) -> Self {
        Self(categories.iter().fold(0, |bits, &category| bits | Self::bit(category)))
    }

    pub fn from_bits(bits: u8) -> Self {
        Self(bits & Self::ALL.0)
    }

    pub fn bits(self) -> u8 {
        self.0
    }

    pub fn contains(self, category: Category) -> bool {
        self.0 & Self::bit(category) != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn includes(self, other: Self) -> bool {
        other.0 & !self.0 == 0
    }

    pub fn to_vec(self) -> Vec<Category> {
        CATEGORIES
            .into_iter()
            .filter(|&category| self.contains(category))
            .collect()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub kind: Kind,
    pub category: Option<Category>,
    pub readable: bool,
    pub has_contents: bool,
    pub index: IndexStatus,
    pub queued: bool,
    pub excluded_here: bool,
    pub removable: bool,
    pub size: Option<u64>,
    pub modified: Option<i64>,
    pub privacy: Option<crate::folder_access::Pane>,
    #[serde(skip)]
    pub cloud: bool,
}

impl Entry {
    fn new(name: String, path: &Path, kind: Kind) -> Self {
        Self {
            name,
            path: path.to_string_lossy().into_owned(),
            kind,
            category: None,
            readable: true,
            has_contents: false,
            index: IndexStatus::NotIndexed,
            queued: false,
            excluded_here: false,
            removable: false,
            size: None,
            modified: None,
            privacy: None,
            cloud: false,
        }
    }

    fn drive(root: &Path) -> Self {
        Self {
            readable: is_network_drive(root) || fs::read_dir(root).is_ok(),
            ..Self::new(display_name(root), root, Kind::Drive)
        }
    }

    fn folder(name: String, path: &Path) -> Self {
        let cloud = crate::cloud::is_cloud_only_path(path);
        let (readable, has_contents) =
            if crate::folder_access::may_open(path) && !unindexable::is_unindexable(path) && !cloud {
                probe(path)
            } else {
                (true, true)
            };
        Self {
            readable,
            has_contents,
            cloud,
            ..Self::new(name, path, Kind::Folder)
        }
    }

    fn file(name: String, path: &Path, category: Category) -> Self {
        Self {
            category: Some(category),
            ..Self::new(name, path, Kind::File)
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crumb {
    pub name: String,
    pub path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub path: String,
    pub parent: Option<String>,
    pub breadcrumbs: Vec<Crumb>,
    pub entries: Vec<Entry>,
}

pub fn list(path: &str) -> io::Result<Listing> {
    let entries = if path.is_empty() {
        root_entries()
    } else {
        read_entries(Path::new(path))?
    };
    Ok(Listing {
        path: path.to_owned(),
        parent: parent(path),
        breadcrumbs: breadcrumbs(path),
        entries,
    })
}

pub fn describe(path: &Path) -> (Entry, bool) {
    let name = display_name(path);
    let (entry, missing) = match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() && !is_package(path) => (
            if path.parent().is_none() {
                Entry::drive(path)
            } else {
                Entry::folder(name, path)
            },
            false,
        ),
        Ok(_) => match category_of(&name) {
            Some(category) => (Entry::file(name, path, category), false),
            None => (Entry::new(name, path, Kind::File), false),
        },
        Err(e) => {
            let entry = if path.parent().is_none() {
                Entry::new(name, path, Kind::Drive)
            } else if let Some(category) = category_of(&name) {
                Entry::file(name, path, category)
            } else {
                Entry {
                    has_contents: true,
                    ..Entry::new(name, path, Kind::Folder)
                }
            };
            (entry, e.kind() == io::ErrorKind::NotFound)
        }
    };
    (
        Entry {
            readable: true,
            ..entry
        },
        missing,
    )
}

#[cfg(windows)]
pub fn roots() -> Vec<PathBuf> {
    let present = unsafe { windows::Win32::Storage::FileSystem::GetLogicalDrives() };
    ('A'..='Z')
        .enumerate()
        .filter(|&(at, _)| present & (1 << at) != 0)
        .map(|(_, letter)| PathBuf::from(format!("{letter}:\\")))
        .filter(|root| is_network_drive(root) || root.exists())
        .collect()
}

#[cfg(windows)]
fn is_network_drive(root: &Path) -> bool {
    const DRIVE_REMOTE: u32 = 4;
    let root = windows::core::HSTRING::from(root.as_os_str());
    unsafe { windows::Win32::Storage::FileSystem::GetDriveTypeW(&root) == DRIVE_REMOTE }
}

#[cfg(target_os = "macos")]
fn is_network_drive(root: &Path) -> bool {
    mounts().iter().any(|mount| mount.network && mount.path == root)
}

#[cfg(target_os = "linux")]
fn is_network_drive(root: &Path) -> bool {
    fs::read_to_string("/proc/self/mounts").is_ok_and(|list| network_mounts_in(&list).iter().any(|point| point == root))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn is_network_drive(_: &Path) -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/")];
    roots.extend(external_volumes());
    roots
}

#[cfg(target_os = "macos")]
fn external_volumes() -> Vec<PathBuf> {
    let mut volumes: Vec<PathBuf> = mounts()
        .into_iter()
        .filter(|mount| !mount.hidden && is_external_volume(&mount.path))
        .map(|mount| mount.path)
        .collect();
    volumes.sort();
    volumes.dedup();
    volumes
}

#[cfg(target_os = "macos")]
struct Mount {
    path: PathBuf,
    network: bool,
    hidden: bool,
}

#[cfg(target_os = "macos")]
fn mounts() -> Vec<Mount> {
    use std::{
        ffi::{CStr, OsStr},
        os::unix::ffi::OsStrExt,
    };
    let known = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
    let Ok(known) = usize::try_from(known) else {
        return Vec::new();
    };
    let mut stats: Vec<libc::statfs> = Vec::with_capacity(known + 4);
    let Ok(bytes) = libc::c_int::try_from(stats.capacity() * std::mem::size_of::<libc::statfs>()) else {
        return Vec::new();
    };
    let filled = unsafe { libc::getfsstat(stats.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
    let Ok(filled) = usize::try_from(filled) else {
        return Vec::new();
    };
    unsafe { stats.set_len(filled.min(stats.capacity())) };
    stats
        .iter()
        .map(|stat| Mount {
            path: PathBuf::from(OsStr::from_bytes(
                unsafe { CStr::from_ptr(stat.f_mntonname.as_ptr()) }.to_bytes(),
            )),
            network: stat.f_flags & libc::MNT_LOCAL as u32 == 0,
            hidden: stat.f_flags & libc::MNT_DONTBROWSE as u32 != 0,
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn startup_disk_name() -> Option<String> {
    fs::read_dir("/Volumes")
        .ok()?
        .flatten()
        .find(|volume| {
            volume.file_type().is_ok_and(|kind| kind.is_symlink())
                && fs::read_link(volume.path()).is_ok_and(|target| target == Path::new("/"))
        })
        .map(|volume| volume.file_name().to_string_lossy().into_owned())
}

#[cfg(target_os = "macos")]
fn is_external_volume(path: &Path) -> bool {
    path.parent() == Some(Path::new("/Volumes"))
}

#[cfg(target_os = "linux")]
pub fn roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/")];
    roots.extend(mounted_disks());
    roots
}

#[cfg(target_os = "linux")]
fn mount_folders() -> Vec<PathBuf> {
    let mut folders = vec![PathBuf::from("/mnt"), PathBuf::from("/media")];
    if let Some(user) = user_name() {
        folders.push(Path::new("/media").join(&user));
        folders.push(Path::new("/run/media").join(&user));
    }
    folders
}

#[cfg(target_os = "linux")]
fn user_name() -> Option<String> {
    std::env::var("USER")
        .ok()
        .filter(|user| !user.is_empty() && !user.contains('/'))
        .or_else(|| {
            std::env::home_dir()?
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
}

#[cfg(target_os = "linux")]
fn mounted_disks() -> Vec<PathBuf> {
    let mut disks: Vec<PathBuf> = mount_points()
        .into_iter()
        .filter(|point| is_placed_as_a_disk(point))
        .collect();
    disks.sort();
    disks.dedup();
    disks
}

#[cfg(target_os = "linux")]
fn is_placed_as_a_disk(path: &Path) -> bool {
    path == Path::new("/mnt")
        || path
            .parent()
            .is_some_and(|parent| mount_folders().iter().any(|folder| folder == parent))
}

#[cfg(target_os = "linux")]
fn mount_points() -> Vec<PathBuf> {
    fs::read_to_string("/proc/self/mounts")
        .map(|list| mount_points_in(&list))
        .unwrap_or_default()
}

#[cfg(any(target_os = "linux", test))]
fn mount_points_in(list: &str) -> Vec<PathBuf> {
    list.lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .map(unescape_mount_point)
        .collect()
}

#[cfg(any(target_os = "linux", test))]
const NETWORK_FILE_SYSTEMS: &[&str] = &[
    "nfs",
    "nfs4",
    "cifs",
    "smb3",
    "smbfs",
    "ncpfs",
    "afs",
    "ceph",
    "glusterfs",
    "lustre",
    "davfs",
    "fuse.sshfs",
    "fuse.rclone",
    "fuse.s3fs",
];

#[cfg(any(target_os = "linux", test))]
fn network_mounts_in(list: &str) -> Vec<PathBuf> {
    list.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace().skip(1);
            let point = fields.next()?;
            let kind = fields.next()?;
            NETWORK_FILE_SYSTEMS
                .contains(&kind)
                .then(|| unescape_mount_point(point))
        })
        .collect()
}

#[cfg(any(target_os = "linux", test))]
pub fn unescape_mount_point(written: &str) -> PathBuf {
    let (mut bytes, raw) = (Vec::with_capacity(written.len()), written.as_bytes());
    let mut at = 0;
    while at < raw.len() {
        let escape = raw
            .get(at + 1..at + 4)
            .filter(|digits| raw[at] == b'\\' && digits.iter().all(|d| (b'0'..=b'7').contains(d)));
        match escape {
            Some(digits) => {
                bytes.push(digits.iter().fold(0u8, |n, d| n.wrapping_mul(8) + (d - b'0')));
                at += 4;
            }
            None => {
                bytes.push(raw[at]);
                at += 1;
            }
        }
    }
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(target_os = "linux")]
fn is_external_volume(path: &Path) -> bool {
    is_placed_as_a_disk(path) && mount_points().iter().any(|point| point == path)
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/")]
}

pub fn user_roots(home: Option<PathBuf>) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = home.into_iter().collect();
    candidates.extend(known_folders());
    outermost(candidates.into_iter().filter(|folder| folder.is_dir()).collect())
}

fn outermost(mut folders: Vec<PathBuf>) -> Vec<PathBuf> {
    folders.sort();
    folders.dedup();
    crate::rules::outermost(&folders).into_iter().cloned().collect()
}

#[cfg(windows)]
fn known_folders() -> Vec<PathBuf> {
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{
            FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures,
            FOLDERID_Videos, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
        },
    };
    [
        FOLDERID_Desktop,
        FOLDERID_Documents,
        FOLDERID_Downloads,
        FOLDERID_Music,
        FOLDERID_Pictures,
        FOLDERID_Videos,
    ]
    .iter()
    .filter_map(|id| unsafe {
        let path = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let folder = path.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(path.0 as *const _));
        folder
    })
    .collect()
}

#[cfg(target_os = "linux")]
fn known_folders() -> Vec<PathBuf> {
    let (Some(home), Some(config)) = (std::env::home_dir(), crate::xdg::base_dir("XDG_CONFIG_HOME", ".config")) else {
        return Vec::new();
    };
    fs::read_to_string(config.join("user-dirs.dirs"))
        .map(|text| user_dirs(&text, &home))
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn user_dirs(text: &str, home: &Path) -> Vec<PathBuf> {
    const NAMES: &[&str] = &[
        "XDG_DESKTOP_DIR",
        "XDG_DOCUMENTS_DIR",
        "XDG_DOWNLOAD_DIR",
        "XDG_MUSIC_DIR",
        "XDG_PICTURES_DIR",
        "XDG_VIDEOS_DIR",
    ];
    text.lines()
        .filter_map(|line| {
            let (name, value) = line.trim().split_once('=')?;
            if !NAMES.contains(&name.trim()) {
                return None;
            }
            let quoted = value.trim().strip_prefix('"')?.strip_suffix('"')?;
            let mut value = String::with_capacity(quoted.len());
            let mut chars = quoted.chars();
            while let Some(c) = chars.next() {
                value.push(if c == '\\' { chars.next()? } else { c });
            }
            match value.strip_prefix("$HOME") {
                Some("") => Some(home.to_owned()),
                Some(rest) => Some(home.join(rest.strip_prefix('/')?)),
                None if value.starts_with('/') => Some(PathBuf::from(value)),
                None => None,
            }
        })
        .collect()
}

#[cfg(not(any(windows, target_os = "linux")))]
fn known_folders() -> Vec<PathBuf> {
    Vec::new()
}

fn root_entries() -> Vec<Entry> {
    roots().iter().map(|root| Entry::drive(root)).collect()
}

pub fn listing_order(entry: &Entry) -> (bool, String) {
    (entry.kind == Kind::File, entry.name.to_lowercase())
}

const PARALLEL_FROM: usize = 2_000;

pub fn map_in_parallel<T: Sync, R: Send>(items: &[T], f: impl Fn(usize, &T) -> R + Sync) -> Vec<R> {
    if items.len() < PARALLEL_FROM {
        return items.iter().enumerate().map(|(at, item)| f(at, item)).collect();
    }
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8);
    let run = items.len().div_ceil(threads);
    let f = &f;
    std::thread::scope(|scope| {
        let runs: Vec<_> = items
            .chunks(run)
            .enumerate()
            .map(|(n, chunk)| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .enumerate()
                        .map(|(at, item)| f(n * run + at, item))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        runs.into_iter()
            .flat_map(|run| run.join().unwrap_or_default())
            .collect()
    })
}

fn read_entries(dir: &Path) -> io::Result<Vec<Entry>> {
    let items: Vec<fs::DirEntry> = fs::read_dir(dir)?.flatten().collect();
    let mut entries: Vec<Entry> = map_in_parallel(&items, |_, item| entry_of(item))
        .into_iter()
        .flatten()
        .collect();
    entries.sort_by_cached_key(listing_order);
    Ok(entries)
}

fn entry_of(item: &fs::DirEntry) -> Option<Entry> {
    let name = item.file_name().into_string().ok()?;
    if name.starts_with('.') {
        return None;
    }
    let file_type = item.file_type().ok()?;
    let followed = if file_type.is_symlink() {
        Some(fs::metadata(item.path()).ok()?)
    } else {
        None
    };
    let is_dir = followed.as_ref().map_or(file_type.is_dir(), fs::Metadata::is_dir);
    let package = is_dir && is_package(&item.path());
    let shown = if is_dir && !package {
        Shown::Folder
    } else {
        Shown::File(category_of(&name).filter(|_| is_what_it_is_named(&item.path()))?)
    };
    let own = item.metadata().ok();
    if is_hidden_entry(&item.path(), &name, || own.clone()) {
        return None;
    }
    let facts = if package {
        indexer::package_facts(&item.path())
    } else {
        followed.as_ref().or(own.as_ref()).and_then(indexer::facts_of)
    };
    let modified = facts.map(|(modified, _)| modified);
    let path = item.path();
    Some(match shown {
        Shown::Folder => Entry {
            modified,
            ..Entry::folder(name, &path)
        },
        Shown::File(category) => Entry {
            modified,
            size: facts.map(|(_, size)| size),
            cloud: crate::cloud::is_remote(&path)
                || followed
                    .as_ref()
                    .or(own.as_ref())
                    .is_some_and(crate::cloud::is_cloud_only),
            ..Entry::file(name, &path, category)
        },
    })
}

pub enum Shown {
    Folder,
    File(Category),
}

pub fn classify(item: &fs::DirEntry) -> Option<(String, Shown)> {
    let name = item.file_name().into_string().ok()?;
    if is_hidden(item, &name) {
        return None;
    }
    let shown = if is_dir(item)? && !is_package(&item.path()) {
        Shown::Folder
    } else {
        Shown::File(category_of_path(&item.path(), &name).filter(|_| is_what_it_is_named(&item.path()))?)
    };
    Some((name, shown))
}

const EMPTY_CHECK_LIMIT: usize = 200;

fn probe(dir: &Path) -> (bool, bool) {
    match fs::read_dir(dir) {
        Err(_) => (false, false),
        Ok(items) => {
            let has_contents = items
                .take(EMPTY_CHECK_LIMIT)
                .flatten()
                .any(|item| classify(&item).is_some());
            (true, has_contents)
        }
    }
}

fn is_dir(item: &fs::DirEntry) -> Option<bool> {
    let file_type = item.file_type().ok()?;
    if file_type.is_symlink() {
        fs::metadata(item.path()).ok().map(|m| m.is_dir())
    } else {
        Some(file_type.is_dir())
    }
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(unused_variables))]
fn is_hidden_entry(path: &Path, name: &str, metadata: impl FnOnce() -> Option<fs::Metadata>) -> bool {
    if name.starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        if let Some(metadata) = metadata() {
            return metadata.file_attributes() & (HIDDEN | SYSTEM) != 0 && !unindexable::is_app_data(path);
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        const UF_HIDDEN: u32 = 0x8000;
        if let Some(metadata) = metadata() {
            return metadata.st_flags() & UF_HIDDEN != 0 && !unindexable::is_app_data(path);
        }
    }
    false
}

#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
pub fn explain(path: &Path, error: &io::Error) -> String {
    #[cfg(target_os = "macos")]
    if error.kind() == io::ErrorKind::PermissionDenied {
        if unindexable::is_unindexable(path) {
            return "macOS keeps this folder private.".into();
        }
        return "macOS isn't letting Mesa read it. Allow Mesa under System Settings \u{203A} Privacy & Security \u{203A} Files and Folders (or Full Disk Access), then try again.".into();
    }
    error.to_string()
}

fn is_hidden(item: &fs::DirEntry, name: &str) -> bool {
    is_hidden_entry(&item.path(), name, || item.metadata().ok())
}

pub fn is_hidden_path(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| is_hidden_entry(path, &name.to_string_lossy(), || fs::symlink_metadata(path).ok()))
}

pub fn is_listed_file(path: &Path) -> bool {
    is_file_like(path)
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| category_of(name).is_some())
        && is_what_it_is_named(path)
}

pub fn is_what_it_is_named(path: &Path) -> bool {
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("key"))
    {
        return true;
    }
    if path.is_dir() {
        return is_package(path);
    }
    if crate::cloud::is_cloud_only_path(path) {
        return true;
    }
    let mut start = [0u8; 4];
    fs::File::open(path)
        .and_then(|mut file| io::Read::read_exact(&mut file, &mut start))
        .is_ok()
        && start == *b"PK\x03\x04"
}

const IWORK: [&str; 3] = ["pages", "numbers", "key"];

pub fn is_package(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| IWORK.iter().any(|known| extension.eq_ignore_ascii_case(known)))
        && path.is_dir()
        && (path.join("Index").is_dir() || path.join("Index.zip").is_file())
}

pub fn package_holding(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|above| is_package(above))
        .map(Path::to_owned)
}

pub fn is_folder(path: &Path) -> bool {
    path.is_dir() && !is_package(path)
}

pub fn is_file_like(path: &Path) -> bool {
    path.is_file() || is_package(path)
}

pub fn package_state(path: &Path) -> Option<(u64, SystemTime)> {
    fn add(folder: &Path, depth: usize, size: &mut u64, newest: &mut SystemTime) -> Option<()> {
        let metadata = fs::symlink_metadata(folder).ok()?;
        *newest = (*newest).max(metadata.modified().ok()?);
        if depth > 8 {
            return Some(());
        }
        for item in fs::read_dir(folder).ok()?.flatten() {
            let Ok(kind) = item.file_type() else { continue };
            if kind.is_dir() {
                add(&item.path(), depth + 1, size, newest)?;
            } else if kind.is_file() {
                let metadata = item.metadata().ok()?;
                *size += metadata.len();
                *newest = (*newest).max(metadata.modified().ok()?);
            }
        }
        Some(())
    }
    let (mut size, mut newest) = (0, SystemTime::UNIX_EPOCH);
    add(path, 0, &mut size, &mut newest)?;
    Some((size, newest))
}

pub fn has_listed_name(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| category_of(&name.to_string_lossy()).is_some())
        && (!path.is_file() || is_what_it_is_named(path))
}

pub fn category_of_path(path: &Path, name: &str) -> Option<Category> {
    if crate::embed::code::is_html_name(name) && crate::code_scope::in_project(path) {
        return Some(Category::Code);
    }
    category_of(name)
}

pub fn category_of(name: &str) -> Option<Category> {
    if crate::embed::code::is_code_name(name) {
        return Some(Category::Code);
    }
    let extension = Path::new(name).extension()?.to_str()?;
    INDEXABLE
        .iter()
        .find(|(_, extensions)| extensions.iter().any(|e| e.eq_ignore_ascii_case(extension)))
        .map(|(category, _)| *category)
}

fn parent(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if is_external_volume(Path::new(path)) {
        return Some(String::new());
    }
    let parent = Path::new(path).parent();
    Some(parent.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default())
}

fn breadcrumbs(path: &str) -> Vec<Crumb> {
    let mut crumbs = vec![Crumb {
        name: COMPUTER_LABEL.to_owned(),
        path: String::new(),
    }];
    if !path.is_empty() {
        let mut chain: Vec<&Path> = Path::new(path)
            .ancestors()
            .filter(|p| !p.as_os_str().is_empty())
            .collect();
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        if let Some(at) = chain.iter().position(|p| is_external_volume(p)) {
            chain.truncate(at + 1);
        }
        chain.reverse();
        crumbs.extend(chain.into_iter().map(|p| Crumb {
            name: display_name(p),
            path: p.to_string_lossy().into_owned(),
        }));
    }
    crumbs
}

#[cfg(target_os = "linux")]
pub const LINUX_ROOT_NAME: &str = "File System";

pub fn display_name(path: &Path) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        #[cfg(target_os = "macos")]
        None if path == Path::new("/") => startup_disk_name().unwrap_or_else(|| "/".to_owned()),
        #[cfg(target_os = "linux")]
        None if path == Path::new("/") => LINUX_ROOT_NAME.to_owned(),
        None => {
            let full = path.to_string_lossy();
            let trimmed = full.trim_end_matches(['\\', '/']);
            if trimmed.is_empty() {
                full.into_owned()
            } else {
                trimmed.to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mesa-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn leaves_out_names_that_arent_text() {
        let dir = temp_dir("not-text");
        fs::write(dir.join("fine.png"), "").unwrap();
        #[cfg(windows)]
        let not_text = |name: &[u16]| {
            use std::os::windows::ffi::OsStringExt;
            std::ffi::OsString::from_wide(name)
        };
        #[cfg(windows)]
        let (file, folder) = (
            not_text(&[0x62, 0xD800, 0x2E, 0x70, 0x6E, 0x67]),
            not_text(&[0x64, 0xD800]),
        );
        #[cfg(unix)]
        let (file, folder) = {
            use std::os::unix::ffi::OsStrExt;
            (
                std::ffi::OsStr::from_bytes(b"b\xff.png").to_owned(),
                std::ffi::OsStr::from_bytes(b"d\xff").to_owned(),
            )
        };
        fs::write(dir.join(&file), "").unwrap();
        fs::create_dir(dir.join(&folder)).unwrap();
        fs::write(dir.join(&folder).join("inside.png"), "").unwrap();
        let listing = list(dir.to_str().unwrap()).unwrap();
        let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["fine.png"]);
        let indexed: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter_map(|item| classify(&item).map(|(name, _)| name))
            .collect();
        assert_eq!(indexed, ["fine.png"]);
    }

    #[test]
    fn folders_first_then_case_insensitive_name() {
        let dir = temp_dir("sort");
        fs::create_dir(dir.join("zeta")).unwrap();
        fs::create_dir(dir.join("Alpha")).unwrap();
        fs::write(dir.join("b.png"), "").unwrap();
        fs::write(dir.join("A.png"), "").unwrap();

        let listing = list(dir.to_str().unwrap()).unwrap();
        let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "zeta", "A.png", "b.png"]);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parses_escaped_mount_points() {
        let list = "/dev/nvme0n1p2 / ext4 rw,relatime 0 0\n\
            /dev/sda1 /media/me/USB\\040Stick vfat rw 0 0\n\
            server:/share /mnt/back\\134slash nfs rw 0 0\n";
        assert_eq!(
            mount_points_in(list),
            [
                PathBuf::from("/"),
                PathBuf::from("/media/me/USB Stick"),
                PathBuf::from("/mnt/back\\slash")
            ],
        );
    }

    #[test]
    fn finds_network_shares_in_the_mount_list() {
        let list = "/dev/nvme0n1p2 / ext4 rw,relatime 0 0\n\
            /dev/sda1 /media/me/USB\\040Stick vfat rw 0 0\n\
            server:/share /mnt/back\\134slash nfs rw 0 0\n\
            //nas/photos /media/me/Photos cifs rw 0 0\n\
            me@host:/ /mnt/remote fuse.sshfs rw 0 0\n";
        assert_eq!(
            network_mounts_in(list),
            [
                PathBuf::from("/mnt/back\\slash"),
                PathBuf::from("/media/me/Photos"),
                PathBuf::from("/mnt/remote")
            ],
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_startup_disk_is_local_and_not_an_external_volume() {
        assert!(
            mounts()
                .iter()
                .any(|mount| mount.path == Path::new("/") && !mount.network)
        );
        assert!(!is_network_drive(Path::new("/")));
        assert!(
            external_volumes()
                .iter()
                .all(|volume| volume.parent() == Some(Path::new("/Volumes")))
        );
        assert!(startup_disk_name().is_some());
    }

    #[test]
    fn breadcrumbs_from_top_level() {
        let dir = temp_dir("crumbs");
        let path = dir.to_string_lossy().into_owned();

        let crumbs = breadcrumbs(&path);
        assert_eq!(crumbs.first().unwrap().path, "");
        assert_eq!(crumbs.first().unwrap().name, COMPUTER_LABEL);
        assert_eq!(crumbs.last().unwrap().path, path);
        assert_eq!(crumbs.last().unwrap().name, dir.file_name().unwrap().to_string_lossy());
        for pair in crumbs[1..].windows(2) {
            assert_eq!(Path::new(&pair[1].path).parent(), Some(Path::new(&pair[0].path)));
        }

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn extension_match_ignores_case() {
        assert_eq!(category_of("A.PnG"), Some(Category::Image));
        assert_eq!(category_of("clip.mkv"), Some(Category::Video));
        assert_eq!(category_of("notes.md"), Some(Category::Document));
        assert_eq!(category_of("Budget.XLSX"), Some(Category::Document));
        assert_eq!(category_of("scan.tif"), Some(Category::Image));
        assert_eq!(category_of("book.m4b"), Some(Category::Audio));
        assert_eq!(category_of("archive.zip"), None);
    }

    #[cfg(windows)]
    #[test]
    fn leaves_cloud_only_folders_unlisted() {
        use windows::{
            Win32::Storage::FileSystem::{FILE_ATTRIBUTE_OFFLINE, SetFileAttributesW},
            core::HSTRING,
        };
        let dir = temp_dir("cloud-folders");
        fs::create_dir(dir.join("here")).unwrap();
        fs::create_dir(dir.join("away")).unwrap();
        unsafe { SetFileAttributesW(&HSTRING::from(dir.join("away").as_path()), FILE_ATTRIBUTE_OFFLINE) }.unwrap();
        let listing = list(&dir.to_string_lossy()).unwrap();
        let entry = |name: &str| listing.entries.iter().find(|entry| entry.name == name).unwrap();
        assert!(entry("away").cloud && entry("away").readable && entry("away").has_contents);
        assert!(!entry("here").cloud && !entry("here").has_contents);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn marks_cloud_only_files() {
        use windows::{
            Win32::Storage::FileSystem::{FILE_ATTRIBUTE_OFFLINE, SetFileAttributesW},
            core::HSTRING,
        };
        let dir = temp_dir("cloud-listing");
        fs::write(dir.join("here.png"), "x").unwrap();
        fs::write(dir.join("away.png"), "x").unwrap();
        unsafe { SetFileAttributesW(&HSTRING::from(dir.join("away.png").as_path()), FILE_ATTRIBUTE_OFFLINE) }.unwrap();
        let listing = list(&dir.to_string_lossy()).unwrap();
        let cloud = |name: &str| listing.entries.iter().find(|entry| entry.name == name).unwrap().cloud;
        assert!(cloud("away.png"));
        assert!(!cloud("here.png"));
        let _ = fs::remove_dir_all(&dir);
    }
}
