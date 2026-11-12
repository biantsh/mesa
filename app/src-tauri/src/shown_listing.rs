use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use serde::Serialize;

use crate::{
    browse::{self, Category, Crumb, Entry, Kind, Listing},
    folder_access::{FolderAccess, Pane},
    index::{Index, IndexState, IndexStatus},
};

#[derive(Clone, PartialEq, Debug)]
struct Row {
    name: Box<str>,
    kind: Kind,
    category: Option<Category>,
    readable: bool,
    has_contents: bool,
    size: Option<u64>,
    modified: Option<i64>,
    privacy: Option<Pane>,
    state: IndexState,
}

impl Row {
    fn of(entry: &Entry) -> Self {
        Self {
            name: entry.name.as_str().into(),
            kind: entry.kind,
            category: entry.category,
            readable: entry.readable,
            has_contents: entry.has_contents,
            size: entry.size,
            modified: entry.modified,
            privacy: entry.privacy,
            state: IndexState {
                status: entry.index,
                queued: entry.queued,
                excluded_here: entry.excluded_here,
                removable: entry.removable,
            },
        }
    }
}

struct Held {
    generation: u64,
    shape: u64,
    path: String,
    prefix: Option<String>,
    paths: Option<Vec<String>>,
    rows: Vec<Row>,
    looked: Option<crate::index::FolderStates>,
}

impl Held {
    fn path_of(&self, at: usize) -> String {
        match (&self.prefix, &self.paths) {
            (Some(prefix), _) => format!("{prefix}{}", self.rows[at].name),
            (None, Some(paths)) => paths[at].clone(),
            (None, None) => unreachable!("a held listing has a prefix or the paths"),
        }
    }
}

#[derive(Default)]
pub struct ShownListing {
    held: Mutex<Option<Held>>,
    generations: AtomicU64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Said {
    generation: u64,
    shape: u64,
    path: String,
    parent: Option<String>,
    breadcrumbs: Vec<Crumb>,
    entries: Columns,
}

#[derive(Serialize, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Columns {
    names: Vec<String>,
    prefix: Option<String>,
    paths: Option<Vec<String>>,
    kinds: String,
    categories: String,
    statuses: String,
    flags: String,
    sizes: Vec<Option<u64>>,
    modified: Vec<Option<i64>>,
    privacy: Vec<(u32, Pane)>,
    legend: Legend,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Legend {
    kinds: Vec<String>,
    categories: Vec<String>,
    statuses: Vec<String>,
}

impl Default for Legend {
    fn default() -> Self {
        let names = |all: &[serde_json::Value]| {
            all.iter()
                .map(|value| value.as_str().unwrap_or_default().to_owned())
                .collect()
        };
        Self {
            kinds: names(&KINDS.map(|kind| serde_json::to_value(kind).unwrap_or_default())),
            categories: names(&CATEGORIES.map(|category| serde_json::to_value(category).unwrap_or_default())),
            statuses: names(&STATUSES.map(|status| serde_json::to_value(status).unwrap_or_default())),
        }
    }
}

const LETTERS: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";
const KINDS: [Kind; 3] = [Kind::Drive, Kind::Folder, Kind::File];
const CATEGORIES: [Category; 4] = browse::CATEGORIES;
const STATUSES: [IndexStatus; 8] = [
    IndexStatus::NotIndexed,
    IndexStatus::Indexing,
    IndexStatus::Paused,
    IndexStatus::Indexed,
    IndexStatus::PartlyIndexed,
    IndexStatus::Unindexable,
    IndexStatus::Excluded,
    IndexStatus::TypeNotIndexed,
];
const FLAG_READABLE: u8 = 1;
const FLAG_HAS_CONTENTS: u8 = 2;
const FLAG_QUEUED: u8 = 4;
const FLAG_EXCLUDED_HERE: u8 = 8;
const FLAG_REMOVABLE: u8 = 16;

fn status_code(status: IndexStatus) -> usize {
    match status {
        IndexStatus::NotIndexed => 0,
        IndexStatus::Indexing => 1,
        IndexStatus::Paused => 2,
        IndexStatus::Indexed => 3,
        IndexStatus::PartlyIndexed => 4,
        IndexStatus::Unindexable => 5,
        IndexStatus::Excluded => 6,
        IndexStatus::TypeNotIndexed => 7,
    }
}

fn letter<T: PartialEq>(all: &[T], it: &T) -> char {
    char::from(LETTERS[all.iter().position(|one| one == it).unwrap_or(all.len())])
}

impl Columns {
    fn of(rows: &[&Row], prefix: Option<String>, paths: Option<Vec<String>>) -> Self {
        let mut columns = Self {
            prefix,
            paths,
            ..Self::default()
        };
        for (at, row) in rows.iter().enumerate() {
            columns.names.push(row.name.to_string());
            columns.kinds.push(letter(&KINDS, &row.kind));
            columns
                .categories
                .push(row.category.map_or(char::from(LETTERS[CATEGORIES.len()]), |category| {
                    letter(&CATEGORIES, &category)
                }));
            columns
                .statuses
                .push(char::from(LETTERS[status_code(row.state.status)]));
            let flags = [
                (row.readable, FLAG_READABLE),
                (row.has_contents, FLAG_HAS_CONTENTS),
                (row.state.queued, FLAG_QUEUED),
                (row.state.excluded_here, FLAG_EXCLUDED_HERE),
                (row.state.removable, FLAG_REMOVABLE),
            ]
            .into_iter()
            .filter(|(set, _)| *set)
            .fold(0, |all, (_, flag)| all | flag);
            columns.flags.push(char::from(LETTERS[usize::from(flags)]));
            columns.sizes.push(row.size);
            columns.modified.push(row.modified);
            if let Some(pane) = row.privacy {
                columns.privacy.push((at as u32, pane));
            }
        }
        columns
    }
}

pub fn columns_of(entries: &[&Entry]) -> Columns {
    let rows: Vec<Row> = entries.iter().map(|entry| Row::of(entry)).collect();
    let (prefix, paths) = prefix_or_paths(entries, |entry| &entry.name, |entry| &entry.path);
    Columns::of(&rows.iter().collect::<Vec<_>>(), prefix, paths)
}

fn prefix_or_paths<T>(
    items: &[T],
    name: impl Fn(&T) -> &str,
    path: impl Fn(&T) -> &str,
) -> (Option<String>, Option<Vec<String>>) {
    let prefix = items
        .first()
        .and_then(|first| path(first).strip_suffix(name(first)))
        .map(str::to_owned);
    let prefix = prefix.filter(|prefix| {
        items.iter().all(|item| {
            let (name, path) = (name(item), path(item));
            path.len() == prefix.len() + name.len() && path.starts_with(prefix.as_str()) && path.ends_with(name)
        })
    });
    if prefix.is_some() || items.is_empty() {
        return (prefix, None);
    }
    (None, Some(items.iter().map(|item| path(item).to_owned()).collect()))
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Statuses {
    stale: bool,
    base: u64,
    generation: u64,
    changes: Vec<(u32, IndexStatus, bool, bool, bool)>,
    folder: Option<IndexState>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Refreshed {
    Whole(Said),
    Changes {
        base: u64,
        generation: u64,
        shape: u64,
        runs: Vec<Run>,
        fresh: Columns,
        parent: Option<String>,
        breadcrumbs: Vec<Crumb>,
    },
}

#[derive(Serialize, Debug, PartialEq, Clone, Copy)]
#[serde(untagged)]
pub enum Run {
    Keep([u32; 2]),
    Fresh(u32),
}

impl ShownListing {
    fn lock(&self) -> MutexGuard<'_, Option<Held>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn next_generation(&self) -> u64 {
        self.generations.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn read(&self, path: &str, index: &Index, access: &FolderAccess) -> Result<Said, String> {
        let listing = browse::list(path).map_err(|e| browse::explain(&e))?;
        let (rows, prefix, paths) = rows_of(&listing, index, access);
        let generation = self.next_generation();
        let said = Said {
            generation,
            shape: generation,
            path: listing.path.clone(),
            parent: listing.parent.clone(),
            breadcrumbs: listing.breadcrumbs,
            entries: Columns::of(&rows.iter().collect::<Vec<_>>(), prefix.clone(), paths.clone()),
        };
        *self.lock() = Some(Held {
            generation,
            shape: generation,
            path: listing.path,
            prefix,
            paths,
            rows,
            looked: None,
        });
        Ok(said)
    }

    pub fn refresh(&self, path: &str, base: u64, index: &Index, access: &FolderAccess) -> Result<Refreshed, String> {
        let listing = browse::list(path).map_err(|e| browse::explain(&e))?;
        let (rows, prefix, paths) = rows_of(&listing, index, access);
        let mut held = self.lock();
        let Some(was) = held
            .as_ref()
            .filter(|held| held.generation == base && held.path == listing.path && held.prefix == prefix)
        else {
            drop(held);
            return self.read(path, index, access).map(Refreshed::Whole);
        };
        let (runs, fresh_at, same_shape) = changes(&was.rows, &rows, was.paths.as_deref(), paths.as_deref());
        let generation = self.next_generation();
        let shape = if same_shape { was.shape } else { generation };
        let fresh_rows: Vec<&Row> = fresh_at.iter().map(|&at| &rows[at]).collect();
        let fresh_paths = paths
            .as_ref()
            .filter(|_| !fresh_at.is_empty())
            .map(|paths| fresh_at.iter().map(|&at| paths[at].clone()).collect());
        let fresh = Columns::of(
            &fresh_rows,
            prefix.clone().filter(|_| !fresh_at.is_empty()),
            fresh_paths,
        );
        *held = Some(Held {
            generation,
            shape,
            path: listing.path,
            prefix,
            paths,
            rows,
            looked: None,
        });
        Ok(Refreshed::Changes {
            base,
            generation,
            shape,
            runs,
            fresh,
            parent: listing.parent,
            breadcrumbs: listing.breadcrumbs,
        })
    }

    pub fn statuses(&self, generation: u64, index: &Index) -> Statuses {
        let stale = || Statuses {
            stale: true,
            base: generation,
            generation,
            changes: Vec::new(),
            folder: None,
        };
        let (path, folder) = {
            let held = self.lock();
            let Some(held) = held.as_ref().filter(|held| held.generation == generation) else {
                return stale();
            };
            let folder = held
                .prefix
                .as_ref()
                .filter(|_| !held.rows.is_empty())
                .and_then(|_| PathBuf::from(held.path_of(0)).parent().map(Path::to_owned));
            (held.path.clone(), folder)
        };
        let own = (!path.is_empty()).then(|| index.state(Path::new(&path)));
        let looked = folder.map(|folder| (index.folder_states(&folder), folder));
        let mut held = self.lock();
        let Some(held) = held.as_mut().filter(|held| held.generation == generation) else {
            return stale();
        };
        let unchanged = looked
            .as_ref()
            .is_some_and(|(states, _)| held.looked.as_ref() == Some(states));
        let mut changes = Vec::new();
        if !unchanged {
            let states: Vec<IndexState> = match &looked {
                Some((states, _)) => (0..held.rows.len())
                    .map(|at| states.state(Path::new(&held.path_of(at)), Some(held.rows[at].kind == Kind::File)))
                    .collect(),
                None => {
                    let paths: Vec<PathBuf> = (0..held.rows.len()).map(|at| PathBuf::from(held.path_of(at))).collect();
                    let asked: Vec<(&Path, Option<bool>)> = paths
                        .iter()
                        .zip(&held.rows)
                        .map(|(path, row)| (path.as_path(), Some(row.kind == Kind::File)))
                        .collect();
                    index.states_of(&asked)
                }
            };
            for (at, (row, state)) in held.rows.iter_mut().zip(states).enumerate() {
                if row.state != state {
                    row.state = state;
                    changes.push((
                        at as u32,
                        state.status,
                        state.queued,
                        state.excluded_here,
                        state.removable,
                    ));
                }
            }
            held.looked = looked.map(|(states, _)| states);
        }
        if !changes.is_empty() {
            held.generation = self.next_generation();
        }
        Statuses {
            stale: false,
            base: generation,
            generation: held.generation,
            changes,
            folder: own,
        }
    }

    pub fn previews(&self, shape: u64, order: Option<&[u32]>) -> Option<crate::thumbnail::PreviewEntries> {
        let held = self.lock();
        let held = held.as_ref().filter(|held| held.shape == shape)?;
        let preview = |at: usize| {
            let row = held.rows.get(at)?;
            let wanted = row.kind == Kind::File
                && row.readable
                && row.category.is_some_and(|category| category != Category::Audio);
            wanted.then(|| (held.path_of(at), row.category == Some(Category::Document)))
        };
        Some(match order {
            Some(order) => order.iter().map(|&at| preview(at as usize)).collect(),
            None => (0..held.rows.len()).map(preview).collect(),
        })
    }
}

fn rows_of(listing: &Listing, index: &Index, access: &FolderAccess) -> (Vec<Row>, Option<String>, Option<Vec<String>>) {
    let asked: Vec<(&Path, Option<bool>)> = listing
        .entries
        .iter()
        .map(|entry| (Path::new(&entry.path), Some(entry.kind == Kind::File)))
        .collect();
    let states = index.states_of(&asked);
    let rows: Vec<Row> = listing
        .entries
        .iter()
        .zip(states)
        .map(|(entry, state)| Row {
            state,
            privacy: if entry.readable {
                None
            } else {
                access.pane_for(Path::new(&entry.path))
            },
            ..Row::of(entry)
        })
        .collect();
    let (prefix, paths) = prefix_or_paths(&listing.entries, |entry| &entry.name, |entry| &entry.path);
    (rows, prefix, paths)
}

fn changes(
    was: &[Row],
    now: &[Row],
    was_paths: Option<&[String]>,
    now_paths: Option<&[String]>,
) -> (Vec<Run>, Vec<usize>, bool) {
    let key = |rows: &[Row], paths: Option<&[String]>, at: usize| -> String {
        paths.map_or_else(|| rows[at].name.to_string(), |paths| paths[at].clone())
    };
    let places: std::collections::HashMap<String, usize> =
        (0..was.len()).map(|at| (key(was, was_paths, at), at)).collect();
    let mut runs: Vec<Run> = Vec::new();
    let mut fresh = Vec::new();
    let mut same_shape = was.len() == now.len();
    for at in 0..now.len() {
        let name = key(now, now_paths, at);
        let kept = places.get(&name).copied();
        same_shape &= kept == Some(at) && was[at].kind == now[at].kind && was[at].readable == now[at].readable;
        match kept.filter(|&from| was[from] == now[at]) {
            Some(from) => match runs.last_mut() {
                Some(Run::Keep([start, count])) if *start as usize + *count as usize == from => *count += 1,
                _ => runs.push(Run::Keep([from as u32, 1])),
            },
            None => {
                fresh.push(at);
                match runs.last_mut() {
                    Some(Run::Fresh(count)) => *count += 1,
                    _ => runs.push(Run::Fresh(1)),
                }
            }
        }
    }
    (runs, fresh, same_shape)
}
