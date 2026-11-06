//! One writer for durable state. A deadline belongs to the first pending change,
//! so continuous input cannot postpone a commit indefinitely.
use crate::app_database::AppDatabase;
use anyhow::{Context, Result, anyhow};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicI64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use workspace_core::Workspace;

const BATCH: Duration = Duration::from_millis(500);
static STORE: OnceLock<Result<Store, String>> = OnceLock::new();
static ERROR: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn report(error: impl std::fmt::Display) {
    let message = format!("App data could not be saved or restored: {error}");
    eprintln!("Axolotl: {message}");
    *ERROR.lock().unwrap() = Some(message);
}
pub fn error_status() -> Option<String> {
    ERROR.lock().unwrap().clone()
}
pub fn store() -> Result<&'static Store> {
    STORE
        .get_or_init(|| {
            Store::new(crate::persistence::data_dir()).map_err(|e| {
                report(format!("{e:#}"));
                format!("{e:#}")
            })
        })
        .as_ref()
        .map_err(|e| anyhow!("{e}"))
}

pub enum Value {
    Json(String),
    Workspace(Workspace),
}
impl Value {
    fn json(&self) -> String {
        match self {
            Self::Json(json) => json.clone(),
            Self::Workspace(workspace) => workspace.to_json(),
        }
    }
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}
impl From<rusqlite::types::Value> for SqlValue {
    fn from(v: rusqlite::types::Value) -> Self {
        use rusqlite::types::Value as V;
        match v {
            V::Null => Self::Null,
            V::Integer(v) => Self::Integer(v),
            V::Real(v) => Self::Real(v),
            V::Text(v) => Self::Text(v),
            V::Blob(v) => Self::Blob(v),
        }
    }
}
impl From<&SqlValue> for rusqlite::types::Value {
    fn from(v: &SqlValue) -> Self {
        match v {
            SqlValue::Null => Self::Null,
            SqlValue::Integer(v) => Self::Integer(*v),
            SqlValue::Real(v) => Self::Real(*v),
            SqlValue::Text(v) => Self::Text(v.clone()),
            SqlValue::Blob(v) => Self::Blob(v.clone()),
        }
    }
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Sql {
    statement: String,
    params: Vec<SqlValue>,
}
impl Sql {
    fn db(&self) -> (String, Vec<rusqlite::types::Value>) {
        (
            self.statement.clone(),
            self.params.iter().map(Into::into).collect(),
        )
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Recovery {
    id: String,
    state: Vec<(String, String)>,
    history: Vec<Sql>,
}
enum Command {
    Shutdown,
    History(Sql, Instant),
    Search(
        String,
        mpsc::Sender<Result<Vec<crate::browsing_history::HistoryItem>, String>>,
    ),
    Scalar(String, mpsc::Sender<Result<i64, String>>),
    Save(Vec<(String, Value)>, Instant),
    Flush(mpsc::Sender<Result<(), String>>),
}
#[derive(Clone)]
pub struct Store {
    sender: Arc<SenderWithLock>,
    cache: Arc<Mutex<BTreeMap<String, String>>>,
    next_history_id: Arc<AtomicI64>,
}
// Only one process may write a profile. The OS releases this advisory lock on
// crashes; the file itself is retained so another process cannot lock a new inode.
struct SenderWithLock {
    sender: mpsc::Sender<Command>,
    _lock: Option<std::fs::File>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl std::ops::Deref for SenderWithLock {
    type Target = mpsc::Sender<Command>;
    fn deref(&self) -> &Self::Target {
        &self.sender
    }
}
impl Drop for SenderWithLock {
    fn drop(&mut self) {
        // Join before releasing the profile lock, including the final retry or
        // recovery-file write. A new process must never race that last batch.
        let _ = self.sender.send(Command::Shutdown);
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            report("persistence worker panicked");
        }
    }
}
impl Store {
    pub fn new(directory: Option<PathBuf>) -> Result<Self> {
        let lock = directory
            .as_ref()
            .map(|dir| -> Result<_> {
                std::fs::create_dir_all(dir)?;
                let lock = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(dir.join("app.lock"))?;
                lock.try_lock()
                    .context("this profile is already open in another Axolotl process")?;
                Ok(lock)
            })
            .transpose()?;
        let mut db = match &directory {
            Some(dir) => AppDatabase::open(dir)?,
            None => AppDatabase::in_memory()?,
        };
        // A failed shutdown can leave a newer recoverable batch. Replay it before
        // exposing any state, and never discard it until its transaction commits.
        let recovery = directory.as_ref().map(|d| d.join("app-recovery.json"));
        if let Some(path) = &recovery
            && path.exists()
        {
            let entries: Recovery = serde_json::from_slice(&std::fs::read(path)?)
                .context("invalid app-recovery.json; kept for recovery")?;
            let marker = format!("recovery:{}", entries.id);
            let applied: bool = db.connection().query_row(
                "SELECT EXISTS(SELECT 1 FROM migration_metadata WHERE key=?1)",
                [&marker],
                |r| r.get(0),
            )?;
            if !applied {
                let mut sql: Vec<_> = entries.history.iter().map(Sql::db).collect();
                sql.push((
                    "INSERT INTO migration_metadata(key,value) VALUES (?1,'complete')".into(),
                    vec![marker.into()],
                ));
                db.write_transaction(&entries.state, &sql)
                    .context("replaying unsaved app data")?;
            }
            std::fs::remove_file(path)?;
        }
        let mut cache = BTreeMap::new();
        for key in ["session", "settings", "window", "sleep", "protection"] {
            if let Some(value) = db.load(key)? {
                crate::app_database::validate_state(key, &value)
                    .context("invalid saved app state; database retained and writes disabled")?;
                cache.insert(key.to_owned(), value);
            }
        }
        let next_history_id = db.connection().query_row(
            "SELECT max(COALESCE((SELECT max(id) FROM visits),0), COALESCE((SELECT seq FROM sqlite_sequence WHERE name='visits'),0)) + 1",
            [], |r| r.get::<_, i64>(0))?;
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("app-persistence".into())
            .spawn(move || run(db, receiver, recovery))?;
        Ok(Self {
            sender: Arc::new(SenderWithLock {
                sender,
                _lock: lock,
                worker: Some(worker),
            }),
            cache: Arc::new(Mutex::new(cache)),
            next_history_id: Arc::new(AtomicI64::new(next_history_id)),
        })
    }
    pub fn load(&self, key: &str) -> Option<String> {
        self.cache.lock().unwrap().get(key).cloned()
    }
    pub fn save(&self, entries: Vec<(String, Value)>) -> Result<()> {
        self.save_since(entries, Instant::now())
    }
    pub fn save_since(&self, entries: Vec<(String, Value)>, changed: Instant) -> Result<()> {
        // Small preference records stay available immediately to other consumers.
        // Workspace serialization is deliberately deferred to the writer.
        let mut cache = self.cache.lock().unwrap();
        for (key, value) in &entries {
            if let Value::Json(json) = value {
                cache.insert(key.clone(), json.clone());
            }
        }
        self.sender
            .send(Command::Save(entries, changed + BATCH))
            .map_err(|_| anyhow!("persistence worker stopped"))
    }
    pub fn reserve_history_id(&self) -> Result<i64> {
        self.next_history_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| anyhow!("History visit IDs exhausted"))
    }
    pub fn history(&self, sql: &str, params: Vec<rusqlite::types::Value>) -> Result<()> {
        self.sender
            .send(Command::History(
                Sql {
                    statement: sql.into(),
                    params: params.into_iter().map(Into::into).collect(),
                },
                Instant::now() + BATCH,
            ))
            .map_err(|_| anyhow!("persistence worker stopped"))
    }
    pub fn history_search(&self, query: &str) -> Result<Vec<crate::browsing_history::HistoryItem>> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::Search(query.into(), tx))
            .map_err(|_| anyhow!("persistence worker stopped"))?;
        rx.recv()?.map_err(|e| anyhow!(e))
    }
    pub fn history_scalar(&self, sql: &str) -> Result<i64> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::Scalar(sql.into(), tx))
            .map_err(|_| anyhow!("persistence worker stopped"))?;
        rx.recv()?.map_err(|e| anyhow!(e))
    }
    pub fn flush(&self) -> Result<()> {
        let (sender, receiver) = mpsc::channel();
        self.sender
            .send(Command::Flush(sender))
            .map_err(|_| anyhow!("persistence worker stopped"))?;
        receiver
            .recv()
            .context("persistence flush interrupted")?
            .map_err(|e| anyhow!(e))
    }
}
fn commit(
    db: &mut AppDatabase,
    pending: &mut BTreeMap<String, Value>,
    history: &mut Vec<Sql>,
    batch_id: &mut String,
    recovery: Option<&Path>,
) -> Result<()> {
    if pending.is_empty() && history.is_empty() {
        return Ok(());
    }
    let entries: Vec<_> = pending.iter().map(|(k, v)| (k.clone(), v.json())).collect();
    let mut sql: Vec<_> = history.iter().map(Sql::db).collect();
    if recovery.is_some_and(Path::exists) {
        sql.push((
            "INSERT OR IGNORE INTO migration_metadata(key,value) VALUES (?1,'complete')".into(),
            vec![format!("recovery:{batch_id}").into()],
        ));
    }
    if let Err(error) = db.write_transaction(&entries, &sql) {
        if let Some(path) = recovery {
            workspace_core::write_atomic(
                path,
                &serde_json::to_vec(&Recovery {
                    id: batch_id.clone(),
                    state: entries,
                    history: history.clone(),
                })?,
            )
            .context("database save and recovery file both failed")?;
        }
        return Err(error.context("batch retained for retry"));
    }
    pending.clear();
    history.clear();
    *ERROR.lock().unwrap() = None;
    *batch_id = uuid::Uuid::new_v4().to_string();
    if let Some(path) = recovery
        && path.exists()
    {
        std::fs::remove_file(path).context("removing committed recovery file")?;
    }
    Ok(())
}
fn run(mut db: AppDatabase, receiver: mpsc::Receiver<Command>, recovery: Option<PathBuf>) {
    let mut pending = BTreeMap::new();
    let mut history = Vec::new();
    let mut batch_id = uuid::Uuid::new_v4().to_string();
    let mut deadline: Option<Instant> = None;
    loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            if let Err(error) = commit(
                &mut db,
                &mut pending,
                &mut history,
                &mut batch_id,
                recovery.as_deref(),
            ) {
                report(format!("{error:#}"));
            }
            deadline = (!pending.is_empty() || !history.is_empty()).then(|| Instant::now() + BATCH);
        }
        let command = match deadline {
            Some(d) => match receiver.recv_timeout(d.saturating_duration_since(Instant::now())) {
                Ok(command) => command,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            },
            None => match receiver.recv() {
                Ok(command) => command,
                Err(_) => break,
            },
        };
        match command {
            Command::Shutdown => break,
            Command::Search(query, reply) => {
                let result = commit(
                    &mut db,
                    &mut pending,
                    &mut history,
                    &mut batch_id,
                    recovery.as_deref(),
                )
                .and_then(|_| {
                    let mut stmt = db
                        .connection()
                        .prepare("SELECT title,url FROM visits ORDER BY visited_at DESC,id DESC")?;
                    let mut rows = stmt.query([])?;
                    let mut seen = std::collections::HashSet::new();
                    let mut matches = Vec::new();
                    while let Some(row) = rows.next()? {
                        let title: String = row.get(0)?;
                        let url: String = row.get(1)?;
                        if seen.insert(url.clone())
                            && (title.to_lowercase().contains(&query)
                                || url.to_lowercase().contains(&query))
                        {
                            matches.push(crate::browsing_history::HistoryItem { title, url });
                            if matches.len() == 8 {
                                break;
                            }
                        }
                    }
                    Ok(matches)
                });
                let _ = reply.send(result.map_err(|e| {
                    report(format!("{e:#}"));
                    format!("{e:#}")
                }));
                deadline =
                    (!pending.is_empty() || !history.is_empty()).then(|| Instant::now() + BATCH);
            }
            Command::Scalar(sql, reply) => {
                let result = commit(
                    &mut db,
                    &mut pending,
                    &mut history,
                    &mut batch_id,
                    recovery.as_deref(),
                )
                .and_then(|_| Ok(db.connection().query_row(&sql, [], |r| r.get(0))?));
                let _ = reply.send(result.map_err(|e| {
                    report(format!("{e:#}"));
                    format!("{e:#}")
                }));
                deadline =
                    (!pending.is_empty() || !history.is_empty()).then(|| Instant::now() + BATCH);
            }
            Command::History(sql, due) => {
                history.push(sql);
                deadline = Some(deadline.map_or(due, |old| old.min(due)));
            }
            Command::Save(entries, due) => {
                pending.extend(entries);
                deadline = Some(deadline.map_or(due, |old| old.min(due)));
            }
            Command::Flush(reply) => {
                let result = commit(
                    &mut db,
                    &mut pending,
                    &mut history,
                    &mut batch_id,
                    recovery.as_deref(),
                );
                if let Err(error) = &result {
                    report(format!("{error:#}"));
                }
                let _ = reply.send(result.map_err(|e| format!("{e:#}")));
                deadline =
                    (!pending.is_empty() || !history.is_empty()).then(|| Instant::now() + BATCH);
            }
        }
    }
    if let Err(error) = commit(
        &mut db,
        &mut pending,
        &mut history,
        &mut batch_id,
        recovery.as_deref(),
    ) {
        report(format!("{error:#}"));
    }
}

#[cfg(test)]
mod tests;
