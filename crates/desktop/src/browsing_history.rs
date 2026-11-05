//! Shared, local visit log. Session state and native Back/Forward are independent.
use crate::persistence_worker::Store;
use rusqlite::types::Value;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq)]
pub struct HistoryItem {
    pub title: String,
    pub url: String,
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Clonable search handle for work scheduled away from the UI thread. The shared
/// writer orders reads after queued visits and supports in-memory profiles too.
#[derive(Clone)]
pub struct HistorySearch(Store);
impl HistorySearch {
    pub fn search(&self, query: &str) -> anyhow::Result<Vec<HistoryItem>> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Ok(vec![]);
        }
        self.0.history_search(&query)
    }
}

pub struct BrowsingHistory {
    writer: Store,
}
impl BrowsingHistory {
    pub fn restore() -> anyhow::Result<Self> {
        Self::from_store(crate::persistence_worker::store()?.clone())
    }
    #[cfg(test)]
    fn open(directory: &std::path::Path) -> anyhow::Result<Self> {
        Self::from_store(Store::new(Some(directory.to_owned()))?)
    }
    fn from_store(writer: Store) -> anyhow::Result<Self> {
        Ok(Self { writer })
    }
    pub fn record(&self, url: &str, title: &str, time: i64) -> anyhow::Result<Option<i64>> {
        if !url.to_ascii_lowercase().starts_with("http://")
            && !url.to_ascii_lowercase().starts_with("https://")
        {
            return Ok(None);
        }
        // Reserve from the shared writer so multiple consumers cannot collide.
        let id = self.writer.reserve_history_id()?;
        self.writer.history(
            "INSERT INTO visits(id,title,url,visited_at) VALUES (?1,?2,?3,?4)",
            vec![
                Value::Integer(id),
                Value::Text(title.into()),
                Value::Text(url.into()),
                Value::Integer(time),
            ],
        )?;
        Ok(Some(id))
    }
    pub fn title(&self, visit: i64, title: &str) -> anyhow::Result<()> {
        self.writer.history(
            "UPDATE visits SET title=?1 WHERE id=?2",
            vec![Value::Text(title.into()), Value::Integer(visit)],
        )
    }
    pub fn clear(&self) -> anyhow::Result<()> {
        self.writer.history("DELETE FROM visits", vec![])?;
        // A confirmed clear acknowledges the actual transaction, preserving the
        // current error-reporting contract of the settings dialog.
        self.writer.flush()
    }
    pub fn prune(&self, days: Option<u32>, time: i64) -> anyhow::Result<()> {
        if let Some(days) = days {
            self.writer.history(
                "DELETE FROM visits WHERE visited_at < ?1",
                vec![Value::Integer(time.saturating_sub(i64::from(days) * 86400))],
            )?;
        }
        Ok(())
    }
    #[cfg(any(test, feature = "smoke"))]
    pub fn count(&self) -> anyhow::Result<i64> {
        self.writer.history_scalar("SELECT count(*) FROM visits")
    }
    pub fn searcher(&self) -> HistorySearch {
        HistorySearch(self.writer.clone())
    }
    #[cfg(any(test, feature = "smoke"))]
    pub fn search(&self, query: &str) -> anyhow::Result<Vec<HistoryItem>> {
        self.searcher().search(query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn background_search_handle_observes_writes_titles_and_clear() {
        let dir = std::env::temp_dir().join(format!("axolotl-reader-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = BrowsingHistory::open(&dir).unwrap();
        let searcher = db.searcher();
        let id = db
            .record("https://reader.test/", "Old", 1)
            .unwrap()
            .unwrap();
        let search = |reader: HistorySearch, query: &'static str| {
            std::thread::spawn(move || reader.search(query).unwrap())
                .join()
                .unwrap()
        };
        assert_eq!(search(searcher.clone(), "Old").len(), 1);
        db.title(id, "Über %_'").unwrap();
        assert!(search(searcher.clone(), "Old").is_empty());
        assert_eq!(search(searcher.clone(), "ÜBER %_'").len(), 1);
        db.clear().unwrap();
        assert!(search(searcher, "reader").is_empty());
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn persistence_repeats_titles_matching_retention_and_clear() {
        let path = std::env::temp_dir().join(format!("axolotl-history-{}", uuid::Uuid::new_v4()));
        {
            let db = BrowsingHistory::open(&path).unwrap();
            assert!(db.record("about:blank", "internal", 0).unwrap().is_none());
            assert!(db.record("file:///tmp/x", "file", 0).unwrap().is_none());
            db.record("https://one.test/", "Old", 0).unwrap();
            let id = db.record("https://one.test/", "", 86400).unwrap().unwrap();
            db.title(id, "Über 100%_'").unwrap();
            db.record("http://two.test/", "Other", 86401).unwrap();
            assert_eq!(db.count().unwrap(), 3);
            assert_eq!(db.search("ÜBER").unwrap()[0].title, "Über 100%_'");
            assert_eq!(db.search("%_'").unwrap().len(), 1);
            assert!(db.search("Old").unwrap().is_empty());
            assert!(db.search(" ").unwrap().is_empty());
        }
        {
            let db = BrowsingHistory::open(&path).unwrap();
            assert_eq!(
                db.search("test")
                    .unwrap()
                    .iter()
                    .map(|i| i.url.as_str())
                    .collect::<Vec<_>>(),
                vec!["http://two.test/", "https://one.test/"]
            );
            db.prune(None, i64::MAX).unwrap();
            assert_eq!(db.search("test").unwrap().len(), 2);
            db.prune(Some(1), 172801).unwrap();
            assert_eq!(db.search("test").unwrap().len(), 1);
            db.clear().unwrap();
            assert!(db.search("test").unwrap().is_empty());
            for i in 0..12 {
                db.record(&format!("https://test/{i}"), "match", 100)
                    .unwrap();
            }
            assert_eq!(db.search("match").unwrap().len(), 8);
            assert_eq!(db.search("match").unwrap()[0].url, "https://test/11");
        }
        let _ = std::fs::remove_dir_all(path);
    }
    #[test]
    fn background_visits_flush_and_ids_survive_clear_and_restart() {
        let directory =
            std::env::temp_dir().join(format!("axolotl-history-batch-{}", uuid::Uuid::new_v4()));
        let first_id;
        {
            let db = BrowsingHistory::open(&directory).unwrap();
            first_id = db
                .record("https://batched.test/", "Queued", 1)
                .unwrap()
                .unwrap();
            // The read waits behind pending writes, so a late title and immediate
            // search see the same reserved identity before the 500 ms deadline.
            db.title(first_id, "Updated before commit").unwrap();
            assert_eq!(
                db.search("updated").unwrap()[0].url,
                "https://batched.test/"
            );
            db.clear().unwrap();
            assert_eq!(db.count().unwrap(), 0);
        }
        {
            let db = BrowsingHistory::open(&directory).unwrap();
            let next_id = db.record("https://next.test/", "Next", 2).unwrap().unwrap();
            assert!(
                next_id > first_id,
                "Clearing history must not reuse visit IDs"
            );
            db.writer.flush().unwrap();
            let reader = rusqlite::Connection::open(directory.join("app.sqlite3")).unwrap();
            assert_eq!(
                reader
                    .query_row("SELECT id FROM visits", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                next_id
            );
        }
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn smoke_history_uses_the_same_in_memory_writer_for_reads() {
        let db = BrowsingHistory::from_store(Store::new(None).unwrap()).unwrap();
        db.record("https://memory.test/", "Memory", 1).unwrap();
        assert_eq!(db.count().unwrap(), 1);
        assert_eq!(db.search("memory").unwrap().len(), 1);
        db.clear().unwrap();
        assert!(db.search("memory").unwrap().is_empty());
    }
}

#[cfg(test)]
mod shared_writer_tests {
    use super::*;
    #[test]
    fn consumers_share_visit_id_reservations() {
        let writer = Store::new(None).unwrap();
        let first = BrowsingHistory::from_store(writer.clone()).unwrap();
        let second = BrowsingHistory::from_store(writer).unwrap();
        let a = first
            .record("https://first.test", "First", 1)
            .unwrap()
            .unwrap();
        let b = second
            .record("https://second.test", "Second", 1)
            .unwrap()
            .unwrap();
        assert!(b > a);
        assert_eq!(first.count().unwrap(), 2);
        first.title(a, "First updated").unwrap();
        assert_eq!(
            second.search("updated").unwrap()[0].url,
            "https://first.test"
        );
    }
}
