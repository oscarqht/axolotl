//! Shared, local visit log. Session state and native Back/Forward are independent.
use rusqlite::{Connection, params};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

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

pub struct BrowsingHistory(Connection);
impl BrowsingHistory {
    pub fn restore() -> anyhow::Result<Self> {
        match crate::persistence::data_dir() {
            Some(dir) => {
                std::fs::create_dir_all(&dir)?;
                Self::open(&dir.join("history.sqlite3"))
            }
            None => Self::from_connection(Connection::open_in_memory()?),
        }
    }
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }
    fn from_connection(connection: Connection) -> anyhow::Result<Self> {
        connection.busy_timeout(std::time::Duration::from_millis(250))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS visits (
                id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, url TEXT NOT NULL,
                visited_at INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS visits_time ON visits(visited_at);
            PRAGMA user_version=1;",
        )?;
        Ok(Self(connection))
    }
    pub fn record(&self, url: &str, title: &str, time: i64) -> anyhow::Result<Option<i64>> {
        if !url.to_ascii_lowercase().starts_with("http://")
            && !url.to_ascii_lowercase().starts_with("https://")
        {
            return Ok(None);
        }
        self.0.execute(
            "INSERT INTO visits(title,url,visited_at) VALUES (?1,?2,?3)",
            params![title, url, time],
        )?;
        Ok(Some(self.0.last_insert_rowid()))
    }
    pub fn title(&self, visit: i64, title: &str) -> anyhow::Result<()> {
        self.0.execute(
            "UPDATE visits SET title=?1 WHERE id=?2",
            params![title, visit],
        )?;
        Ok(())
    }
    pub fn clear(&self) -> anyhow::Result<()> {
        self.0.execute("DELETE FROM visits", [])?;
        Ok(())
    }
    pub fn prune(&self, days: Option<u32>, time: i64) -> anyhow::Result<()> {
        if let Some(days) = days {
            self.0.execute(
                "DELETE FROM visits WHERE visited_at < ?1",
                [time.saturating_sub(i64::from(days) * 86400)],
            )?;
        }
        Ok(())
    }
    #[cfg(feature = "smoke")]
    pub fn count(&self) -> anyhow::Result<i64> {
        Ok(self
            .0
            .query_row("SELECT count(*) FROM visits", [], |row| row.get(0))?)
    }
    pub fn search(&self, query: &str) -> anyhow::Result<Vec<HistoryItem>> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Ok(vec![]);
        }
        // Rust lowercase handles Unicode; literal substring matching keeps %, _ and
        // quotes literal. Deduplicate before matching so only the latest title wins.
        let mut stmt = self
            .0
            .prepare("SELECT title,url FROM visits ORDER BY visited_at DESC,id DESC")?;
        let mut rows = stmt.query([])?;
        let mut seen = std::collections::HashSet::new();
        let mut matches = Vec::new();
        while let Some(row) = rows.next()? {
            let title: String = row.get(0)?;
            let url: String = row.get(1)?;
            if seen.insert(url.clone())
                && (title.to_lowercase().contains(&query) || url.to_lowercase().contains(&query))
            {
                matches.push(HistoryItem { title, url });
                if matches.len() == 8 {
                    break;
                }
            }
        }
        Ok(matches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistence_repeats_titles_matching_retention_and_clear() {
        let path =
            std::env::temp_dir().join(format!("axolotl-history-{}.sqlite3", uuid::Uuid::new_v4()));
        {
            let db = BrowsingHistory::open(&path).unwrap();
            assert!(db.record("about:blank", "internal", 0).unwrap().is_none());
            assert!(db.record("file:///tmp/x", "file", 0).unwrap().is_none());
            db.record("https://one.test/", "Old", 0).unwrap();
            let id = db.record("https://one.test/", "", 86400).unwrap().unwrap();
            db.title(id, "Über 100%_'").unwrap();
            db.record("http://two.test/", "Other", 86401).unwrap();
            assert_eq!(
                db.0.query_row("SELECT count(*) FROM visits", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                3
            );
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
        let _ = std::fs::remove_file(path);
    }
}
