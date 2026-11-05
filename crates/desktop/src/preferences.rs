//! App preferences are independent of workspace/session snapshots.
use std::path::PathBuf;
use workspace_core::{NavigationError, SearchEngine, normalize_url, write_atomic};

pub struct Preferences {
    pub search_engine: SearchEngine,
    pub homepage_url: String,
    pub history_retention_days: Option<u32>,
    path: Option<PathBuf>,
}

impl Preferences {
    pub fn restore() -> Self {
        Self::load(crate::persistence::data_dir().map(|dir| dir.join("settings.json")))
    }

    fn load(path: Option<PathBuf>) -> Self {
        let value = path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
            .unwrap_or_default();
        Self {
            search_engine: value["search_engine"]
                .as_str()
                .and_then(SearchEngine::from_id)
                .unwrap_or_default(),
            homepage_url: value["homepage_url"]
                .as_str()
                .and_then(|url| normalize_homepage_url(url).ok())
                .unwrap_or_else(|| "about:blank".into()),
            history_retention_days: if value
                .get("history_retention_days")
                .is_some_and(|v| v.is_null())
            {
                None
            } else {
                Some(
                    value["history_retention_days"]
                        .as_u64()
                        .and_then(|d| u32::try_from(d).ok())
                        .filter(|d| *d > 0)
                        .unwrap_or(30),
                )
            },
            path,
        }
    }

    /// Apply immediately; surface write failures without undoing the in-memory choice.
    pub fn set_search_engine(&mut self, engine: SearchEngine) -> std::io::Result<()> {
        self.search_engine = engine;
        self.save()
    }

    /// Reject invalid addresses before applying; keep valid changes on write failure.
    pub fn set_homepage_url(&mut self, input: &str) -> anyhow::Result<()> {
        self.homepage_url = normalize_homepage_url(input)?;
        self.save()?;
        Ok(())
    }

    pub fn set_history_retention(&mut self, days: Option<u32>) -> std::io::Result<()> {
        if days == Some(0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Use Never or a positive whole number of days.",
            ));
        }
        self.history_retention_days = days;
        self.save()
    }

    fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let json = serde_json::json!({
            "search_engine": self.search_engine.id(),
            "homepage_url": self.homepage_url,
            "history_retention_days": self.history_retention_days,
        });
        write_atomic(path, json.to_string().as_bytes())
    }
}

/// Homepage values are addresses, never search phrases. Empty resets the start page.
pub(crate) fn normalize_homepage_url(input: &str) -> Result<String, NavigationError> {
    let input = input.trim();
    if input.is_empty() || input == "about:blank" {
        return Ok("about:blank".into());
    }
    if !input.to_ascii_lowercase().starts_with("https://")
        && !input.to_ascii_lowercase().starts_with("http://")
    {
        return Err(NavigationError(
            "Enter an HTTP or HTTPS URL, or about:blank for the built-in start page.".into(),
        ));
    }
    normalize_url(input)
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
    fn homepage_validation_defaults_and_independent_persistence() {
        let dir = std::env::temp_dir()
            .join(["axolotl-homepage-", &uuid::Uuid::new_v4().to_string()].concat());
        let path = dir.join("settings.json");
        let mut prefs = Preferences::load(Some(path.clone()));
        assert_eq!(prefs.homepage_url, "about:blank");
        prefs.set_homepage_url(" https://example.com ").unwrap();
        prefs.set_search_engine(SearchEngine::Bing).unwrap();
        let loaded = Preferences::load(Some(path.clone()));
        assert_eq!(loaded.homepage_url, "https://example.com/");
        assert_eq!(loaded.search_engine, SearchEngine::Bing);
        for input in [
            "a search phrase",
            "example.com",
            "javascript:alert(1)",
            "file:///tmp/test",
            "data:text/html,x",
            "about:config",
            "https://",
            "https://user:password@example.com",
            "https://example.com/\npath",
        ] {
            assert!(prefs.set_homepage_url(input).is_err(), "accepted {input}");
            assert_eq!(prefs.homepage_url, "https://example.com/");
            assert_eq!(
                Preferences::load(Some(path.clone())).homepage_url,
                "https://example.com/"
            );
        }
        for input in [
            "http://localhost:3000/",
            "HTTPS://example.com/path?q=1#part",
            "about:blank",
            "",
            "  ",
        ] {
            prefs.set_homepage_url(input).unwrap();
            let loaded = Preferences::load(Some(path.clone()));
            assert_eq!(loaded.homepage_url, prefs.homepage_url);
            assert_eq!(loaded.search_engine, SearchEngine::Bing);
        }
        for value in [
            serde_json::Value::Null,
            serde_json::json!(false),
            serde_json::json!("javascript:alert(1)"),
        ] {
            std::fs::write(
                &path,
                serde_json::json!({"search_engine":"bing", "homepage_url":value}).to_string(),
            )
            .unwrap();
            let loaded = Preferences::load(Some(path.clone()));
            assert_eq!(loaded.homepage_url, "about:blank");
            assert_eq!(loaded.search_engine, SearchEngine::Bing);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn history_retention_defaults_validates_and_persists() {
        let dir = std::env::temp_dir().join(format!("axolotl-retention-{}", uuid::Uuid::new_v4()));
        let path = dir.join("settings.json");
        let mut prefs = Preferences::load(Some(path.clone()));
        assert_eq!(prefs.history_retention_days, Some(30));
        for days in [None, Some(1), Some(365), Some(u32::MAX)] {
            prefs.set_history_retention(days).unwrap();
            assert_eq!(
                Preferences::load(Some(path.clone())).history_retention_days,
                days
            );
        }
        assert!(prefs.set_history_retention(Some(0)).is_err());
        for value in [
            serde_json::json!(0),
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!("never"),
            serde_json::json!(4294967296_u64),
        ] {
            std::fs::write(
                &path,
                serde_json::json!({"history_retention_days":value}).to_string(),
            )
            .unwrap();
            assert_eq!(
                Preferences::load(Some(path.clone())).history_retention_days,
                Some(30)
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
        assert!(prefs.set_homepage_url("https://example.com").is_err());
        assert_eq!(prefs.homepage_url, "https://example.com/");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
