//! The authoritative durable store. Legacy files are read once and left untouched
//! as recovery backups; the import and its completion marker commit together.
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{path::Path, time::Duration};

const SCHEMA_VERSION: i64 = 1;
const LEGACY: &[(&str, &str)] = &[
    ("session", "session.json"),
    ("settings", "settings.json"),
    ("window", "window-state.json"),
    ("sleep", "sleep/state.json"),
    ("protection", "protection.json"),
];

pub(crate) struct AppDatabase(Connection);

impl AppDatabase {
    pub fn open(directory: &Path) -> Result<Self> {
        std::fs::create_dir_all(directory).context("create app data directory")?;
        let mut db = Self::from_connection(Connection::open(directory.join("app.sqlite3"))?)?;
        db.import_legacy(directory)?;
        Ok(db)
    }

    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    pub fn from_connection(mut connection: Connection) -> Result<Self> {
        connection.busy_timeout(Duration::from_secs(2))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(
            version <= SCHEMA_VERSION,
            "unsupported app database version {version}"
        );
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        if version == 0 {
            let transaction =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            // Another opener may have completed the schema while we waited.
            let version: i64 =
                transaction.pragma_query_value(None, "user_version", |r| r.get(0))?;
            ensure!(
                version <= SCHEMA_VERSION,
                "unsupported app database version {version}"
            );
            if version == 0 {
                transaction.execute_batch(
                    "CREATE TABLE app_state (key TEXT PRIMARY KEY, json TEXT NOT NULL);
                 CREATE TABLE migration_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE visits (
                    id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL,
                    url TEXT NOT NULL, visited_at INTEGER NOT NULL);
                 CREATE INDEX visits_time ON visits(visited_at);
                 PRAGMA user_version=1;",
                )?;
            }
            transaction.commit()?;
        }
        Ok(Self(connection))
    }

    pub fn load(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .0
            .query_row("SELECT json FROM app_state WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// One transaction pairs the workspace with metadata referring to its IDs.
    #[cfg(test)]
    pub fn write_batch(&mut self, entries: &[(String, String)]) -> Result<()> {
        self.write_transaction(entries, &[])
    }

    /// Preserve operation order for visits, title changes, retention and clear,
    /// committing them together with the latest durable application records.
    pub fn write_transaction(
        &mut self,
        entries: &[(String, String)],
        history: &[(String, Vec<rusqlite::types::Value>)],
    ) -> Result<()> {
        let transaction = self.0.transaction()?;
        for (key, json) in entries {
            validate_state(key, json)?;
            transaction.execute(
                "INSERT INTO app_state(key,json) VALUES (?1,?2)
                 ON CONFLICT(key) DO UPDATE SET json=excluded.json",
                params![key, json],
            )?;
        }
        for (sql, values) in history {
            transaction.execute(sql, rusqlite::params_from_iter(values))?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn connection(&self) -> &Connection {
        &self.0
    }

    fn import_legacy(&mut self, directory: &Path) -> Result<()> {
        // Recheck under the write transaction, so competing opens cannot import
        // twice. A crash at any point rolls back both rows and marker.
        let transaction = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if transaction
            .query_row(
                "SELECT value FROM migration_metadata WHERE key='legacy_import'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .is_some()
        {
            return Ok(());
        }
        for (key, relative) in LEGACY {
            // A pre-existing authoritative row takes priority during recovery.
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM app_state WHERE key=?1)",
                [key],
                |r| r.get(0),
            )?;
            if exists {
                continue;
            }
            let path = directory.join(relative);
            let json = match std::fs::read_to_string(&path) {
                Ok(json) => json,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error).with_context(|| format!("read legacy {relative}")),
            };
            validate_state(key, &json)
                .with_context(|| format!("import legacy {relative}; original retained"))?;
            transaction.execute(
                "INSERT INTO app_state(key,json) VALUES (?1,?2)",
                params![key, json],
            )?;
        }
        let history = directory.join("history.sqlite3");
        match std::fs::metadata(&history) {
            Ok(_) => {
                // SQLite itself reads the legacy WAL, including committed visits
                // not checkpointed into the main file. Never copy raw DB bytes.
                let legacy =
                    Connection::open_with_flags(&history, OpenFlags::SQLITE_OPEN_READ_ONLY)
                        .context("open legacy history; original retained")?;
                legacy.busy_timeout(Duration::from_secs(2))?;
                let mut statement =
                    legacy.prepare("SELECT id,title,url,visited_at FROM visits ORDER BY id")?;
                let mut rows = statement.query([])?;
                while let Some(row) = rows.next()? {
                    transaction
                        .execute(
                            "INSERT INTO visits(id,title,url,visited_at) VALUES (?1,?2,?3,?4)",
                            params![
                                row.get::<_, i64>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, i64>(3)?
                            ],
                        )
                        .context("import legacy history; original retained")?;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("inspect legacy history"),
        }
        transaction.execute(
            "INSERT INTO migration_metadata(key,value) VALUES ('legacy_import','complete')",
            [],
        )?;
        transaction
            .commit()
            .context("commit legacy app data import")?;
        Ok(())
    }
}

pub(crate) fn validate_state(key: &str, json: &str) -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    ensure!(value.is_object(), "{key} must be a JSON object");
    if key == "session" {
        workspace_core::Workspace::from_json(json)?;
    }
    if key == "sleep" {
        for (id, entry) in value.as_object().unwrap() {
            id.parse::<u64>()
                .context("sleep entry has invalid tab ID")?;
            ensure!(entry.is_object(), "sleep entry must be a JSON object");
            for field in ["sleeping", "keep_alive"] {
                if let Some(value) = entry.get(field) {
                    ensure!(value.is_boolean(), "sleep {field} must be a boolean");
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Directory(std::path::PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("axolotl-db-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn imports_all_state_and_wal_history_once_preserving_originals() {
        let directory = Directory::new();
        let session = workspace_core::Workspace::new("https://example.com/").to_json();
        std::fs::create_dir(directory.0.join("sleep")).unwrap();
        for (key, path) in LEGACY {
            std::fs::write(
                directory.0.join(path),
                if *key == "session" { &session } else { "{}" },
            )
            .unwrap();
        }
        let legacy = Connection::open(directory.0.join("history.sqlite3")).unwrap();
        legacy.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
            CREATE TABLE visits(id INTEGER PRIMARY KEY AUTOINCREMENT,title TEXT NOT NULL,url TEXT NOT NULL,visited_at INTEGER NOT NULL);
            INSERT INTO visits VALUES (9,'Preserved','https://example.com/',123);").unwrap();
        let db = AppDatabase::open(&directory.0).unwrap();
        for (key, path) in LEGACY {
            let original = std::fs::read_to_string(directory.0.join(path)).unwrap();
            assert_eq!(db.load(key).unwrap().unwrap(), original);
        }
        assert_eq!(
            db.0.query_row("SELECT id FROM visits", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            9
        );
        drop(db);
        // Once imported, stale backup edits cannot override authoritative state
        // or restore cleared history.
        let db = AppDatabase::open(&directory.0).unwrap();
        db.0.execute("DELETE FROM visits", []).unwrap();
        drop(db);
        std::fs::write(directory.0.join("session.json"), "invalid stale backup").unwrap();
        let db = AppDatabase::open(&directory.0).unwrap();
        assert_eq!(db.load("session").unwrap().unwrap(), session);
        assert_eq!(
            db.0.query_row("SELECT count(*) FROM visits", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            legacy
                .query_row("SELECT count(*) FROM visits", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn failed_import_rolls_back_and_retries_without_duplicates() {
        let directory = Directory::new();
        let session = workspace_core::Workspace::new("about:blank").to_json();
        std::fs::write(directory.0.join("session.json"), &session).unwrap();
        std::fs::write(directory.0.join("settings.json"), "broken").unwrap();
        assert!(AppDatabase::open(&directory.0).is_err());
        let connection = Connection::open(directory.0.join("app.sqlite3")).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM app_state", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM migration_metadata", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        std::fs::write(directory.0.join("settings.json"), "{}").unwrap();
        let db = AppDatabase::open(&directory.0).unwrap();
        assert_eq!(db.load("session").unwrap().unwrap(), session);
        assert_eq!(
            std::fs::read_to_string(directory.0.join("settings.json")).unwrap(),
            "{}"
        );
    }

    #[test]
    fn failed_batch_keeps_previous_related_state() {
        let mut db = AppDatabase::in_memory().unwrap();
        db.write_batch(&[
            ("settings".into(), "{\"old\":true}".into()),
            ("sleep".into(), "{}".into()),
        ])
        .unwrap();
        assert!(
            db.write_batch(&[
                ("settings".into(), "{\"new\":true}".into()),
                ("sleep".into(), "broken".into())
            ])
            .is_err()
        );
        assert_eq!(db.load("settings").unwrap().unwrap(), "{\"old\":true}");
        assert_eq!(db.load("sleep").unwrap().unwrap(), "{}");
    }

    #[test]
    fn future_schema_is_rejected_without_modification() {
        let directory = Directory::new();
        let connection = Connection::open(directory.0.join("app.sqlite3")).unwrap();
        connection.execute_batch("PRAGMA user_version=99;").unwrap();
        assert!(AppDatabase::open(&directory.0).is_err());
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            99
        );
    }

    #[test]
    fn interrupted_import_is_rolled_back_before_retry() {
        let directory = Directory::new();
        let json = workspace_core::Workspace::new("https://retry.example/").to_json();
        std::fs::write(directory.0.join("session.json"), &json).unwrap();
        let connection = Connection::open(directory.0.join("app.sqlite3")).unwrap();
        let mut db = AppDatabase::from_connection(connection).unwrap();
        {
            let transaction = db.0.transaction().unwrap();
            transaction
                .execute("INSERT INTO app_state VALUES ('session','interrupted')", [])
                .unwrap();
            // Dropping the transaction simulates termination before its commit.
        }
        drop(db);
        let db = AppDatabase::open(&directory.0).unwrap();
        assert_eq!(db.load("session").unwrap(), Some(json));
        assert_eq!(
            db.0.query_row("SELECT count(*) FROM migration_metadata", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn busy_save_preserves_committed_state_and_can_retry() {
        let directory = Directory::new();
        let mut db = AppDatabase::open(&directory.0).unwrap();
        db.write_batch(&[("settings".into(), "{\"old\":true}".into())])
            .unwrap();
        db.0.busy_timeout(Duration::from_millis(1)).unwrap();
        let mut blocker = Connection::open(directory.0.join("app.sqlite3")).unwrap();
        let lock = blocker
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        let update = [("settings".into(), "{\"new\":true}".into())];
        assert!(db.write_batch(&update).is_err());
        assert_eq!(db.load("settings").unwrap().unwrap(), "{\"old\":true}");
        drop(lock);
        db.write_batch(&update).unwrap();
        assert_eq!(db.load("settings").unwrap().unwrap(), "{\"new\":true}");
    }

    #[test]
    fn history_failure_rolls_back_state_and_visits_together() {
        let mut db = AppDatabase::in_memory().unwrap();
        db.write_batch(&[("settings".into(), "{\"old\":true}".into())])
            .unwrap();
        let entries = [("settings".into(), "{\"new\":true}".into())];
        let visit = (
            "INSERT INTO visits(title,url,visited_at) VALUES (?1,?2,?3)".into(),
            vec![
                "Title".to_owned().into(),
                "https://atomic.example/".to_owned().into(),
                123.into(),
            ],
        );
        assert!(
            db.write_transaction(
                &entries,
                &[
                    visit.clone(),
                    ("INSERT INTO missing_table VALUES (1)".into(), vec![])
                ]
            )
            .is_err()
        );
        assert_eq!(db.load("settings").unwrap().unwrap(), "{\"old\":true}");
        assert_eq!(
            db.0.query_row("SELECT count(*) FROM visits", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        db.write_transaction(&entries, &[visit]).unwrap();
        assert_eq!(db.load("settings").unwrap().unwrap(), "{\"new\":true}");
        assert_eq!(
            db.0.query_row("SELECT count(*) FROM visits", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn invalid_sleep_metadata_blocks_import_and_transaction() {
        for json in [
            r#"{"not-an-id":{}}"#,
            r#"{"-1":{}}"#,
            r#"{"18446744073709551616":{}}"#,
            r#"{"1":false}"#,
            r#"{"1":{"sleeping":1}}"#,
            r#"{"1":{"keep_alive":null}}"#,
        ] {
            let directory = Directory::new();
            std::fs::create_dir(directory.0.join("sleep")).unwrap();
            std::fs::write(directory.0.join("sleep/state.json"), json).unwrap();
            assert!(AppDatabase::open(&directory.0).is_err(), "accepted {json}");
            assert_eq!(
                std::fs::read_to_string(directory.0.join("sleep/state.json")).unwrap(),
                json
            );
            let mut db = AppDatabase::in_memory().unwrap();
            assert!(
                db.write_batch(&[
                    ("settings".into(), "{}".into()),
                    ("sleep".into(), json.into())
                ])
                .is_err()
            );
            assert!(db.load("settings").unwrap().is_none());
        }
        assert!(validate_state("sleep", "{}").is_ok());
        assert!(
            validate_state(
                "sleep",
                r#"{"1":{},"2":{"sleeping":true,"keep_alive":false}}"#
            )
            .is_ok()
        );
    }
}
