use super::*;
use rusqlite::{Connection, OptionalExtension};

fn directory() -> PathBuf {
    std::env::temp_dir().join(format!("axolotl-worker-{}", uuid::Uuid::new_v4()))
}
fn settings(value: i64) -> Vec<(String, Value)> {
    vec![(
        "settings".into(),
        Value::Json(format!(r#"{{"value":{value}}}"#)),
    )]
}
#[test]
fn coalesces_continuous_changes_without_postponing_and_flushes_immediately() {
    let dir = directory();
    let writer = Store::new(Some(dir.clone())).unwrap();
    let reader = Connection::open(dir.join("app.sqlite3")).unwrap();
    assert_eq!(
        reader
            .query_row("SELECT count(*) FROM app_state", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    let start = Instant::now();
    loop {
        // Keep changes arriving until an independent connection sees a commit.
        // A fixed sleep races the worker on busy runners; flushing or querying
        // through Store here would force a commit and hide deadline regressions.
        writer.save(settings(2)).unwrap();
        let value: Option<String> = reader
            .query_row("SELECT json FROM app_state WHERE key='settings'", [], |r| {
                r.get(0)
            })
            .optional()
            .unwrap();
        if let Some(value) = value {
            assert_eq!(value, r#"{"value":2}"#);
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "continuous changes must commit without waiting for input to stop"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    writer.save(settings(3)).unwrap();
    writer.flush().unwrap();
    assert_eq!(
        reader
            .query_row("SELECT json FROM app_state WHERE key='settings'", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        r#"{"value":3}"#
    );
    drop(writer);
    drop(reader);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn history_id_exhaustion_does_not_wrap_or_advance() {
    let writer = Store::new(None).unwrap();
    writer
        .next_history_id
        .store(i64::MAX - 1, Ordering::Relaxed);
    assert_eq!(writer.reserve_history_id().unwrap(), i64::MAX - 1);
    for _ in 0..2 {
        assert!(writer.reserve_history_id().is_err());
        assert_eq!(writer.next_history_id.load(Ordering::Relaxed), i64::MAX);
    }
}

#[test]
fn concurrent_history_reservations_are_unique_and_contiguous() {
    let writer = Store::new(None).unwrap();
    let first = writer.next_history_id.load(Ordering::Relaxed);
    let start = std::sync::Barrier::new(8);
    let mut ids = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let writer = &writer;
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    (0..1000)
                        .map(|_| writer.reserve_history_id().unwrap())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    ids.sort_unstable();
    assert_eq!(ids, (first..first + 8000).collect::<Vec<_>>());
    assert_eq!(writer.reserve_history_id().unwrap(), first + 8000);
}

#[test]
fn failed_batch_keeps_last_committed_data_and_retries() {
    let dir = directory();
    let writer = Store::new(Some(dir.clone())).unwrap();
    writer.save(settings(1)).unwrap();
    writer.flush().unwrap();
    let lock = Connection::open(dir.join("app.sqlite3")).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    writer.save(settings(2)).unwrap();
    assert!(writer.flush().is_err());
    let recovery = dir.join("app-recovery.json");
    assert!(recovery.exists());
    assert_eq!(
        lock.query_row("SELECT json FROM app_state WHERE key='settings'", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        r#"{"value":1}"#
    );
    lock.execute_batch("ROLLBACK").unwrap();
    writer.flush().unwrap();
    assert!(!recovery.exists());
    assert_eq!(
        lock.query_row("SELECT json FROM app_state WHERE key='settings'", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        r#"{"value":2}"#
    );
    drop(writer);
    drop(lock);
    let _ = std::fs::remove_dir_all(dir);
}
#[test]
fn startup_replays_recovery_once_including_history() {
    let dir = directory();
    let db = AppDatabase::open(&dir).unwrap();
    drop(db);
    let recovery = Recovery {
        id: "interrupted".into(),
        state: vec![("settings".into(), r#"{"recovered":true}"#.into())],
        history: vec![Sql {
            statement:
                "INSERT INTO visits(id,title,url,visited_at) VALUES (99,'Title','https://test',1)"
                    .into(),
            params: vec![],
        }],
    };
    let bytes = serde_json::to_vec(&recovery).unwrap();
    for _ in 0..2 {
        // Simulate a crash after commit but before removing the recovery file.
        std::fs::write(dir.join("app-recovery.json"), &bytes).unwrap();
        let writer = Store::new(Some(dir.clone())).unwrap();
        assert_eq!(writer.load("settings").unwrap(), r#"{"recovered":true}"#);
        assert_eq!(
            writer
                .history_scalar("SELECT count(*) FROM visits")
                .unwrap(),
            1
        );
        assert!(!dir.join("app-recovery.json").exists());
        drop(writer);
    }
    let _ = std::fs::remove_dir_all(dir);
}
#[test]
fn workspace_and_sleep_commit_atomically() {
    let dir = directory();
    let writer = Store::new(Some(dir.clone())).unwrap();
    writer
        .save(vec![
            ("session".into(), Value::Workspace(Workspace::default())),
            ("sleep".into(), Value::Json("invalid".into())),
        ])
        .unwrap();
    assert!(writer.flush().is_err());
    let db = AppDatabase::open(&dir).unwrap();
    assert!(db.load("session").unwrap().is_none());
    writer
        .save(vec![("sleep".into(), Value::Json("{}".into()))])
        .unwrap();
    writer.flush().unwrap();
    assert!(db.load("session").unwrap().is_some());
    drop(writer);
    drop(db);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn profile_lock_prevents_competing_writers_and_releases_on_drop() {
    let dir = directory();
    let first = Store::new(Some(dir.clone())).unwrap();
    assert!(Store::new(Some(dir.clone())).is_err());
    first.save(settings(10)).unwrap();
    drop(first); // Last owner flushes before releasing the profile lock.
    let next = Store::new(Some(dir.clone())).unwrap();
    assert_eq!(next.load("settings").unwrap(), r#"{"value":10}"#);
    drop(next);
    let _ = std::fs::remove_dir_all(dir);
}
#[test]
fn corrupt_stored_workspace_disables_writer_without_overwriting_data() {
    let dir = directory();
    let db = AppDatabase::open(&dir).unwrap();
    db.connection()
        .execute("INSERT INTO app_state VALUES ('session','{broken')", [])
        .unwrap();
    assert!(Store::new(Some(dir.clone())).is_err());
    assert_eq!(db.load("session").unwrap().unwrap(), "{broken");
    drop(db);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn delay_before_the_ui_poll_counts_toward_batch_deadline() {
    let dir = directory();
    let writer = Store::new(Some(dir.clone())).unwrap();
    // A model change can wait for the next 40ms poll before it is queued.
    writer
        .save_since(settings(1), Instant::now() - BATCH)
        .unwrap();
    let reader = Connection::open(dir.join("app.sqlite3")).unwrap();
    let start = Instant::now();
    loop {
        let count: i64 = reader
            .query_row("SELECT count(*) FROM app_state", [], |r| r.get(0))
            .unwrap();
        if count == 1 {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_millis(300),
            "queueing must not restart the 500ms batch timer"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(writer);
    drop(reader);
    let _ = std::fs::remove_dir_all(dir);
}
