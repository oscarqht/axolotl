//! App preferences are independent of workspace/session snapshots.
use std::path::{Path, PathBuf};
use workspace_core::{SearchEngine, write_atomic};

pub struct Preferences {
    pub search_engine: SearchEngine,
    path: Option<PathBuf>,
}

impl Preferences {
    pub fn restore() -> Self {
        Self::load(crate::persistence::data_dir().map(|dir| dir.join("settings.json")))
    }

    fn load(path: Option<PathBuf>) -> Self {
        let search_engine = path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
            .and_then(|value| {
                value["search_engine"]
                    .as_str()
                    .and_then(SearchEngine::from_id)
            })
            .unwrap_or_default();
        Self {
            search_engine,
            path,
        }
    }

    /// Apply immediately; surface write failures without undoing the in-memory choice.
    pub fn set_search_engine(&mut self, engine: SearchEngine) -> std::io::Result<()> {
        self.search_engine = engine;
        if let Some(path) = &self.path {
            Self::save(path, engine)?;
        }
        Ok(())
    }

    fn save(path: &Path, engine: SearchEngine) -> std::io::Result<()> {
        let json = serde_json::json!({ "search_engine": engine.id() });
        write_atomic(path, json.to_string().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_invalid_and_unknown_preferences_default_to_google() {
        let dir = std::env::temp_dir()
            .join(["axolotl-preferences-", &uuid::Uuid::new_v4().to_string()].concat());
        let path = dir.join("settings.json");
        assert_eq!(
            Preferences::load(Some(path.clone())).search_engine,
            SearchEngine::Google
        );
        std::fs::create_dir_all(&dir).unwrap();
        for json in [
            "{bad",
            "{}",
            "null",
            r#"{"search_engine":false}"#,
            r#"{"search_engine":"unknown"}"#,
        ] {
            std::fs::write(&path, json).unwrap();
            assert_eq!(
                Preferences::load(Some(path.clone())).search_engine,
                SearchEngine::Google
            );
        }
        std::fs::write(dir.join("session.json"), "unrelated session").unwrap();
        for engine in [
            SearchEngine::Bing,
            SearchEngine::DuckDuckGo,
            SearchEngine::Google,
        ] {
            let mut prefs = Preferences::load(Some(path.clone()));
            prefs.set_search_engine(engine).unwrap();
            assert_eq!(prefs.search_engine, engine);
            assert_eq!(Preferences::load(Some(path.clone())).search_engine, engine);
            assert_eq!(
                std::fs::read_to_string(dir.join("session.json")).unwrap(),
                "unrelated session"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn write_failure_preserves_immediate_choice() {
        let dir = std::env::temp_dir()
            .join(["axolotl-preferences-", &uuid::Uuid::new_v4().to_string()].concat());
        std::fs::create_dir_all(&dir).unwrap();
        let mut prefs = Preferences::load(Some(dir.clone()));
        assert!(prefs.set_search_engine(SearchEngine::Bing).is_err());
        assert_eq!(prefs.search_engine, SearchEngine::Bing);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
