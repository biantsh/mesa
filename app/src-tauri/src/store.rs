//! Vectors are written and synced before the transaction that refers to them commits, so a crash can leave
//! unreferenced vectors at the end of the file but never a row pointing at a missing one. Opening trims the leftovers.

use std::{
    collections::{HashMap, HashSet},
    fmt, fs, io, mem,
    path::{Component, MAIN_SEPARATOR, Path, PathBuf},
};

use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter, types::Value};

use crate::{
    browse::Category,
    embed,
    vectors::{DIMS, Located, Record, VectorFile, similarity, to_floats},
};

const CONFIG_ID: u32 = 3;
const MIGRATIONS: &[&str] = &[SCHEMA, ADD_META, ADD_EMBEDDED_WITH, ADD_LOCATOR, ADD_EXCLUSIONS];

const SCHEMA: &str = "
CREATE TABLE dirs (
    id        INTEGER PRIMARY KEY,
    parent_id INTEGER NOT NULL REFERENCES dirs(id) ON DELETE CASCADE,
    name      TEXT NOT NULL,
    UNIQUE (parent_id, name)
);
-- Drives and `/` hang off this folder, which stands for the top level.
INSERT INTO dirs (id, parent_id, name) VALUES (0, 0, '');

CREATE TABLE files (
    id          INTEGER PRIMARY KEY,
    dir_id      INTEGER NOT NULL REFERENCES dirs(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    category    INTEGER NOT NULL,  -- see `category_code`
    modified_ms INTEGER NOT NULL,  -- Unix time in milliseconds
    size        INTEGER NOT NULL,  -- bytes
    UNIQUE (dir_id, name)
);
CREATE INDEX files_by_category_and_date ON files (category, modified_ms);

CREATE TABLE embedding_configs (
    id    INTEGER PRIMARY KEY,
    model TEXT NOT NULL,
    dims  INTEGER NOT NULL,
    dtype TEXT NOT NULL
);

-- A file can have several vectors (a video's frames, a document's pages), hence a table of its own.
CREATE TABLE embeddings (
    config_id INTEGER NOT NULL REFERENCES embedding_configs(id),
    slot      INTEGER NOT NULL,
    file_id   INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    PRIMARY KEY (config_id, slot)
) WITHOUT ROWID;
CREATE INDEX embeddings_by_file ON embeddings (file_id);

-- Slots in the vector file that nothing refers to any more, to be reused before the file grows.
CREATE TABLE free_slots (
    config_id INTEGER NOT NULL,
    slot      INTEGER NOT NULL,
    PRIMARY KEY (config_id, slot)
) WITHOUT ROWID;
CREATE TRIGGER embeddings_free_slot AFTER DELETE ON embeddings
BEGIN
    INSERT INTO free_slots (config_id, slot) VALUES (old.config_id, old.slot);
END;

CREATE TABLE rules (path TEXT PRIMARY KEY, indexed INTEGER NOT NULL) WITHOUT ROWID;

-- Indexing that was asked for and hasn't finished, so it can carry on after a restart.
CREATE TABLE jobs (id INTEGER PRIMARY KEY, path TEXT NOT NULL);
";

const ADD_META: &str = "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;";
const ADD_EMBEDDED_WITH: &str = "ALTER TABLE files ADD COLUMN embedded_with INTEGER NOT NULL DEFAULT 0;";
const ADD_LOCATOR: &str = "ALTER TABLE embeddings ADD COLUMN locator INTEGER NOT NULL DEFAULT 0;";
const ADD_EXCLUSIONS: &str = "CREATE TABLE exclusions (path TEXT PRIMARY KEY) WITHOUT ROWID;";

#[derive(Debug)]
pub enum Error {
    Db(rusqlite::Error),
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Db(e) => write!(f, "database error: {e}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Db(e)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<Error> for String {
    fn from(e: Error) -> Self {
        e.to_string()
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct ScannedFile {
    pub name: String,
    pub category: Category,
    pub modified_ms: i64,
    pub size: u64,
    pub vectors: Vec<Located>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    pub category: Option<Category>,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub file: i64,
    pub path: PathBuf,
    pub folder: String,
    pub category: Category,
    pub modified_ms: i64,
    pub size: u64,
    pub score: f32,
}

pub struct ScannedDir {
    pub path: PathBuf,
    pub files: Vec<ScannedFile>,
    pub complete: bool,
    pub subfolders: Option<Vec<String>>,
}

pub struct Store {
    dir: PathBuf,
    conn: Connection,
    vectors: VectorFile,
}

impl Store {
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)?;
        let conn = open_db(dir)?;
        remove_other_vector_files(dir);
        let mut vectors = VectorFile::open(&vectors_path(dir), CONFIG_ID)?;
        let expected = extent(&conn)?;
        if expected > vectors.next_slot() {
            eprintln!(
                "Mesa: the vector file has {} vectors and the index refers to {expected}, so the vectors are lost and will be made again",
                vectors.next_slot()
            );
            conn.execute("DELETE FROM embeddings WHERE config_id = ?1", [CONFIG_ID])?;
            conn.execute("DELETE FROM free_slots WHERE config_id = ?1", [CONFIG_ID])?;
            vectors.truncate_to(0)?;
        } else {
            vectors.truncate_to(expected)?;
        }
        Ok(Self {
            dir: dir.to_owned(),
            conn,
            vectors,
        })
    }

    pub fn reset(&mut self) -> Result<()> {
        self.vectors.truncate_to(0)?;
        self.vectors.sync()?;
        let closed = mem::replace(&mut self.conn, Connection::open_in_memory()?);
        closed.close().map_err(|(_, e)| e)?;
        for name in ["mesa.db", "mesa.db-wal", "mesa.db-shm"] {
            match fs::remove_file(self.dir.join(name)) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        self.conn = open_db(&self.dir)?;
        Ok(())
    }

    pub fn load_rules(&self) -> Result<HashMap<PathBuf, bool>> {
        let mut statement = self.conn.prepare("SELECT path, indexed FROM rules")?;
        let rows = statement.query_map([], |r| Ok((PathBuf::from(r.get::<_, String>(0)?), r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn save_rules(&mut self, rules: &HashMap<PathBuf, bool>) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM rules", [])?;
        {
            let mut insert = tx.prepare("INSERT INTO rules (path, indexed) VALUES (?1, ?2)")?;
            for (path, indexed) in rules {
                insert.execute(params![path.to_string_lossy(), indexed])?;
            }
        }
        Ok(tx.commit()?)
    }

    pub fn load_exclusions(&self) -> Result<HashSet<PathBuf>> {
        let mut statement = self.conn.prepare("SELECT path FROM exclusions")?;
        let rows = statement.query_map([], |r| Ok(PathBuf::from(r.get::<_, String>(0)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn save_exclusions(&mut self, exclusions: &HashSet<PathBuf>) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM exclusions", [])?;
        {
            let mut insert = tx.prepare("INSERT INTO exclusions (path) VALUES (?1)")?;
            for path in exclusions {
                insert.execute(params![path.to_string_lossy()])?;
            }
        }
        Ok(tx.commit()?)
    }

    pub fn is_paused(&self) -> Result<bool> {
        let paused: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'paused'", [], |r| r.get(0))
            .optional()?;
        Ok(paused.is_some())
    }

    pub fn set_paused(&self, paused: bool) -> Result<()> {
        if paused {
            self.conn
                .execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('paused', '1')", [])?;
        } else {
            self.conn.execute("DELETE FROM meta WHERE key = 'paused'", [])?;
        }
        Ok(())
    }

    pub fn add_job(&self, path: &Path) -> Result<i64> {
        self.conn
            .execute("INSERT INTO jobs (path) VALUES (?1)", [path.to_string_lossy()])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn add_jobs(&self, paths: &[PathBuf]) -> Result<Vec<i64>> {
        let tx = self.conn.unchecked_transaction()?;
        let mut ids = Vec::with_capacity(paths.len());
        {
            let mut insert = tx.prepare_cached("INSERT INTO jobs (path) VALUES (?1)")?;
            for path in paths {
                insert.execute([path.to_string_lossy()])?;
                ids.push(tx.last_insert_rowid());
            }
        }
        tx.commit()?;
        Ok(ids)
    }

    pub fn finish_job(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM jobs WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn finish_jobs(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut delete = tx.prepare_cached("DELETE FROM jobs WHERE id = ?1")?;
            for id in ids {
                delete.execute([id])?;
            }
        }
        Ok(tx.commit()?)
    }

    pub fn unfinished_jobs(&self) -> Result<Vec<(i64, PathBuf)>> {
        let mut statement = self.conn.prepare("SELECT id, path FROM jobs ORDER BY id")?;
        let rows = statement.query_map([], |r| Ok((r.get(0)?, PathBuf::from(r.get::<_, String>(1)?))))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn write_scans(&mut self, scans: &[ScannedDir], wanted: &dyn Fn(&Path) -> bool) -> Result<()> {
        let result = self.write_scans_inner(scans, wanted);
        if result.is_err() {
            if let Ok(slots) = extent(&self.conn) {
                let _ = self.vectors.truncate_to(slots);
            }
        }
        result
    }

    fn write_scans_inner(&mut self, scans: &[ScannedDir], wanted: &dyn Fn(&Path) -> bool) -> Result<()> {
        let tx = self.conn.transaction()?;
        let mut next_slot = self.vectors.next_slot();
        let mut writes: Vec<(u64, Record)> = Vec::new();

        for scan in scans {
            let mut dir_id = find_dir(&tx, &scan.path)?;
            let existing = match dir_id {
                Some(id) => existing_files(&tx, id)?,
                None => HashMap::new(),
            };
            let mut seen = HashSet::with_capacity(scan.files.len());

            for file in &scan.files {
                seen.insert(file.name.as_str());
                let old = existing.get(&file.name);
                if old.is_some_and(|old| is_current(old, file)) {
                    continue;
                }
                if !wanted(&scan.path.join(&file.name)) {
                    continue;
                }
                let dir = match dir_id {
                    Some(id) => id,
                    None => *dir_id.insert(ensure_dir(&tx, &scan.path)?),
                };
                let file_id = upsert_file(&tx, dir, file)?;
                if old.is_some() {
                    tx.prepare_cached("DELETE FROM embeddings WHERE config_id = ?1 AND file_id = ?2")?
                        .execute(params![CONFIG_ID, file_id])?;
                }
                for vector in &file.vectors {
                    let slot = take_slot(&tx, &mut next_slot)?;
                    tx.prepare_cached(
                        "INSERT INTO embeddings (config_id, slot, file_id, locator) VALUES (?1, ?2, ?3, ?4)",
                    )?
                    .execute(params![CONFIG_ID, slot as i64, file_id, vector.locator as i64])?;
                    writes.push((slot, vector.record));
                }
            }

            if scan.complete {
                for (name, old) in &existing {
                    if !seen.contains(name.as_str()) {
                        tx.prepare_cached("DELETE FROM files WHERE id = ?1")?
                            .execute([old.id])?;
                    }
                }
                if let (Some(dir), Some(present)) = (dir_id, &scan.subfolders) {
                    forget_missing_folders(&tx, dir, present)?;
                }
            }
        }

        self.vectors.write(&mut writes)?;
        self.vectors.sync()?;
        Ok(tx.commit()?)
    }

    pub fn recover_lost_vectors(&self) -> Result<bool> {
        let lost: bool = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM files) AND NOT EXISTS (SELECT 1 FROM embeddings WHERE config_id = ?1)",
            [CONFIG_ID],
            |r| r.get(0),
        )?;
        if lost {
            self.conn.execute("UPDATE files SET embedded_with = 0", [])?;
        }
        Ok(lost)
    }

    pub fn has_stale_files(&self) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM files WHERE embedded_with != ?1)",
            [CONFIG_ID],
            |r| r.get(0),
        )?)
    }

    pub fn changes(&self, scan: &ScannedDir) -> Result<Changes> {
        let existing = match find_dir(&self.conn, &scan.path)? {
            Some(dir) => existing_files(&self.conn, dir)?,
            None => HashMap::new(),
        };
        let mut changes = Changes::default();
        for file in &scan.files {
            match existing.get(&file.name) {
                Some(old) if is_current(old, file) => {}
                Some(_) => {
                    changes.changed.insert(file.name.clone());
                }
                None => {
                    changes.changed.insert(file.name.clone());
                    changes.new.insert(file.name.clone());
                }
            }
        }
        Ok(changes)
    }

    #[cfg(test)]
    pub fn remove_path(&mut self, path: &Path) -> Result<()> {
        self.remove_paths(std::slice::from_ref(&path.to_path_buf()))
    }

    pub fn rename_path(&mut self, from: &Path, to: &Path) -> Result<bool> {
        let (Some(to_parent), Some(to_name)) = (to.parent(), to.file_name()) else {
            return Ok(false);
        };
        let to_name = to_name.to_string_lossy().into_owned();
        let tx = self.conn.transaction()?;

        if let Some(id) = find_dir(&tx, from)? {
            let parent = ensure_dir(&tx, to_parent)?;
            if let Some(stale) = child_dir(&tx, parent, &to_name)?
                && stale != id
            {
                tx.execute("DELETE FROM dirs WHERE id = ?1", [stale])?;
            }
            tx.execute(
                "UPDATE dirs SET parent_id = ?1, name = ?2 WHERE id = ?3",
                params![parent, to_name, id],
            )?;
            tx.commit()?;
            return Ok(true);
        }

        let (Some(from_parent), Some(from_name)) = (from.parent(), from.file_name()) else {
            return Ok(false);
        };
        let Some(from_dir) = find_dir(&tx, from_parent)? else {
            return Ok(false);
        };
        let file: Option<i64> = tx
            .query_row(
                "SELECT id FROM files WHERE dir_id = ?1 AND name = ?2",
                params![from_dir, from_name.to_string_lossy()],
                |r| r.get(0),
            )
            .optional()?;
        let Some(file) = file else { return Ok(false) };
        match crate::browse::category_of(&to_name) {
            Some(category) => {
                let dir = ensure_dir(&tx, to_parent)?;
                tx.execute(
                    "DELETE FROM files WHERE dir_id = ?1 AND name = ?2 AND id != ?3",
                    params![dir, to_name, file],
                )?;
                tx.execute(
                    "UPDATE files SET dir_id = ?1, name = ?2, category = ?3 WHERE id = ?4",
                    params![dir, to_name, category_code(category), file],
                )?;
            }
            None => {
                tx.execute("DELETE FROM files WHERE id = ?1", [file])?;
            }
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn remove_paths(&mut self, paths: &[PathBuf]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for path in paths {
            if let Some(id) = find_dir(&tx, path)? {
                tx.execute("DELETE FROM dirs WHERE id = ?1", [id])?;
            }
            if let (Some(parent), Some(name)) = (path.parent(), path.file_name())
                && let Some(dir) = find_dir(&tx, parent)?
            {
                tx.execute(
                    "DELETE FROM files WHERE dir_id = ?1 AND name = ?2",
                    params![dir, name.to_string_lossy()],
                )?;
            }
        }
        Ok(tx.commit()?)
    }

    pub fn file_count(&self) -> Result<u64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?)
    }

    pub fn rank(&mut self, query: &Record, filter: Filter) -> Result<Vec<(i64, f32)>> {
        let mut sql = String::from(
            "SELECT e.slot, e.file_id FROM embeddings e JOIN files f ON f.id = e.file_id WHERE e.config_id = ?",
        );
        let mut values: Vec<Value> = vec![i64::from(CONFIG_ID).into()];
        push_filter(filter, &mut sql, &mut values);
        let mut candidates: Vec<(u64, i64)> = {
            let mut statement = self.conn.prepare(&sql)?;
            let rows = statement.query_map(params_from_iter(values), |r| {
                Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)?))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        candidates.sort_unstable();
        let slots: Vec<u64> = candidates.iter().map(|(slot, _)| *slot).collect();

        let query = to_floats(query);
        let mut closest: HashMap<i64, f32> = HashMap::new();
        let mut next = 0;
        self.vectors.read_each(&slots, &mut |slot, record| {
            let (expected, file) = candidates[next];
            debug_assert_eq!(slot, expected);
            next += 1;
            let score = similarity(&query, record);
            closest
                .entry(file)
                .and_modify(|best| *best = best.max(score))
                .or_insert(score);
        })?;

        let mut ranked: Vec<(i64, f32)> = closest.into_iter().collect();
        ranked.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        Ok(ranked)
    }

    pub fn newest(&self, filter: Filter) -> Result<Vec<(i64, f32)>> {
        let mut sql = String::from("SELECT f.id FROM files f WHERE 1 = 1");
        let mut values: Vec<Value> = Vec::new();
        push_filter(filter, &mut sql, &mut values);
        sql.push_str(" ORDER BY f.modified_ms DESC, f.id");
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |r| Ok((r.get::<_, i64>(0)?, 0.0)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn hits(&self, ranked: &[(i64, f32)]) -> Result<Vec<Hit>> {
        let mut folders: HashMap<i64, PathBuf> = HashMap::new();
        let mut hits = Vec::with_capacity(ranked.len());
        for &(file, score) in ranked {
            let stored: Option<(i64, String, i64, i64, i64)> = self
                .conn
                .prepare_cached("SELECT dir_id, name, category, modified_ms, size FROM files WHERE id = ?1")?
                .query_row([file], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
                .optional()?;
            let Some((dir_id, name, category, modified_ms, size)) = stored else {
                continue;
            };
            let Some(category) = category_from_code(category) else {
                continue;
            };
            if !folders.contains_key(&dir_id) {
                folders.insert(dir_id, dir_path(&self.conn, dir_id)?);
            }
            let dir = &folders[&dir_id];
            let folder = dir
                .file_name()
                .unwrap_or(dir.as_os_str())
                .to_string_lossy()
                .into_owned();
            hits.push(Hit {
                file,
                path: dir.join(&name),
                folder,
                category,
                modified_ms,
                size: size as u64,
                score,
            });
        }
        Ok(hits)
    }
}

fn push_filter(filter: Filter, sql: &mut String, values: &mut Vec<Value>) {
    if let Some(category) = filter.category {
        sql.push_str(" AND f.category = ?");
        values.push(category_code(category).into());
    }
    if let Some(from) = filter.from_ms {
        sql.push_str(" AND f.modified_ms >= ?");
        values.push(from.into());
    }
    if let Some(to) = filter.to_ms {
        sql.push_str(" AND f.modified_ms < ?");
        values.push(to.into());
    }
}

#[cfg(test)]
impl Store {
    pub fn count_rows(&self, table: &str) -> i64 {
        self.conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
}

#[derive(Default, Debug)]
pub struct Changes {
    pub changed: HashSet<String>,
    pub new: HashSet<String>,
}

fn vectors_path(dir: &Path) -> PathBuf {
    dir.join(format!("vectors-{CONFIG_ID}.bin"))
}

pub fn disk_bytes(dir: &Path) -> u64 {
    ["mesa.db", "mesa.db-wal"]
        .iter()
        .map(|name| dir.join(name))
        .chain(std::iter::once(vectors_path(dir)))
        .filter_map(|path| fs::metadata(path).ok())
        .map(|metadata| metadata.len())
        .sum()
}

fn open_db(dir: &Path) -> Result<Connection> {
    let conn = Connection::open(dir.join("mesa.db"))?;
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version as usize > MIGRATIONS.len() {
        return Err(Error::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the index database has version {version}, which this version of Mesa can't read"),
        )));
    }
    for (i, migration) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        conn.execute_batch(&format!("BEGIN; {migration} PRAGMA user_version = {}; COMMIT;", i + 1))?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO embedding_configs (id, model, dims, dtype) VALUES (?1, ?2, ?3, 'f16')",
        params![CONFIG_ID, embed::MODEL_NAME, DIMS],
    )?;
    conn.execute("DELETE FROM embeddings WHERE config_id != ?1", [CONFIG_ID])?;
    conn.execute("DELETE FROM free_slots WHERE config_id != ?1", [CONFIG_ID])?;
    Ok(conn)
}

fn remove_other_vector_files(dir: &Path) {
    let keep = vectors_path(dir);
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("vectors-") && name.ends_with(".bin") && entry.path() != keep {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn extent(conn: &Connection) -> Result<u64> {
    let slots: i64 = conn.query_row(
        "SELECT COALESCE(MAX(slot) + 1, 0) FROM (
             SELECT slot FROM embeddings WHERE config_id = ?1
             UNION ALL
             SELECT slot FROM free_slots WHERE config_id = ?1
         )",
        [CONFIG_ID],
        |r| r.get(0),
    )?;
    Ok(slots as u64)
}

fn category_code(category: Category) -> i64 {
    match category {
        Category::Image => 0,
        Category::Video => 1,
        Category::Audio => 2,
        Category::Document => 3,
    }
}

fn category_from_code(code: i64) -> Option<Category> {
    Some(match code {
        0 => Category::Image,
        1 => Category::Video,
        2 => Category::Audio,
        3 => Category::Document,
        _ => return None,
    })
}

fn dir_path(conn: &Connection, mut id: i64) -> Result<PathBuf> {
    let mut names = Vec::new();
    while id != 0 {
        let (parent, name): (i64, String) = conn
            .prepare_cached("SELECT parent_id, name FROM dirs WHERE id = ?1")?
            .query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        names.push(name);
        id = parent;
    }
    Ok(names.into_iter().rev().collect())
}

fn segments(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => names.push(prefix.as_os_str().to_string_lossy().into_owned()),
            Component::RootDir => match names.last_mut() {
                Some(prefix) => prefix.push(MAIN_SEPARATOR),
                None => names.push(MAIN_SEPARATOR.to_string()),
            },
            Component::Normal(name) => names.push(name.to_string_lossy().into_owned()),
            Component::CurDir | Component::ParentDir => {}
        }
    }
    names
}

fn child_dir(conn: &Connection, parent: i64, name: &str) -> Result<Option<i64>> {
    Ok(conn
        .prepare_cached("SELECT id FROM dirs WHERE parent_id = ?1 AND name = ?2")?
        .query_row(params![parent, name], |r| r.get(0))
        .optional()?)
}

fn find_dir(conn: &Connection, path: &Path) -> Result<Option<i64>> {
    let names = segments(path);
    if names.is_empty() {
        return Ok(None);
    }
    let mut id = 0;
    for name in &names {
        match child_dir(conn, id, name)? {
            Some(child) => id = child,
            None => return Ok(None),
        }
    }
    Ok(Some(id))
}

fn ensure_dir(conn: &Connection, path: &Path) -> Result<i64> {
    let mut id = 0;
    for name in segments(path) {
        id = match child_dir(conn, id, &name)? {
            Some(child) => child,
            None => conn
                .prepare_cached("INSERT INTO dirs (parent_id, name) VALUES (?1, ?2) RETURNING id")?
                .query_row(params![id, name], |r| r.get(0))?,
        };
    }
    Ok(id)
}

fn forget_missing_folders(tx: &Transaction, parent: i64, present: &[String]) -> Result<()> {
    let present: HashSet<String> = present.iter().map(|name| name.to_lowercase()).collect();
    let stored: Vec<(i64, String)> = tx
        .prepare_cached("SELECT id, name FROM dirs WHERE parent_id = ?1 AND id != ?1")?
        .query_map([parent], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, name) in stored {
        if !present.contains(&name.to_lowercase()) {
            tx.prepare_cached("DELETE FROM dirs WHERE id = ?1")?.execute([id])?;
        }
    }
    Ok(())
}

struct StoredFile {
    id: i64,
    modified_ms: i64,
    size: i64,
    embedded_with: i64,
}

fn is_current(stored: &StoredFile, file: &ScannedFile) -> bool {
    stored.modified_ms == file.modified_ms
        && stored.size == file.size as i64
        && stored.embedded_with == i64::from(CONFIG_ID)
}

fn existing_files(conn: &Connection, dir_id: i64) -> Result<HashMap<String, StoredFile>> {
    let mut statement =
        conn.prepare_cached("SELECT name, id, modified_ms, size, embedded_with FROM files WHERE dir_id = ?1")?;
    let rows = statement.query_map([dir_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            StoredFile {
                id: r.get(1)?,
                modified_ms: r.get(2)?,
                size: r.get(3)?,
                embedded_with: r.get(4)?,
            },
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn upsert_file(tx: &Transaction, dir_id: i64, file: &ScannedFile) -> Result<i64> {
    Ok(tx
        .prepare_cached(
            "INSERT INTO files (dir_id, name, category, modified_ms, size, embedded_with)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (dir_id, name) DO UPDATE SET
                 category = excluded.category,
                 modified_ms = excluded.modified_ms,
                 size = excluded.size,
                 embedded_with = excluded.embedded_with
             RETURNING id",
        )?
        .query_row(
            params![
                dir_id,
                file.name,
                category_code(file.category),
                file.modified_ms,
                file.size as i64,
                CONFIG_ID
            ],
            |r| r.get(0),
        )?)
}

fn take_slot(tx: &Transaction, next_slot: &mut u64) -> Result<u64> {
    let freed: Option<i64> = tx
        .prepare_cached(
            "DELETE FROM free_slots
             WHERE config_id = ?1 AND slot = (SELECT MIN(slot) FROM free_slots WHERE config_id = ?1)
             RETURNING slot",
        )?
        .query_row([CONFIG_ID], |r| r.get(0))
        .optional()?;
    Ok(match freed {
        Some(slot) => slot as u64,
        None => {
            *next_slot += 1;
            *next_slot - 1
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectors;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mesa-store-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn folder(name: &str) -> PathBuf {
        crate::browse::roots().remove(0).join("mesa-test").join(name)
    }

    fn file(name: &str, modified_ms: i64) -> ScannedFile {
        ScannedFile {
            name: name.to_owned(),
            category: Category::Image,
            modified_ms,
            size: 100,
            vectors: vec![Located {
                locator: 0,
                record: vectors::random_record(),
            }],
        }
    }

    fn in_parts(name: &str, modified_ms: i64, locators: &[u64]) -> ScannedFile {
        let vectors = locators
            .iter()
            .map(|&locator| Located {
                locator,
                record: vectors::random_record(),
            })
            .collect();
        ScannedFile {
            vectors,
            ..file(name, modified_ms)
        }
    }

    fn scan(name: &str, files: Vec<ScannedFile>) -> ScannedDir {
        ScannedDir {
            path: folder(name),
            files,
            complete: true,
            subfolders: None,
        }
    }

    fn write(store: &mut Store, scans: &[ScannedDir]) {
        store.write_scans(scans, &|_| true).unwrap();
    }

    fn count(store: &Store, table: &str) -> i64 {
        store.count_rows(table)
    }

    fn slot_of(store: &Store, dir: &str, name: &str) -> u64 {
        let dir_id = find_dir(&store.conn, &folder(dir)).unwrap().unwrap();
        store
            .conn
            .query_row(
                "SELECT slot FROM embeddings JOIN files ON files.id = file_id WHERE dir_id = ?1 AND name = ?2",
                params![dir_id, name],
                |r| r.get::<_, i64>(0),
            )
            .unwrap() as u64
    }

    #[test]
    fn stores_file_and_vector() {
        let mut store = Store::open(&temp_dir("basic")).unwrap();
        let mut video = file("b.mp4", 2000);
        video.category = Category::Video;
        video.size = 5000;
        write(&mut store, &[scan("a", vec![file("a.jpg", 1000), video])]);

        assert_eq!(count(&store, "files"), 2);
        assert_eq!(count(&store, "embeddings"), 2);
        assert_eq!(store.vectors.next_slot(), 2);
        let (category, modified, size): (i64, i64, i64) = store
            .conn
            .query_row(
                "SELECT category, modified_ms, size FROM files WHERE name = 'b.mp4'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((category, modified, size), (1, 2000, 5000));
        let vector = store.vectors.read(slot_of(&store, "a", "a.jpg")).unwrap();
        assert_eq!(vector.len(), DIMS);
        assert!(vector.iter().any(|v| v.to_f32() != 0.0));
    }

    #[test]
    fn rename_folder_keeps_vectors() {
        let mut store = Store::open(&temp_dir("rename-folder")).unwrap();
        write(
            &mut store,
            &[
                scan("p/old", vec![file("a.jpg", 1)]),
                scan("p/old/deep", vec![file("b.jpg", 1)]),
            ],
        );
        let (slot_a, slot_b) = (
            slot_of(&store, "p/old", "a.jpg"),
            slot_of(&store, "p/old/deep", "b.jpg"),
        );

        assert!(store.rename_path(&folder("p/old"), &folder("p/new")).unwrap());

        assert_eq!(slot_of(&store, "p/new", "a.jpg"), slot_a);
        assert_eq!(slot_of(&store, "p/new/deep", "b.jpg"), slot_b);
        assert!(find_dir(&store.conn, &folder("p/old")).unwrap().is_none());
        assert_eq!(
            (
                count(&store, "files"),
                count(&store, "embeddings"),
                count(&store, "free_slots")
            ),
            (2, 2, 0)
        );
    }

    #[test]
    fn changed_file_reuses_slots() {
        let mut store = Store::open(&temp_dir("reparts")).unwrap();
        write(
            &mut store,
            &[scan("a", vec![in_parts("song.mp3", 1, &[0, 25_000, 50_000])])],
        );
        write(&mut store, &[scan("a", vec![in_parts("song.mp3", 2, &[0, 25_000])])]);
        assert_eq!(count(&store, "embeddings"), 2);
        assert_eq!(count(&store, "free_slots"), 1);
        assert_eq!(store.vectors.next_slot(), 3);
        write(
            &mut store,
            &[scan("a", vec![in_parts("song.mp3", 3, &[0, 25_000, 50_000, 75_000])])],
        );
        assert_eq!((count(&store, "embeddings"), count(&store, "free_slots")), (4, 0));
        assert_eq!(store.vectors.next_slot(), 4);
    }

    #[test]
    fn forgets_missing_only_after_complete_scan() {
        let mut store = Store::open(&temp_dir("prune")).unwrap();
        write(&mut store, &[scan("a", vec![file("keep.jpg", 1), file("gone.jpg", 1)])]);

        let mut partial = scan("a", vec![file("keep.jpg", 1)]);
        partial.complete = false;
        write(&mut store, &[partial]);
        assert_eq!(count(&store, "files"), 2);

        write(&mut store, &[scan("a", vec![file("keep.jpg", 1)])]);
        assert_eq!(count(&store, "files"), 1);
        assert_eq!(count(&store, "free_slots"), 1);
    }

    #[test]
    fn remove_folder_frees_slots() {
        let mut store = Store::open(&temp_dir("remove")).unwrap();
        write(
            &mut store,
            &[
                scan("a", vec![file("1.jpg", 1), file("2.jpg", 1)]),
                scan("a/deeper", vec![file("3.jpg", 1)]),
                scan("b", vec![file("4.jpg", 1)]),
            ],
        );
        store.remove_path(&folder("a")).unwrap();
        assert_eq!(count(&store, "files"), 1);
        assert_eq!(count(&store, "embeddings"), 1);
        assert_eq!(count(&store, "free_slots"), 3);

        write(&mut store, &[scan("c", vec![file("5.jpg", 1), file("6.jpg", 1)])]);
        assert_eq!(store.vectors.next_slot(), 4);
        assert_eq!(count(&store, "free_slots"), 1);
    }

    #[test]
    fn reopen_trims_unreferenced_vectors() {
        let dir = temp_dir("reopen");
        let mut store = Store::open(&dir).unwrap();
        write(&mut store, &[scan("a", vec![file("1.jpg", 1), file("2.jpg", 1)])]);
        store.vectors.write(&mut [(2, vectors::random_record())]).unwrap();
        assert_eq!(store.vectors.next_slot(), 3);
        drop(store);

        let store = Store::open(&dir).unwrap();
        assert_eq!(count(&store, "files"), 2);
        assert_eq!(store.vectors.next_slot(), 2);
    }

    #[test]
    fn refuses_newer_schema() {
        let dir = temp_dir("newer");
        {
            let conn = Connection::open(dir.join("mesa.db")).unwrap();
            conn.execute_batch("PRAGMA user_version = 99").unwrap();
        }
        assert!(Store::open(&dir).is_err());
    }

    fn pointing(x: f32, y: f32) -> Record {
        let mut output = vec![0.0; DIMS];
        output[0] = x;
        output[1] = y;
        vectors::record_from(&output).unwrap()
    }

    fn found_as(name: &str, category: Category, modified_ms: i64, vector: Record) -> ScannedFile {
        ScannedFile {
            category,
            vectors: vec![Located {
                locator: 0,
                record: vector,
            }],
            ..file(name, modified_ms)
        }
    }

    fn names(hits: &[Hit]) -> Vec<&str> {
        hits.iter()
            .map(|hit| hit.path.file_name().unwrap().to_str().unwrap())
            .collect()
    }

    const EVERYTHING: Filter = Filter {
        category: None,
        from_ms: None,
        to_ms: None,
    };

    fn top(store: &mut Store, query: &Record, filter: Filter, limit: usize) -> Result<Vec<Hit>> {
        let ranked = store.rank(query, filter)?;
        store.hits(&ranked[..limit.min(ranked.len())])
    }

    #[test]
    fn search_orders_by_similarity() {
        let mut store = Store::open(&temp_dir("search-order")).unwrap();
        write(
            &mut store,
            &[
                scan(
                    "a",
                    vec![found_as("opposite.jpg", Category::Image, 1, pointing(-1.0, 0.0))],
                ),
                scan(
                    "b",
                    vec![
                        found_as("same.jpg", Category::Image, 1, pointing(1.0, 0.0)),
                        found_as("unrelated.jpg", Category::Image, 1, pointing(0.0, 1.0)),
                        found_as("near.jpg", Category::Image, 1, pointing(3.0, 1.0)),
                    ],
                ),
            ],
        );
        let hits = top(&mut store, &pointing(1.0, 0.0), EVERYTHING, 10).unwrap();
        assert_eq!(names(&hits), ["same.jpg", "near.jpg", "unrelated.jpg", "opposite.jpg"]);
        assert!(
            (hits[0].score - 1.0).abs() < 1e-2 && (hits[3].score + 1.0).abs() < 1e-2,
            "{hits:?}"
        );
        assert_eq!(hits[0].path, folder("b").join("same.jpg"));
        assert_eq!((hits[0].folder.as_str(), hits[3].folder.as_str()), ("b", "a"));
        assert_eq!(
            (hits[0].category, hits[0].modified_ms, hits[0].size),
            (Category::Image, 1, 100)
        );
    }

    #[test]
    fn multi_vector_file_listed_once() {
        let mut store = Store::open(&temp_dir("search-parts")).unwrap();
        let song = ScannedFile {
            category: Category::Audio,
            vectors: vec![
                Located {
                    locator: 0,
                    record: pointing(0.0, 1.0),
                },
                Located {
                    locator: 25_000,
                    record: pointing(1.0, 0.0),
                },
                Located {
                    locator: 50_000,
                    record: pointing(1.0, 1.0),
                },
            ],
            ..file("song.mp3", 1)
        };
        write(
            &mut store,
            &[scan(
                "a",
                vec![song, found_as("between.jpg", Category::Image, 1, pointing(5.0, 1.0))],
            )],
        );
        let hits = top(&mut store, &pointing(1.0, 0.0), EVERYTHING, 10).unwrap();
        assert_eq!(names(&hits), ["song.mp3", "between.jpg"]);
        assert!((hits[0].score - 1.0).abs() < 1e-2, "{}", hits[0].score);
    }
}
