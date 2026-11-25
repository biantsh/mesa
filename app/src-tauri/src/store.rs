//! Vectors are written and synced before the transaction that refers to them commits, so a crash can leave
//! unreferenced vectors at the end of the file but never a row pointing at a missing one. Opening trims the leftovers.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fmt, fs, io,
    path::{Component, MAIN_SEPARATOR, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicI64, AtomicU64, Ordering},
    },
};

use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter, types::Value};

use crate::{
    browse::{Categories, Category},
    embed::{self, PageTexts},
    relevance::{self, Ranked},
    vectors::{DIMS, Located, Record, VectorFile, similarity, to_floats},
    words,
};

const CONFIG_ID: u32 = 1;

const MIGRATIONS: &[&str] = &[SCHEMA];

const WORDS_ID: u32 = 1;

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
    id            INTEGER PRIMARY KEY,
    dir_id        INTEGER NOT NULL REFERENCES dirs(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    category      INTEGER NOT NULL,  -- see `category_code`
    modified_ms   INTEGER NOT NULL,  -- Unix time in milliseconds
    size          INTEGER NOT NULL,  -- bytes
    embedded_with INTEGER NOT NULL DEFAULT 0,
    read_with     INTEGER NOT NULL DEFAULT 0,
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
    locator   INTEGER NOT NULL DEFAULT 0,
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

CREATE TABLE rules (path TEXT PRIMARY KEY) WITHOUT ROWID;
CREATE TABLE exclusions (path TEXT PRIMARY KEY) WITHOUT ROWID;

-- Indexing that was asked for and hasn't finished, so it can carry on after a restart.
CREATE TABLE jobs (id INTEGER PRIMARY KEY, path TEXT NOT NULL);

CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;

CREATE TABLE file_count (n INTEGER NOT NULL);
INSERT INTO file_count (n) VALUES (0);
CREATE TRIGGER files_counted_in AFTER INSERT ON files BEGIN UPDATE file_count SET n = n + 1; END;
CREATE TRIGGER files_counted_out AFTER DELETE ON files BEGIN UPDATE file_count SET n = n - 1; END;

CREATE VIRTUAL TABLE picture_words USING fts5(folded, text UNINDEXED, tokenize = 'trigram remove_diacritics 1');
CREATE VIRTUAL TABLE document_words USING fts5(folded, content = '', contentless_delete = 1, tokenize = 'trigram remove_diacritics 1');
CREATE TRIGGER files_words_out AFTER DELETE ON files BEGIN
    DELETE FROM picture_words WHERE rowid = old.id;
    DELETE FROM document_words WHERE rowid = old.id;
END;

CREATE TABLE page_texts (
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    page    INTEGER NOT NULL,
    text    TEXT NOT NULL,
    PRIMARY KEY (file_id, page)
) WITHOUT ROWID;

CREATE VIRTUAL TABLE file_names USING fts5(folded, content = '', contentless_delete = 1, tokenize = 'trigram remove_diacritics 1');
CREATE TRIGGER file_names_added AFTER INSERT ON files BEGIN
    INSERT INTO file_names (rowid, folded) VALUES (new.id, mesa_fold(new.name));
END;
CREATE TRIGGER file_names_gone AFTER DELETE ON files BEGIN
    DELETE FROM file_names WHERE rowid = old.id;
END;
CREATE TRIGGER file_names_renamed AFTER UPDATE OF name ON files BEGIN
    DELETE FROM file_names WHERE rowid = old.id;
    INSERT INTO file_names (rowid, folded) VALUES (new.id, mesa_fold(new.name));
END;

CREATE TABLE cloud_files (
    dir_id   INTEGER NOT NULL REFERENCES dirs(id) ON DELETE CASCADE,
    name     TEXT NOT NULL,
    category INTEGER NOT NULL,
    PRIMARY KEY (dir_id, name)
) WITHOUT ROWID;
";

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
    pub words: Option<String>,
    pub pages: PageTexts,
    pub words_only: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    pub category: Option<Category>,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub code: CodeFilter,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CodeFilter {
    pub language: Option<&'static str>,
    pub project: Option<i64>,
    pub tests: bool,
}

impl CodeFilter {
    pub const NONE: Self = Self {
        language: None,
        project: None,
        tests: false,
    };

    fn is_none(&self) -> bool {
        *self == Self::NONE
    }
}

const TEST_NAMES: &[&str] = &[
    "test_*",
    "*_test.*",
    "*.test.*",
    "*.spec.*",
    "*_spec.*",
    "*Test.*",
    "*Tests.*",
    "conftest.py",
];
const TEST_FOLDERS: &[&str] = &["tests", "test", "__tests__", "spec", "specs", "testing"];

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub file: i64,
    pub path: PathBuf,
    pub folder: String,
    pub category: Category,
    pub modified_ms: i64,
    pub size: u64,
    pub score: f32,
    pub why: Why,
    pub copies: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Why {
    pub reason: Option<crate::relevance::Reason>,
    pub by_words: Option<WordsMatch>,
    pub at: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WordsMatch {
    Words,
    Phrase,
    Name,
}

pub struct ScannedDir {
    pub path: PathBuf,
    pub files: Vec<ScannedFile>,
    pub cloud: Vec<(String, Category)>,
    pub complete: bool,
    pub subfolders: Option<Vec<String>>,
}

impl ScannedDir {
    pub fn cloud_part(&self) -> Option<ScannedDir> {
        (!self.cloud.is_empty()).then(|| ScannedDir {
            path: self.path.clone(),
            files: Vec::new(),
            cloud: self.cloud.clone(),
            complete: false,
            subfolders: None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudOnly {
    Not,
    Partly,
    Wholly,
}

const CLOUD_CACHED: usize = 256;

#[derive(Default)]
struct CloudCache {
    any: Option<bool>,
    folders: HashMap<PathBuf, HashMap<String, CloudOnly>>,
    paths: HashMap<PathBuf, CloudOnly>,
}

pub struct Store {
    conn: Connection,
    vectors: VectorFile,
    counts: Arc<Counts>,
    files: i64,
    reading_pictures: Box<dyn Fn() -> bool + Send>,
    cloud: CloudCache,
}

#[derive(Default)]
pub struct Counts {
    pub files: AtomicI64,
    pub bytes: AtomicU64,
}

impl Counts {
    pub fn made(&self) {
        self.files.fetch_add(1, Ordering::SeqCst);
    }

    pub fn not_stored(&self, made: u64) {
        self.files.fetch_sub(made as i64, Ordering::SeqCst);
    }

    pub fn files(&self) -> u64 {
        self.files.load(Ordering::SeqCst).max(0) as u64
    }

    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::SeqCst)
    }
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
            conn.execute("UPDATE files SET embedded_with = 0", [])?;
            vectors.truncate_to(0)?;
        } else {
            vectors.truncate_to(expected)?;
            heal_free_slots(&conn)?;
        }
        let mut store = Self {
            conn,
            vectors,
            counts: Arc::default(),
            files: 0,
            reading_pictures: Box::new(|| true),
            cloud: CloudCache::default(),
        };
        store.publish_counts(0);
        Ok(store)
    }

    pub fn load_rules(&self) -> Result<BTreeSet<PathBuf>> {
        let mut statement = self.conn.prepare("SELECT path FROM rules")?;
        let rows = statement.query_map([], |r| Ok(PathBuf::from(r.get::<_, String>(0)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    #[cfg(test)]
    pub fn save_rules(&mut self, rules: &BTreeSet<PathBuf>) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM rules", [])?;
        {
            let mut insert = tx.prepare("INSERT INTO rules (path) VALUES (?1)")?;
            for path in rules {
                insert.execute(params![path.to_string_lossy()])?;
            }
        }
        Ok(tx.commit()?)
    }

    pub fn save_rules_diff<'a>(
        &mut self,
        gone: impl Iterator<Item = &'a PathBuf>,
        new: impl Iterator<Item = &'a PathBuf>,
    ) -> Result<()> {
        save_changes(
            &mut self.conn,
            "rules",
            "INSERT INTO rules (path) VALUES (?1)",
            gone,
            new,
        )
    }

    pub fn save_exclusions_changes(&mut self, before: &BTreeSet<PathBuf>, after: &BTreeSet<PathBuf>) -> Result<()> {
        save_changes(
            &mut self.conn,
            "exclusions",
            "INSERT INTO exclusions (path) VALUES (?1)",
            before.difference(after),
            after.difference(before),
        )
    }

    pub fn load_exclusions(&self) -> Result<BTreeSet<PathBuf>> {
        let mut statement = self.conn.prepare("SELECT path FROM exclusions")?;
        let rows = statement.query_map([], |r| Ok(PathBuf::from(r.get::<_, String>(0)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
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

    pub fn indexed_types(&self) -> Result<Option<Categories>> {
        let kept: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'indexed_types'", [], |r| r.get(0))
            .optional()?;
        Ok(kept.and_then(|bits| bits.parse().ok()).map(Categories::from_bits))
    }

    pub fn set_indexed_types(&self, types: Categories) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('indexed_types', ?1)",
            [types.bits().to_string()],
        )?;
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

    pub fn write_scans(&mut self, scans: &[ScannedDir], wanted: &dyn Fn(&Path) -> bool, made: u64) -> Result<()> {
        let result = self.write_scans_inner(scans, wanted);
        self.cloud = CloudCache::default();
        if result.is_err()
            && let Ok(slots) = extent(&self.conn)
        {
            let _ = self.vectors.truncate_to(slots);
        }
        self.publish_counts(made);
        result
    }

    pub fn read_pictures_if(&mut self, reading: impl Fn() -> bool + Send + 'static) {
        self.reading_pictures = Box::new(reading);
    }

    pub fn counts(&self) -> Arc<Counts> {
        Arc::clone(&self.counts)
    }

    fn publish_counts(&mut self, made: u64) {
        let Ok(files) = self
            .conn
            .query_row("SELECT n FROM file_count", [], |r| r.get::<_, i64>(0))
        else {
            return;
        };
        self.counts
            .files
            .fetch_add(files - self.files - made as i64, Ordering::SeqCst);
        self.files = files;
        if let Ok(bytes) = self.live_bytes() {
            self.counts.bytes.store(bytes, Ordering::SeqCst);
        }
    }

    fn write_scans_inner(&mut self, scans: &[ScannedDir], wanted: &dyn Fn(&Path) -> bool) -> Result<()> {
        let reading = (self.reading_pictures)();
        let tx = self.conn.transaction()?;
        let mut next_slot = self.vectors.next_slot();
        let mut reused_up_to = None;
        tx.execute("DELETE FROM temp.freed_now", [])?;
        let mut writes: Vec<(u64, Record)> = Vec::new();

        for scan in scans {
            let mut dir_id = find_dir(&tx, &scan.path)?;
            let existing = match dir_id {
                Some(id) => existing_of(&tx, id, scan)?,
                None => HashMap::new(),
            };
            let mut seen = HashSet::with_capacity(scan.files.len());

            for file in &scan.files {
                seen.insert(file.name.as_str());
                let old = existing.get(&file.name);
                if old.is_some_and(|old| is_current(old, file, reading)) {
                    continue;
                }
                if !wanted(&scan.path.join(&file.name)) {
                    continue;
                }
                if file.words_only {
                    if let Some(old) = old
                        && vectors_current(old, file)
                    {
                        write_words(&tx, old.id, file.category, &file.name, file.words.as_deref(), reading)?;
                        write_page_texts(&tx, old.id, &file.pages)?;
                    }
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
                    let slot = take_slot(&tx, &mut next_slot, &mut reused_up_to)?;
                    tx.prepare_cached(
                        "INSERT INTO embeddings (config_id, slot, file_id, locator) VALUES (?1, ?2, ?3, ?4)",
                    )?
                    .execute(params![CONFIG_ID, slot as i64, file_id, vector.locator as i64])?;
                    writes.push((slot, vector.record));
                }
                write_words(&tx, file_id, file.category, &file.name, file.words.as_deref(), reading)?;
                write_page_texts(&tx, file_id, &file.pages)?;
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
            write_cloud(&tx, &mut dir_id, scan, wanted)?;
        }

        if !writes.is_empty() {
            self.vectors.write(&mut writes)?;
            self.vectors.sync()?;
        }
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

    pub fn files_under_each(&self, roots: &[PathBuf]) -> Result<Vec<u64>> {
        let mut folders = Folders::default();
        roots
            .iter()
            .map(|root| match folders.find_child(&self.conn, root)? {
                Some(dir) => self.files_in_tree(dir),
                None => {
                    let (Some(parent), Some(name)) = (root.parent(), root.file_name()) else {
                        return Ok(0);
                    };
                    let Some(dir) = folders.find(&self.conn, parent)? else {
                        return Ok(0);
                    };
                    self.files_named(dir, &name.to_string_lossy())
                }
            })
            .collect()
    }

    fn files_in_tree(&self, dir: i64) -> Result<u64> {
        Ok(self
            .conn
            .prepare_cached(
                "WITH RECURSIVE tree(id) AS (
                 SELECT ?1 UNION ALL SELECT d.id FROM dirs d JOIN tree t ON d.parent_id = t.id WHERE d.id != 0
             )
             SELECT COUNT(*) FROM files WHERE embedded_with = ?2 AND dir_id IN (SELECT id FROM tree)",
            )?
            .query_row(params![dir, CONFIG_ID], |r| r.get(0))?)
    }

    fn files_named(&self, dir: i64, name: &str) -> Result<u64> {
        Ok(self
            .conn
            .prepare_cached("SELECT COUNT(*) FROM files WHERE dir_id = ?1 AND name = ?2 AND embedded_with = ?3")?
            .query_row(params![dir, name, CONFIG_ID], |r| r.get(0))?)
    }

    pub fn cloud_in(&mut self, folder: &Path) -> Result<HashMap<String, CloudOnly>> {
        if !self.any_cloud()? {
            return Ok(HashMap::new());
        }
        if let Some(found) = self.cloud.folders.get(folder) {
            return Ok(found.clone());
        }
        let found = match find_dir(&self.conn, folder)? {
            Some(dir) => cloud_children(&self.conn, dir)?,
            None => HashMap::new(),
        };
        if self.cloud.folders.len() >= CLOUD_CACHED {
            self.cloud.folders.clear();
        }
        self.cloud.folders.insert(folder.to_owned(), found.clone());
        Ok(found)
    }

    pub fn cloud_of(&mut self, path: &Path) -> Result<CloudOnly> {
        if !self.any_cloud()? {
            return Ok(CloudOnly::Not);
        }
        if let Some(&found) = self.cloud.paths.get(path) {
            return Ok(found);
        }
        let found = cloud_of(&self.conn, path)?;
        if self.cloud.paths.len() >= CLOUD_CACHED {
            self.cloud.paths.clear();
        }
        self.cloud.paths.insert(path.to_owned(), found);
        Ok(found)
    }

    pub fn files_below(&self, path: &Path) -> Result<bool> {
        match find_dir(&self.conn, path)? {
            Some(dir) => has_files_below(&self.conn, dir),
            None => Ok(false),
        }
    }

    fn any_cloud(&mut self) -> Result<bool> {
        if let Some(any) = self.cloud.any {
            return Ok(any);
        }
        let any = self
            .conn
            .query_row("SELECT EXISTS (SELECT 1 FROM cloud_files)", [], |r| r.get(0))?;
        self.cloud.any = Some(any);
        Ok(any)
    }

    pub fn has_stale_files(&self) -> Result<bool> {
        let reading = (self.reading_pictures)();
        let pictures = if reading { category_code(Category::Image) } else { -1 };
        Ok(self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM files WHERE embedded_with != ?1)
                 OR EXISTS (SELECT 1 FROM files WHERE read_with != ?2
                            AND (category = ?3 OR (category = ?4 AND (?5 OR lower(name) NOT LIKE '%.pdf'))))",
            params![
                CONFIG_ID,
                WORDS_ID,
                pictures,
                category_code(Category::Document),
                reading
            ],
            |r| r.get(0),
        )?)
    }

    pub fn changes(&self, scan: &ScannedDir) -> Result<Changes> {
        let existing = match find_dir(&self.conn, &scan.path)? {
            Some(dir) => existing_of(&self.conn, dir, scan)?,
            None => HashMap::new(),
        };
        let mut changes = Changes::default();
        let reading = (self.reading_pictures)();
        for file in &scan.files {
            match existing.get(&file.name) {
                Some(old) if is_current(old, file, reading) => {}
                Some(old) if vectors_current(old, file) => {
                    changes.changed.insert(file.name.clone());
                    changes.reread.insert(file.name.clone());
                }
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
            self.cloud = CloudCache::default();
            self.publish_counts(0);
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
        self.cloud = CloudCache::default();
        self.publish_counts(0);
        Ok(true)
    }

    pub fn count_of(&self, category: Category) -> Result<u64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE category = ?1",
            [category_code(category)],
            |r| r.get(0),
        )?;
        Ok(count as u64)
    }

    pub fn remove_categories(&mut self, categories: &[Category]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for &category in categories {
            tx.execute("DELETE FROM files WHERE category = ?1", [category_code(category)])?;
            tx.execute("DELETE FROM cloud_files WHERE category = ?1", [category_code(category)])?;
        }
        tx.commit()?;
        self.cloud = CloudCache::default();
        self.publish_counts(0);
        Ok(())
    }

    pub fn remove_paths(&mut self, paths: &[PathBuf]) -> Result<()> {
        let mut by_folder: Vec<(&Path, Vec<String>)> = Vec::new();
        let mut places: HashMap<&Path, usize> = HashMap::new();
        let mut tops = Vec::new();
        for path in paths {
            match (path.parent(), path.components().next_back()) {
                (Some(parent), Some(Component::Normal(name))) if !segments(parent).is_empty() => {
                    let at = *places.entry(parent).or_insert_with(|| {
                        by_folder.push((parent, Vec::new()));
                        by_folder.len() - 1
                    });
                    by_folder[at].1.push(name.to_string_lossy().into_owned());
                }
                _ => tops.push(path),
            }
        }
        let tx = self.conn.transaction()?;
        for path in tops {
            if let Some(id) = find_dir(&tx, path)? {
                tx.prepare_cached("DELETE FROM dirs WHERE id = ?1")?.execute([id])?;
            }
        }
        let mut folders = Folders::default();
        for (parent, names) in by_folder {
            let Some(dir) = folders.find(&tx, parent)? else {
                continue;
            };
            let inside: HashMap<String, i64> = tx
                .prepare_cached("SELECT name, id FROM dirs WHERE parent_id = ?1 AND id != 0")?
                .query_map([dir], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            for name in &names {
                if let Some(id) = inside.get(name) {
                    tx.prepare_cached("DELETE FROM dirs WHERE id = ?1")?.execute([id])?;
                }
            }
            for some in names.chunks(500) {
                let names = serde_json::to_string(some).map_err(|e| Error::Io(io::Error::other(e)))?;
                tx.prepare_cached("DELETE FROM files WHERE dir_id = ?1 AND name IN (SELECT value FROM json_each(?2))")?
                    .execute(params![dir, names])?;
                tx.prepare_cached(
                    "DELETE FROM cloud_files WHERE dir_id = ?1 AND name IN (SELECT value FROM json_each(?2))",
                )?
                .execute(params![dir, names])?;
            }
        }
        tx.commit()?;
        self.cloud = CloudCache::default();
        self.publish_counts(0);
        Ok(())
    }

    pub fn live_bytes(&self) -> Result<u64> {
        Ok(self.reclaimable()?.live_bytes())
    }

    pub fn reclaimable(&self) -> Result<Reclaimable> {
        let free_slots: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM free_slots WHERE config_id = ?1",
            [CONFIG_ID],
            |r| r.get(0),
        )?;
        let pragma = |name: &str| {
            self.conn
                .query_row(&format!("PRAGMA {name}"), [], |r| r.get::<_, i64>(0))
        };
        Ok(Reclaimable {
            free_slots: free_slots.max(0) as u64,
            slots: self.vectors.next_slot(),
            free_pages: pragma("freelist_count")?.max(0) as u64,
            pages: pragma("page_count")?.max(0) as u64,
            page_size: pragma("page_size")?.max(0) as u64,
        })
    }

    pub fn begin_compaction(&mut self, most: usize) -> Result<Option<Compaction>> {
        let tx = self.conn.transaction()?;
        let free: Vec<u64> = {
            let mut statement =
                tx.prepare("SELECT slot FROM free_slots WHERE config_id = ?1 ORDER BY slot LIMIT ?2")?;
            let rows = statement.query_map(params![CONFIG_ID, most as i64], |r| {
                r.get::<_, i64>(0).map(|s| s as u64)
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let Some(&lowest) = free.first() else { return Ok(None) };
        let used: Vec<(u64, i64, i64)> = {
            let mut statement = tx.prepare(
                "SELECT slot, file_id, locator FROM embeddings WHERE config_id = ?1 AND slot > ?2 ORDER BY slot DESC LIMIT ?3",
            )?;
            let rows = statement.query_map(params![CONFIG_ID, lowest as i64, most as i64], |r| {
                Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let pairs: Vec<(u64, (u64, i64, i64))> = free
            .iter()
            .copied()
            .zip(used.iter().copied())
            .take_while(|(to, (from, ..))| to < from)
            .collect();
        if pairs.is_empty() {
            return Ok(None);
        }
        let records = read_records(&mut self.vectors, pairs.iter().map(|(_, (from, ..))| *from))?;
        let moves: Vec<Move> = pairs
            .iter()
            .filter_map(|&(to, (from, file_id, locator))| {
                records.get(&from).map(|record| Move {
                    from,
                    to,
                    file_id,
                    locator,
                    record: *record,
                })
            })
            .collect();
        let mut writes: Vec<(u64, Record)> = moves.iter().map(|m| (m.to, m.record)).collect();
        self.vectors.write(&mut writes)?;
        for m in &moves {
            tx.prepare_cached("DELETE FROM free_slots WHERE config_id = ?1 AND slot = ?2")?
                .execute(params![CONFIG_ID, m.to as i64])?;
        }
        tx.commit()?;
        Ok(Some(Compaction {
            flush: self.vectors.flush_handle()?,
            moves,
        }))
    }

    pub fn finish_compaction(&mut self, compaction: Compaction) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let now = read_records(&mut self.vectors, compaction.moves.iter().map(|m| m.from))?;
        let mut moved = 0;
        for m in &compaction.moves {
            let unchanged = now.get(&m.from).is_some_and(|record| *record == m.record);
            let done = unchanged
                && tx
                    .prepare_cached(
                        "UPDATE embeddings SET slot = ?1 WHERE config_id = ?2 AND slot = ?3 AND file_id = ?4 AND locator = ?5",
                    )?
                    .execute(params![m.to as i64, CONFIG_ID, m.from as i64, m.file_id, m.locator])?
                    == 1;
            let free = if done { m.from } else { m.to };
            tx.prepare_cached("INSERT OR IGNORE INTO free_slots (config_id, slot) VALUES (?1, ?2)")?
                .execute(params![CONFIG_ID, free as i64])?;
            moved += usize::from(done);
        }
        tx.commit()?;
        Ok(moved)
    }

    pub fn trim_vectors(&mut self) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let end: i64 = tx.query_row(
            "SELECT COALESCE(MAX(slot) + 1, 0) FROM embeddings WHERE config_id = ?1",
            [CONFIG_ID],
            |r| r.get(0),
        )?;
        tx.execute(
            "DELETE FROM free_slots WHERE config_id = ?1 AND slot >= ?2",
            params![CONFIG_ID, end],
        )?;
        tx.commit()?;
        let cut = self.vectors.next_slot().saturating_sub(end as u64);
        if cut > 0 {
            self.vectors.truncate_to(end as u64)?;
        }
        Ok(cut)
    }

    pub fn give_back_pages(&self, pages: u32) -> Result<()> {
        Ok(self
            .conn
            .execute_batch(&format!("PRAGMA incremental_vacuum({pages})"))?)
    }

    pub fn rank(&mut self, query: &Record, filter: Filter, by_words: &HashMap<i64, f64>) -> Result<Ranked> {
        let mut values: Vec<Value> = Vec::new();
        let passes = filter_condition(filter, &mut values);
        let sql = format!(
            "SELECT e.slot, e.file_id, f.category, ({passes}), e.locator FROM embeddings e JOIN files f ON f.id = e.file_id WHERE e.config_id = ?"
        );
        values.push(i64::from(CONFIG_ID).into());
        let mut candidates: Vec<(u64, i64, i64, bool, u64)> = {
            let mut statement = self.conn.prepare(&sql)?;
            let rows = statement.query_map(params_from_iter(values), |r| {
                Ok((
                    r.get::<_, i64>(0)? as u64,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, bool>(3)?,
                    r.get::<_, i64>(4)? as u64,
                ))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        candidates.sort_unstable();
        let slots: Vec<u64> = candidates.iter().map(|&(slot, ..)| slot).collect();

        let query = to_floats(query);
        let mut closest: HashMap<i64, (i64, f32, bool, u64)> = HashMap::new();
        let mut next = 0;
        self.vectors.read_each(&slots, &mut |slot, record| {
            let (expected, file, category, passes, locator) = candidates[next];
            debug_assert_eq!(slot, expected);
            next += 1;
            let score = similarity(&query, record);
            closest
                .entry(file)
                .and_modify(|(_, best, _, at)| {
                    if score > *best {
                        (*best, *at) = (score, locator);
                    }
                })
                .or_insert((category, score, passes, locator));
        })?;

        let among: Vec<(i64, i64, f32)> = closest
            .iter()
            .map(|(&file, &(category, score, ..))| (file, category, score))
            .collect();
        let (mut scored, mut at) = (Vec::new(), HashMap::new());
        for (file, (category, score, passes, locator)) in closest {
            if passes {
                scored.push((file, category, score));
                at.insert(file, locator);
            }
        }
        Ok(relevance::Ranked {
            at,
            ..relevance::rank(&scored, &among, by_words)
        })
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

    pub fn exact(&self, query: &str, filter: Filter) -> Result<Vec<(i64, bool)>> {
        let terms = words::terms(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let fts = terms
            .iter()
            .map(|term| words::whole(term))
            .collect::<Vec<_>>()
            .join(" AND ");
        let (pictures, documents) = self.words_matching(&fts, filter)?;
        Ok(pictures
            .into_iter()
            .map(|file| (file, true))
            .chain(documents.into_iter().map(|file| (file, false)))
            .collect())
    }

    pub fn phrase(&self, query: &str, filter: Filter) -> Result<Vec<i64>> {
        if words::terms(query).len() < 2 {
            return Ok(Vec::new());
        }
        let (mut found, documents) = self.words_matching(&words::whole(&words::words_of(query).join(" ")), filter)?;
        found.extend(documents);
        Ok(found)
    }

    pub fn code_with_phrase(&self, words: &[String], filter: Filter) -> Result<Vec<i64>> {
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let (_, code) = self.words_matching(
            &words::whole(&words.join(" ")),
            Filter {
                category: Some(Category::Code),
                ..filter
            },
        )?;
        Ok(code)
    }

    pub fn code_named(&self, name: &str, filter: Filter) -> Result<Vec<i64>> {
        let mut values: Vec<Value> = vec![name.to_owned().into()];
        let passes = filter_condition(
            Filter {
                category: Some(Category::Code),
                ..filter
            },
            &mut values,
        );
        let sql = format!("SELECT f.id FROM files f WHERE f.name = ? AND {passes}");
        let mut statement = self.conn.prepare_cached(&sql)?;
        let found = statement
            .query_map(rusqlite::params_from_iter(values), |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
        Ok(found)
    }

    fn words_matching(&self, fts: &str, filter: Filter) -> Result<(Vec<i64>, Vec<i64>)> {
        let mut found = [Vec::new(), Vec::new()];
        for (table, found) in ["picture_words", "document_words"].into_iter().zip(&mut found) {
            let mut values: Vec<Value> = vec![fts.to_owned().into()];
            let passes = filter_condition(filter, &mut values);
            let sql = format!(
                "SELECT f.id FROM {table} w JOIN files f ON f.id = w.rowid WHERE w.{table} MATCH ? AND {passes}"
            );
            let mut statement = self.conn.prepare_cached(&sql)?;
            let rows = statement.query_map(params_from_iter(values), |r| r.get::<_, i64>(0))?;
            found.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        let [pictures, documents] = found;
        Ok((pictures, documents))
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
            if let std::collections::hash_map::Entry::Vacant(e) = folders.entry(dir_id) {
                e.insert(dir_path(&self.conn, dir_id)?);
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
                why: Why::default(),
                copies: Vec::new(),
            });
        }
        Ok(hits)
    }

    pub fn picture_text(&self, path: &Path) -> Result<Option<String>> {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Ok(None);
        };
        let Some(dir) = find_dir(&self.conn, parent)? else {
            return Ok(None);
        };
        Ok(self
            .conn
            .prepare_cached(
                "SELECT w.text FROM files f JOIN picture_words w ON w.rowid = f.id WHERE f.dir_id = ?1 AND f.name = ?2",
            )?
            .query_row(params![dir, name.to_string_lossy()], |r| r.get::<_, Option<String>>(0))
            .optional()?
            .flatten())
    }

    pub fn code_folders(&self) -> Result<Vec<PathBuf>> {
        let mut statement = self
            .conn
            .prepare_cached("SELECT DISTINCT dir_id FROM files WHERE category = ?1")?;
        let dirs: Vec<i64> = statement
            .query_map([category_code(Category::Code)], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        dirs.into_iter().map(|dir| dir_path(&self.conn, dir)).collect()
    }

    pub fn folder_id(&self, path: &Path) -> Result<Option<i64>> {
        find_dir(&self.conn, path)
    }

    pub fn named(&self, query: &str, filter: Filter) -> Result<Vec<i64>> {
        let terms = words::terms(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let fts = terms
            .iter()
            .map(|term| words::whole(term))
            .collect::<Vec<_>>()
            .join(" AND ");
        let mut values: Vec<Value> = vec![fts.into()];
        let passes = filter_condition(filter, &mut values);
        let sql = format!(
            "SELECT f.id FROM file_names n JOIN files f ON f.id = n.rowid WHERE file_names MATCH ? AND {passes}"
        );
        let mut statement = self.conn.prepare_cached(&sql)?;
        let found = statement
            .query_map(params_from_iter(values), |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
        Ok(found)
    }

    pub fn named_exactly(&self, name: &str, filter: Filter) -> Result<Vec<i64>> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(Vec::new());
        }
        let escaped = name.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let mut values: Vec<Value> = vec![name.to_owned().into(), format!("{escaped}.%").into()];
        let passes = filter_condition(filter, &mut values);
        let sql = format!(
            "SELECT f.id FROM files f WHERE (f.name = ? COLLATE NOCASE OR f.name LIKE ? ESCAPE '\\') AND {passes}"
        );
        let mut statement = self.conn.prepare_cached(&sql)?;
        let found = statement
            .query_map(params_from_iter(values), |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
        Ok(found)
    }

    pub fn code_of_size(&self, size: u64, except: i64) -> Result<Vec<(i64, PathBuf)>> {
        let mut statement = self
            .conn
            .prepare_cached("SELECT id, dir_id, name FROM files WHERE category = ?1 AND size = ?2 AND id != ?3")?;
        let rows: Vec<(i64, i64, String)> = statement
            .query_map(params![category_code(Category::Code), size as i64, except], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        rows.into_iter()
            .map(|(id, dir, name)| Ok((id, dir_path(&self.conn, dir)?.join(name))))
            .collect()
    }

    pub fn like_code(&mut self, path: &Path, line: u64, least: f32, most: usize) -> Result<Vec<(PathBuf, u64, f32)>> {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Ok(Vec::new());
        };
        let Some(dir) = find_dir(&self.conn, parent)? else {
            return Ok(Vec::new());
        };
        let found: Option<(i64, i64)> = self
            .conn
            .prepare_cached("SELECT f.id, e.slot FROM files f JOIN embeddings e ON e.file_id = f.id WHERE f.dir_id = ?1 AND f.name = ?2 AND e.locator = ?3 AND e.config_id = ?4")?
            .query_row(params![dir, name.to_string_lossy(), line as i64, CONFIG_ID], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let Some((file, slot)) = found else {
            return Ok(Vec::new());
        };
        let mut this = None;
        self.vectors
            .read_each(&[slot as u64], &mut |_, record| this = Some(to_floats(record)))?;
        let Some(this) = this else { return Ok(Vec::new()) };
        let mut candidates: Vec<(u64, i64, u64)> = self
            .conn
            .prepare_cached("SELECT e.slot, e.file_id, e.locator FROM embeddings e JOIN files f ON f.id = e.file_id WHERE f.category = ?1 AND e.config_id = ?2 AND f.id != ?3")?
            .query_map(params![category_code(Category::Code), CONFIG_ID, file], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?, r.get::<_, i64>(2)? as u64)))?
            .collect::<rusqlite::Result<_>>()?;
        candidates.sort_unstable();
        let slots: Vec<u64> = candidates.iter().map(|&(slot, ..)| slot).collect();
        let mut best: HashMap<i64, (u64, f32)> = HashMap::new();
        let mut next = 0;
        self.vectors.read_each(&slots, &mut |_, record| {
            let (_, other, locator) = candidates[next];
            next += 1;
            let score = similarity(&this, record);
            if score >= least {
                let entry = best.entry(other).or_insert((locator, score));
                if score > entry.1 {
                    *entry = (locator, score);
                }
            }
        })?;
        let mut ranked: Vec<(i64, u64, f32)> = best
            .into_iter()
            .map(|(file, (locator, score))| (file, locator, score))
            .collect();
        ranked.sort_by(|a, b| b.2.total_cmp(&a.2));
        ranked.truncate(most);
        let mut out = Vec::new();
        for (file, locator, score) in ranked {
            let (dir, name): (i64, String) =
                self.conn
                    .query_row("SELECT dir_id, name FROM files WHERE id = ?1", [file], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })?;
            out.push((dir_path(&self.conn, dir)?.join(name), locator, score));
        }
        Ok(out)
    }

    pub fn page_texts(&self, path: &Path) -> Result<PageTexts> {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Ok(Vec::new());
        };
        let Some(dir) = find_dir(&self.conn, parent)? else {
            return Ok(Vec::new());
        };
        let mut statement = self.conn.prepare_cached(
            "SELECT p.page, p.text FROM files f JOIN page_texts p ON p.file_id = f.id WHERE f.dir_id = ?1 AND f.name = ?2 ORDER BY p.page",
        )?;
        let rows = statement.query_map(params![dir, name.to_string_lossy()], |r| {
            Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn push_filter(filter: Filter, sql: &mut String, values: &mut Vec<Value>) {
    sql.push_str(" AND ");
    sql.push_str(&filter_condition(filter, values));
}

fn filter_condition(filter: Filter, values: &mut Vec<Value>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(category) = filter.category {
        parts.push("f.category = ?".into());
        values.push(category_code(category).into());
    }
    if let Some(from) = filter.from_ms {
        parts.push("f.modified_ms >= ?".into());
        values.push(from.into());
    }
    if let Some(to) = filter.to_ms {
        parts.push("f.modified_ms < ?".into());
        values.push(to.into());
    }
    let code = filter.code;
    if !code.is_none() {
        parts.push("f.category = ?".into());
        values.push(category_code(Category::Code).into());
    }
    if let Some(language) = code.language {
        let (extensions, whole) = crate::embed::code::names_of(language);
        let mut either: Vec<&str> = Vec::new();
        for extension in extensions {
            either.push("f.name LIKE ? ESCAPE '\\'");
            values.push(format!("%.{}", extension.replace('_', "\\_").replace('%', "\\%")).into());
        }
        for name in whole {
            either.push("f.name = ?");
            values.push(name.to_owned().into());
        }
        parts.push(if either.is_empty() {
            "0".into()
        } else {
            format!("({})", either.join(" OR "))
        });
    }
    if let Some(project) = code.project {
        parts.push(format!("f.dir_id IN ({BELOW_FROM_ONE})"));
        values.push(project.into());
    }
    if code.tests {
        let names = TEST_NAMES
            .iter()
            .map(|_| "f.name GLOB ?")
            .collect::<Vec<_>>()
            .join(" OR ");
        values.extend(TEST_NAMES.iter().map(|pattern| Value::from((*pattern).to_owned())));
        let folders = TEST_FOLDERS.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        values.extend(TEST_FOLDERS.iter().map(|name| Value::from((*name).to_owned())));
        let is_test = format!(
            "({names} OR f.dir_id IN (WITH RECURSIVE below(id) AS (SELECT id FROM dirs WHERE name IN ({folders}) UNION ALL SELECT d.id FROM dirs d JOIN below ON d.parent_id = below.id WHERE d.id != d.parent_id) SELECT id FROM below))"
        );
        parts.push(is_test);
    }
    if parts.is_empty() {
        "1".to_owned()
    } else {
        parts.join(" AND ")
    }
}

const BELOW_FROM_ONE: &str = "WITH RECURSIVE below(id) AS (SELECT ? UNION ALL SELECT d.id FROM dirs d JOIN below ON d.parent_id = below.id WHERE d.id != d.parent_id) SELECT id FROM below";

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
    pub reread: HashSet<String>,
}

fn read_records(vectors: &mut VectorFile, slots: impl Iterator<Item = u64>) -> Result<HashMap<u64, Record>> {
    let mut slots: Vec<u64> = slots.collect();
    slots.sort_unstable();
    let mut records = HashMap::with_capacity(slots.len());
    vectors.read_each(&slots, &mut |slot, record| {
        records.insert(slot, *record);
    })?;
    Ok(records)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reclaimable {
    pub free_slots: u64,
    pub slots: u64,
    pub free_pages: u64,
    pub pages: u64,
    pub page_size: u64,
}

impl Reclaimable {
    pub fn live_bytes(&self) -> u64 {
        let vectors = self.slots.saturating_sub(self.free_slots);
        let pages = self.pages.saturating_sub(self.free_pages);
        crate::vectors::HEADER_BYTES + vectors * crate::vectors::RECORD_BYTES as u64 + pages * self.page_size
    }
}

struct Move {
    from: u64,
    to: u64,
    file_id: i64,
    locator: i64,
    record: Record,
}

pub struct Compaction {
    moves: Vec<Move>,
    flush: fs::File,
}

impl Compaction {
    pub fn flush(&self) -> io::Result<()> {
        self.flush.sync_data()
    }
}

pub fn checkpoint(dir: &Path) {
    let Ok(conn) = Connection::open(dir.join("mesa.db")) else {
        return;
    };
    let _ = conn.busy_timeout(std::time::Duration::ZERO);
    let _ = conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(()));
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
}

fn vectors_path(dir: &Path) -> PathBuf {
    dir.join(format!("vectors-{CONFIG_ID}.bin"))
}

fn open_db(dir: &Path) -> Result<Connection> {
    let conn = Connection::open(dir.join("mesa.db"))?;
    conn.create_scalar_function(
        "mesa_fold",
        1,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8 | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |context| {
            let name: String = context.get(0)?;
            Ok(words::fold(&name))
        },
    )?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL; PRAGMA foreign_keys = ON; PRAGMA mmap_size = 1073741824;",
    )?;
    conn.execute_batch("PRAGMA journal_size_limit = 1048576;")?;
    let auto_vacuum: i64 = conn.query_row("PRAGMA auto_vacuum", [], |r| r.get(0))?;
    if auto_vacuum != 2 {
        conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL; VACUUM;")?;
    }
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
    conn.execute_batch(FREED_NOW)?;
    Ok(conn)
}

const FREED_NOW: &str = "
CREATE TEMP TABLE freed_now (slot INTEGER PRIMARY KEY);
CREATE TEMP TRIGGER embeddings_freed_now AFTER DELETE ON main.embeddings
BEGIN
    INSERT OR IGNORE INTO freed_now (slot) VALUES (old.slot);
END;
";

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

fn heal_free_slots(conn: &Connection) -> Result<()> {
    let slots = extent(conn)? as i64;
    let held = |table: &str| {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE config_id = ?1"),
            [CONFIG_ID],
            |r| r.get::<_, i64>(0),
        )
    };
    if slots > held("embeddings")? + held("free_slots")? {
        conn.execute(
            "WITH RECURSIVE slot(n) AS (SELECT 0 WHERE ?2 > 0 UNION ALL SELECT n + 1 FROM slot WHERE n + 1 < ?2)
             INSERT INTO free_slots (config_id, slot)
             SELECT ?1, n FROM slot
             WHERE n NOT IN (SELECT slot FROM embeddings WHERE config_id = ?1)
               AND n NOT IN (SELECT slot FROM free_slots WHERE config_id = ?1)",
            params![CONFIG_ID, slots],
        )?;
    }
    Ok(())
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
        Category::Code => 4,
    }
}

fn category_from_code(code: i64) -> Option<Category> {
    Some(match code {
        0 => Category::Image,
        1 => Category::Video,
        2 => Category::Audio,
        3 => Category::Document,
        4 => Category::Code,
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

fn save_changes<'a>(
    conn: &mut Connection,
    table: &str,
    insert: &str,
    gone: impl Iterator<Item = &'a PathBuf>,
    new: impl Iterator<Item = &'a PathBuf>,
) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut delete = tx.prepare(&format!("DELETE FROM {table} WHERE path = ?1"))?;
        for path in gone {
            delete.execute(params![path.to_string_lossy()])?;
        }
        let mut insert = tx.prepare(insert)?;
        for path in new {
            insert.execute(params![path.to_string_lossy()])?;
        }
    }
    Ok(tx.commit()?)
}

#[derive(Default)]
struct Folders(HashMap<PathBuf, Option<i64>>);

impl Folders {
    fn find(&mut self, conn: &Connection, path: &Path) -> Result<Option<i64>> {
        if let Some(&found) = self.0.get(path) {
            return Ok(found);
        }
        let found = find_dir(conn, path)?;
        self.0.insert(path.to_owned(), found);
        Ok(found)
    }

    fn find_child(&mut self, conn: &Connection, path: &Path) -> Result<Option<i64>> {
        match (path.parent(), path.components().next_back()) {
            (Some(parent), Some(Component::Normal(name))) if !segments(parent).is_empty() => {
                match self.find(conn, parent)? {
                    Some(dir) => child_dir(conn, dir, &name.to_string_lossy()),
                    None => Ok(None),
                }
            }
            _ => find_dir(conn, path),
        }
    }
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

fn write_cloud(
    tx: &Transaction,
    dir_id: &mut Option<i64>,
    scan: &ScannedDir,
    wanted: &dyn Fn(&Path) -> bool,
) -> Result<()> {
    let cloud: Vec<&(String, Category)> = scan
        .cloud
        .iter()
        .filter(|(name, _)| wanted(&scan.path.join(name)))
        .collect();
    if dir_id.is_none() && !cloud.is_empty() {
        *dir_id = Some(ensure_dir(tx, &scan.path)?);
    }
    let Some(dir) = *dir_id else {
        return Ok(());
    };
    if scan.complete {
        tx.prepare_cached("DELETE FROM cloud_files WHERE dir_id = ?1")?
            .execute([dir])?;
    } else {
        for file in &scan.files {
            tx.prepare_cached("DELETE FROM cloud_files WHERE dir_id = ?1 AND name = ?2")?
                .execute(params![dir, file.name])?;
        }
        for (name, _) in &cloud {
            tx.prepare_cached("DELETE FROM files WHERE dir_id = ?1 AND name = ?2")?
                .execute(params![dir, name])?;
        }
    }
    for (name, category) in cloud {
        tx.prepare_cached("INSERT OR REPLACE INTO cloud_files (dir_id, name, category) VALUES (?1, ?2, ?3)")?
            .execute(params![dir, name, category_code(*category)])?;
    }
    Ok(())
}

const CLOUD_CHILDREN: &str = "
WITH RECURSIVE up(id, child) AS (
    SELECT DISTINCT dir_id, dir_id FROM cloud_files
    UNION
    SELECT d.parent_id, up.id FROM up JOIN dirs d ON d.id = up.id WHERE up.id != ?1 AND d.id != d.parent_id
)
SELECT dirs.id, dirs.name FROM up JOIN dirs ON dirs.id = up.child WHERE up.id = ?1 AND up.child != ?1";

fn cloud_children(conn: &Connection, dir: i64) -> Result<HashMap<String, CloudOnly>> {
    let mut found: HashMap<String, CloudOnly> = conn
        .prepare_cached("SELECT name FROM cloud_files WHERE dir_id = ?1")?
        .query_map([dir], |r| Ok((r.get(0)?, CloudOnly::Wholly)))?
        .collect::<rusqlite::Result<_>>()?;
    let holding: Vec<(i64, String)> = conn
        .prepare_cached(CLOUD_CHILDREN)?
        .query_map([dir], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (child, name) in holding {
        let cloud = if has_files_below(conn, child)? {
            CloudOnly::Partly
        } else {
            CloudOnly::Wholly
        };
        found.insert(name, cloud);
    }
    Ok(found)
}

fn cloud_of(conn: &Connection, path: &Path) -> Result<CloudOnly> {
    if let Some(dir) = find_dir(conn, path)? {
        let below: bool = conn
            .prepare_cached(
                "WITH RECURSIVE up(id) AS (
                     SELECT dir_id FROM cloud_files
                     UNION SELECT d.parent_id FROM up JOIN dirs d ON d.id = up.id WHERE d.id != d.parent_id
                 )
                 SELECT EXISTS (SELECT 1 FROM up WHERE id = ?1)",
            )?
            .query_row([dir], |r| r.get(0))?;
        return Ok(match (below, below && has_files_below(conn, dir)?) {
            (false, _) => CloudOnly::Not,
            (true, true) => CloudOnly::Partly,
            (true, false) => CloudOnly::Wholly,
        });
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok(CloudOnly::Not);
    };
    let Some(dir) = find_dir(conn, parent)? else {
        return Ok(CloudOnly::Not);
    };
    let cloud: bool = conn
        .prepare_cached("SELECT EXISTS (SELECT 1 FROM cloud_files WHERE dir_id = ?1 AND name = ?2)")?
        .query_row(params![dir, name.to_string_lossy()], |r| r.get(0))?;
    Ok(if cloud { CloudOnly::Wholly } else { CloudOnly::Not })
}

fn has_files_below(conn: &Connection, dir: i64) -> Result<bool> {
    Ok(conn
        .prepare_cached(
            "WITH RECURSIVE below(id) AS (
                 SELECT ?1 UNION ALL SELECT d.id FROM dirs d JOIN below ON d.parent_id = below.id WHERE d.id != d.parent_id
             )
             SELECT EXISTS (SELECT 1 FROM below JOIN files ON files.dir_id = below.id)",
        )?
        .query_row([dir], |r| r.get(0))?)
}

fn forget_missing_folders(tx: &Transaction, parent: i64, present: &[String]) -> Result<()> {
    let stored: Vec<(i64, String)> = tx
        .prepare_cached("SELECT id, name FROM dirs WHERE parent_id = ?1 AND id != ?1")?
        .query_map([parent], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    if stored.is_empty() {
        return Ok(());
    }
    let present: HashSet<String> = present.iter().map(|name| name.to_lowercase()).collect();
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
    category: i64,
    read_with: i64,
}

fn is_current(stored: &StoredFile, file: &ScannedFile, reading_pictures: bool) -> bool {
    vectors_current(stored, file) && words_current(stored, &file.name, reading_pictures)
}

fn vectors_current(stored: &StoredFile, file: &ScannedFile) -> bool {
    stored.modified_ms == file.modified_ms
        && stored.size == file.size as i64
        && stored.embedded_with == i64::from(CONFIG_ID)
}

fn words_current(stored: &StoredFile, name: &str, reading_pictures: bool) -> bool {
    stored.read_with == i64::from(WORDS_ID)
        || !has_words(stored.category)
        || (!reading_pictures && waits_for_reading(stored.category, name))
}

fn waits_for_reading(category: i64, name: &str) -> bool {
    category == category_code(Category::Image) || (category == category_code(Category::Document) && is_pdf(name))
}

fn is_pdf(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("pdf"))
}

fn has_words(category: i64) -> bool {
    category == category_code(Category::Image)
        || category == category_code(Category::Document)
        || category == category_code(Category::Code)
}

fn existing_of(conn: &Connection, dir_id: i64, scan: &ScannedDir) -> Result<HashMap<String, StoredFile>> {
    if scan.complete {
        return existing_files(conn, dir_id);
    }
    let mut statement = conn.prepare_cached(
        "SELECT id, modified_ms, size, embedded_with, category, read_with FROM files WHERE dir_id = ?1 AND name = ?2",
    )?;
    let mut found = HashMap::with_capacity(scan.files.len());
    for file in &scan.files {
        let stored = statement
            .query_row(params![dir_id, file.name], |r| {
                Ok(StoredFile {
                    id: r.get(0)?,
                    modified_ms: r.get(1)?,
                    size: r.get(2)?,
                    embedded_with: r.get(3)?,
                    category: r.get(4)?,
                    read_with: r.get(5)?,
                })
            })
            .optional()?;
        if let Some(stored) = stored {
            found.insert(file.name.clone(), stored);
        }
    }
    Ok(found)
}

fn existing_files(conn: &Connection, dir_id: i64) -> Result<HashMap<String, StoredFile>> {
    let mut statement = conn.prepare_cached(
        "SELECT name, id, modified_ms, size, embedded_with, category, read_with FROM files WHERE dir_id = ?1",
    )?;
    let rows = statement.query_map([dir_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            StoredFile {
                id: r.get(1)?,
                modified_ms: r.get(2)?,
                size: r.get(3)?,
                embedded_with: r.get(4)?,
                category: r.get(5)?,
                read_with: r.get(6)?,
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

fn write_words(
    tx: &Transaction,
    id: i64,
    category: Category,
    name: &str,
    words: Option<&str>,
    reading_pictures: bool,
) -> Result<()> {
    tx.prepare_cached("DELETE FROM picture_words WHERE rowid = ?1")?
        .execute([id])?;
    tx.prepare_cached("DELETE FROM document_words WHERE rowid = ?1")?
        .execute([id])?;
    if category == Category::Image && !reading_pictures {
        tx.prepare_cached("UPDATE files SET read_with = 0 WHERE id = ?1")?
            .execute([id])?;
        return Ok(());
    }
    let made_with = if !reading_pictures && waits_for_reading(category_code(category), name) {
        0
    } else {
        WORDS_ID
    };
    let folded = words.map(words::fold).filter(|folded| !folded.is_empty());
    let Some(folded) = folded.filter(|_| matches!(category, Category::Image | Category::Document | Category::Code))
    else {
        tx.prepare_cached("UPDATE files SET read_with = ?2 WHERE id = ?1")?
            .execute(params![id, made_with])?;
        return Ok(());
    };
    if category == Category::Image {
        tx.prepare_cached("INSERT INTO picture_words (rowid, folded, text) VALUES (?1, ?2, ?3)")?
            .execute(params![id, folded, words])?;
    } else {
        tx.prepare_cached("INSERT INTO document_words (rowid, folded) VALUES (?1, ?2)")?
            .execute(params![id, folded])?;
    }
    tx.prepare_cached("UPDATE files SET read_with = ?2 WHERE id = ?1")?
        .execute(params![id, made_with])?;
    Ok(())
}

fn write_page_texts(tx: &Transaction, id: i64, pages: &PageTexts) -> Result<()> {
    tx.prepare_cached("DELETE FROM page_texts WHERE file_id = ?1")?
        .execute([id])?;
    for (page, text) in pages {
        tx.prepare_cached("INSERT OR REPLACE INTO page_texts (file_id, page, text) VALUES (?1, ?2, ?3)")?
            .execute(params![id, *page as i64, text])?;
    }
    Ok(())
}

fn take_slot(tx: &Transaction, next_slot: &mut u64, reused_up_to: &mut Option<i64>) -> Result<u64> {
    let freed: Option<i64> = tx
        .prepare_cached(
            "DELETE FROM free_slots
             WHERE config_id = ?1 AND slot = (
                 SELECT slot FROM free_slots
                 WHERE config_id = ?1 AND slot > ?2 AND slot NOT IN (SELECT slot FROM temp.freed_now)
                 ORDER BY slot LIMIT 1
             )
             RETURNING slot",
        )?
        .query_row(params![CONFIG_ID, reused_up_to.unwrap_or(-1)], |r| r.get(0))
        .optional()?;
    if freed.is_some() {
        *reused_up_to = freed;
    }
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
            words: None,
            pages: Vec::new(),
            words_only: false,
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
            cloud: Vec::new(),
            path: folder(name),
            files,
            complete: true,
            subfolders: None,
        }
    }

    fn write(store: &mut Store, scans: &[ScannedDir]) {
        store.write_scans(scans, &|_| true, 0).unwrap();
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
    fn reads_the_database_through_memory() {
        let store = Store::open(&temp_dir("mmap")).unwrap();
        let mapped: i64 = store.conn.query_row("PRAGMA mmap_size", [], |r| r.get(0)).unwrap();
        assert_eq!(mapped, 1 << 30);
    }

    #[test]
    fn reuses_a_freed_slot_only_in_a_later_save() {
        let mut store = Store::open(&temp_dir("slot-reuse")).unwrap();
        let synchronous: i64 = store.conn.query_row("PRAGMA synchronous", [], |r| r.get(0)).unwrap();
        assert_eq!(synchronous, 2);
        let partial = |files| ScannedDir {
            complete: false,
            ..scan("p", files)
        };

        write(&mut store, &[partial(vec![file("a.jpg", 1)])]);
        assert_eq!(slot_of(&store, "p", "a.jpg"), 0);

        write(&mut store, &[partial(vec![file("a.jpg", 2), file("b.jpg", 1)])]);
        assert_ne!(slot_of(&store, "p", "a.jpg"), 0);
        assert_ne!(slot_of(&store, "p", "b.jpg"), 0);
        assert_eq!(count(&store, "free_slots"), 1);

        write(&mut store, &[partial(vec![file("c.jpg", 1)])]);
        assert_eq!(slot_of(&store, "p", "c.jpg"), 0);
        assert_eq!(count(&store, "free_slots"), 0);
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
        assert_eq!(count(&store, "free_slots"), 3);
        assert_eq!(store.vectors.next_slot(), 5);
        write(
            &mut store,
            &[scan("a", vec![in_parts("song.mp3", 3, &[0, 25_000, 50_000, 75_000])])],
        );
        assert_eq!((count(&store, "embeddings"), count(&store, "free_slots")), (4, 2));
        assert_eq!(store.vectors.next_slot(), 6);
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
        code: CodeFilter::NONE,
    };

    fn top(store: &mut Store, query: &Record, filter: Filter, limit: usize) -> Result<Vec<Hit>> {
        let ranked = store.rank(query, filter, &HashMap::new())?.files;
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

    fn numbered(from: usize, to: usize) -> Vec<ScannedFile> {
        (from..to).map(|i| file(&format!("f{i}.jpg"), 1)).collect()
    }

    fn vector_file_bytes(dir: &Path) -> u64 {
        fs::metadata(vectors_path(dir)).unwrap().len()
    }

    #[test]
    fn compaction_moves_tail_vectors() {
        let dir = temp_dir("compact");
        let mut store = Store::open(&dir).unwrap();
        write(&mut store, &[scan("a", numbered(0, 40))]);
        write(&mut store, &[scan("a", numbered(30, 40))]);
        assert_eq!((count(&store, "embeddings"), count(&store, "free_slots")), (10, 30));
        let before: Vec<_> = (30..40)
            .map(|i| store.vectors.read(slot_of(&store, "a", &format!("f{i}.jpg"))).unwrap())
            .collect();
        let long = vector_file_bytes(&dir);

        let mut steps = 0;
        while let Some(copied) = store.begin_compaction(8).unwrap() {
            copied.flush().unwrap();
            assert!(store.finish_compaction(copied).unwrap() > 0);
            steps += 1;
            assert!(steps < 100);
        }
        assert!(steps > 1, "in pieces: {steps}");
        assert_eq!(store.trim_vectors().unwrap(), 30);

        assert_eq!((count(&store, "embeddings"), count(&store, "free_slots")), (10, 0));
        assert_eq!(store.vectors.next_slot(), 10);
        assert_eq!(
            vector_file_bytes(&dir),
            vectors::HEADER_BYTES + 10 * vectors::RECORD_BYTES as u64
        );
        assert!(vector_file_bytes(&dir) < long);
        for (i, vector) in (30..40).zip(&before) {
            let slot = slot_of(&store, "a", &format!("f{i}.jpg"));
            assert!(slot < 10, "slot {slot}");
            assert_eq!(&store.vectors.read(slot).unwrap(), vector, "f{i}.jpg");
        }
        drop(store);
        let again = Store::open(&dir).unwrap();
        assert_eq!((count(&again, "embeddings"), again.vectors.next_slot()), (10, 10));
        assert_eq!(again.reclaimable().unwrap().free_slots, 0);
    }
}
