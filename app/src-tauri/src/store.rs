//! Vectors are written and synced before the transaction that refers to them commits, so a crash can leave
//! unreferenced vectors at the end of the file but never a row pointing at a missing one. Opening trims the leftovers.

use std::{
    collections::{HashMap, HashSet},
    fmt, fs, io, mem,
    path::{Component, MAIN_SEPARATOR, Path, PathBuf},
};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{
    browse::Category,
    vectors::{self, DIMS, Record, VectorFile},
};

const CONFIG_ID: u32 = 1;
const CONFIG_MODEL: &str = "random-placeholder";
const MIGRATIONS: &[&str] = &[SCHEMA, ADD_META];

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
}

pub struct ScannedDir {
    pub path: PathBuf,
    pub files: Vec<ScannedFile>,
    pub complete: bool,
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
        let mut vectors = VectorFile::open(&vectors_path(dir), CONFIG_ID)?;
        vectors.truncate_to(extent(&conn)?)?;
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

    pub fn finish_job(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM jobs WHERE id = ?1", [id])?;
        Ok(())
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
                let slot = match old.and_then(|o| o.slot) {
                    Some(slot) => slot,
                    None => {
                        let slot = take_slot(&tx, &mut next_slot)?;
                        tx.prepare_cached("INSERT INTO embeddings (config_id, slot, file_id) VALUES (?1, ?2, ?3)")?
                            .execute(params![CONFIG_ID, slot as i64, file_id])?;
                        slot
                    }
                };
                writes.push((slot, vectors::random_record()));
            }

            if scan.complete {
                for (name, old) in &existing {
                    if !seen.contains(name.as_str()) {
                        tx.prepare_cached("DELETE FROM files WHERE id = ?1")?
                            .execute([old.id])?;
                    }
                }
            }
        }

        self.vectors.write(&mut writes)?;
        self.vectors.sync()?;
        Ok(tx.commit()?)
    }

    pub fn changed(&self, scan: &ScannedDir) -> Result<HashSet<String>> {
        let existing = match find_dir(&self.conn, &scan.path)? {
            Some(dir) => existing_files(&self.conn, dir)?,
            None => HashMap::new(),
        };
        Ok(scan
            .files
            .iter()
            .filter(|file| !existing.get(&file.name).is_some_and(|old| is_current(old, file)))
            .map(|file| file.name.clone())
            .collect())
    }

    pub fn remove_path(&mut self, path: &Path) -> Result<()> {
        let tx = self.conn.transaction()?;
        if let Some(id) = find_dir(&tx, path)? {
            tx.execute("DELETE FROM dirs WHERE id = ?1", [id])?;
        }
        if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
            if let Some(dir) = find_dir(&tx, parent)? {
                tx.execute(
                    "DELETE FROM files WHERE dir_id = ?1 AND name = ?2",
                    params![dir, name.to_string_lossy()],
                )?;
            }
        }
        Ok(tx.commit()?)
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

fn vectors_path(dir: &Path) -> PathBuf {
    dir.join(format!("vectors-{CONFIG_ID}.bin"))
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
        params![CONFIG_ID, CONFIG_MODEL, DIMS],
    )?;
    Ok(conn)
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

struct StoredFile {
    id: i64,
    modified_ms: i64,
    size: i64,
    slot: Option<u64>,
}

fn is_current(stored: &StoredFile, file: &ScannedFile) -> bool {
    stored.modified_ms == file.modified_ms && stored.size == file.size as i64 && stored.slot.is_some()
}

fn existing_files(conn: &Connection, dir_id: i64) -> Result<HashMap<String, StoredFile>> {
    let mut statement = conn.prepare_cached(
        "SELECT f.name, f.id, f.modified_ms, f.size,
                (SELECT slot FROM embeddings WHERE config_id = ?2 AND file_id = f.id LIMIT 1)
         FROM files f WHERE f.dir_id = ?1",
    )?;
    let rows = statement.query_map(params![dir_id, CONFIG_ID], |r| {
        Ok((
            r.get::<_, String>(0)?,
            StoredFile {
                id: r.get(1)?,
                modified_ms: r.get(2)?,
                size: r.get(3)?,
                slot: r.get::<_, Option<i64>>(4)?.map(|s| s as u64),
            },
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn upsert_file(tx: &Transaction, dir_id: i64, file: &ScannedFile) -> Result<i64> {
    Ok(tx
        .prepare_cached(
            "INSERT INTO files (dir_id, name, category, modified_ms, size) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (dir_id, name) DO UPDATE SET
                 category = excluded.category,
                 modified_ms = excluded.modified_ms,
                 size = excluded.size
             RETURNING id",
        )?
        .query_row(
            params![
                dir_id,
                file.name,
                category_code(file.category),
                file.modified_ms,
                file.size as i64
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
        }
    }

    fn scan(name: &str, files: Vec<ScannedFile>) -> ScannedDir {
        ScannedDir {
            path: folder(name),
            files,
            complete: true,
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
}
