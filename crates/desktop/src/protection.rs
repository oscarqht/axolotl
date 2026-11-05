//! Shared policy, maintained lists and transactional updates. Native handles live separately.
#[cfg(any(test, target_os = "windows"))]
use adblock::Engine;
use adblock::lists::{FilterSet, ParseOptions, RuleTypes};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(any(test, target_os = "windows"))]
use std::sync::Mutex;

const EASYLIST: &str = include_str!("../assets/filters/easylist.txt");
const EASYPRIVACY: &str = include_str!("../assets/filters/easyprivacy.txt");
const SOURCES: [&str; 2] = [
    "https://easylist.to/easylist/easylist.txt",
    "https://easylist.to/easylist/easyprivacy.txt",
];
const MAX_LIST: usize = 8 * 1024 * 1024;
const DAY: u64 = 86400;
pub const SCRIPT_MARKER: &str = "/* Axolotl protection */";

pub fn host(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    Some(url.host_str()?.trim_end_matches('.').to_ascii_lowercase())
}

#[derive(Clone)]
pub struct Policy {
    pub enabled: bool,
    pub exceptions: BTreeSet<String>,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: true,
            exceptions: BTreeSet::new(),
        }
    }
}
impl Policy {
    pub fn allows(&self, url: &str) -> bool {
        self.enabled && host(url).is_some_and(|h| !self.exceptions.contains(&h))
    }
    fn value(&self) -> Value {
        json!({"enabled":self.enabled,"exceptions":self.exceptions})
    }
}

pub struct Rules {
    #[cfg(any(test, target_os = "windows"))]
    pub engine: Mutex<Engine>,
    pub network_json: String,
    cosmetic_lines: Vec<String>,
    pub converted: usize,
}
impl Rules {
    fn parse(lists: &[String; 2]) -> Result<Self> {
        for list in lists {
            validate(list)?;
        }
        let mut filters = FilterSet::new(true);
        for list in lists {
            filters.add_filter_list(
                list.clone(),
                ParseOptions {
                    rule_types: RuleTypes::NetworkOnly,
                    ..Default::default()
                },
            );
        }
        let (rules, used) = filters
            .clone()
            .into_content_blocking()
            .map_err(|_| anyhow::anyhow!("Filter conversion failed"))?;
        ensure!(
            !rules.is_empty() && rules.len() < 150_000,
            "Invalid native rule count"
        );
        let cosmetic_lines = lists
            .iter()
            .flat_map(|s| s.lines())
            .filter(|s| !s.starts_with('!') && (s.contains("##") || s.contains("#@#")))
            .map(str::to_owned)
            .collect();
        Ok(Self {
            #[cfg(any(test, target_os = "windows"))]
            engine: Mutex::new(Engine::new_with_filter_set(filters)),
            network_json: serde_json::to_string(&rules)?,
            cosmetic_lines,
            converted: used.len(),
        })
    }
    pub fn script(&self, policy: &Policy) -> String {
        let mut policy_value = policy.value();
        policy_value["diagnostics"] =
            json!(cfg!(feature = "smoke") && std::env::args().any(|arg| arg == "--smoke-youtube"));
        format!(
            "{SCRIPT_MARKER}\n(()=>{{const policy={};const filters={};{}\n}})();",
            policy_value,
            serde_json::to_string(&self.cosmetic_lines).unwrap(),
            include_str!("../assets/protection.js")
        )
    }
    #[cfg(any(test, target_os = "windows"))]
    pub fn blocks_request(
        &self,
        policy: &Policy,
        url: &str,
        source: &str,
        kind: &str,
        method: &str,
    ) -> bool {
        if !policy.allows(source) {
            return false;
        }
        adblock::request::Request::new(url, source, kind, method)
            .ok()
            .is_some_and(|r| {
                self.engine
                    .lock()
                    .unwrap()
                    .check_network_request(&r)
                    .should_block()
            })
    }
    #[cfg(test)]
    fn blocks(&self, policy: &Policy, url: &str, source: &str, kind: &str) -> bool {
        self.blocks_request(policy, url, source, kind, "GET")
    }
}
fn validate(list: &str) -> Result<()> {
    ensure!(
        list.len() <= MAX_LIST
            && list.starts_with("[Adblock Plus")
            && list
                .lines()
                .filter(|s| !s.starts_with('!') && !s.is_empty())
                .count()
                > 1000,
        "Incomplete or invalid filter list"
    );
    Ok(())
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

struct Candidate {
    lists: [String; 2],
    rules: Arc<Rules>,
    checked: u64,
}
pub struct Protection {
    pub policy: Policy,
    pub rules: Arc<Rules>,
    pub native: crate::protection_native::RuleStore,
    path: Option<PathBuf>,
    pending: Option<mpsc::Receiver<Result<Candidate>>>,
    candidate: Option<Candidate>,
    last_attempt: Instant,
    pub last_updated: u64,
    pub status: String,
    pub revision: u64,
    pub policy_revision: u64,
}
impl Protection {
    pub fn restore() -> Self {
        Self::load(crate::persistence::data_dir())
    }
    fn load(path: Option<PathBuf>) -> Self {
        let prefs = path
            .as_ref()
            .and_then(|_dir| {
                #[cfg(test)]
                let result = crate::persistence::load_from::<Value>(_dir, "protection");
                #[cfg(not(test))]
                let result = crate::persistence::load::<Value>("protection");
                match result {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("Axolotl: failed to restore protection preferences: {error}");
                        None
                    }
                }
            })
            .unwrap_or_default();
        let policy = Policy {
            enabled: prefs["enabled"].as_bool().unwrap_or(true),
            exceptions: prefs["exceptions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|s| host(&format!("https://{s}/")).as_deref() == Some(*s))
                .map(str::to_owned)
                .collect(),
        };
        let cached = path
            .as_ref()
            .and_then(|p| std::fs::read(p.join("filter-lists.json")).ok())
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let fallback = || [EASYLIST.to_owned(), EASYPRIVACY.to_owned()];
        let (rules, updated) = cached
            .and_then(|v| {
                let lists = [
                    v["easylist"].as_str()?.to_owned(),
                    v["easyprivacy"].as_str()?.to_owned(),
                ];
                Some((
                    Rules::parse(&lists).ok()?,
                    v["updated"].as_u64().unwrap_or(0),
                ))
            })
            .unwrap_or_else(|| {
                (
                    Rules::parse(&fallback()).expect("bundled filters are valid"),
                    0,
                )
            });
        let rules = Arc::new(rules);
        let native = crate::protection_native::RuleStore::new(&rules.network_json, &policy);
        Self {
            policy,
            rules,
            native,
            path,
            pending: None,
            candidate: None,
            last_attempt: Instant::now() - Duration::from_secs(DAY),
            last_updated: updated,
            status: if updated == 0 {
                "Bundled EasyList + EasyPrivacy".into()
            } else {
                "Using saved EasyList + EasyPrivacy".into()
            },
            revision: 0,
            policy_revision: 0,
        }
    }
    pub fn value(&self) -> Value {
        json!({"enabled":self.policy.enabled,"ready":self.native.ready(),"status":if !self.native.ready() && self.native.error().is_none() {"Preparing native protection…"} else {&self.status},"updating":self.pending.is_some()||self.candidate.is_some(),"updated":self.last_updated,"rules":self.rules.converted})
    }
    fn save_policy(&self) -> Result<()> {
        if let Some(_path) = &self.path {
            #[cfg(test)]
            crate::persistence::save_to(_path, "protection", &self.policy.value())?;
            #[cfg(not(test))]
            crate::persistence::save("protection", &self.policy.value())?;
        }
        Ok(())
    }
    pub fn set_enabled(&mut self, enabled: bool) -> Result<()> {
        self.policy.enabled = enabled;
        self.recompile();
        let result = self.save_policy();
        if let Err(error) = &result {
            self.status = format!(
                "Could not save protection preferences: {error}. This choice applies until Axolotl closes."
            );
        }
        result
    }
    pub fn toggle_site(&mut self, url: &str) -> Result<()> {
        let site = host(url)
            .ok_or_else(|| anyhow::anyhow!("This page has no website protection setting"))?;
        if !self.policy.exceptions.remove(&site) {
            self.policy.exceptions.insert(site);
        }
        self.recompile();
        let result = self.save_policy();
        if let Err(error) = &result {
            self.status = format!(
                "Could not save protection preferences: {error}. This choice applies until Axolotl closes."
            );
        }
        result
    }
    fn recompile(&mut self) {
        self.native = crate::protection_native::RuleStore::new(
            &self
                .candidate
                .as_ref()
                .map(|c| &c.rules)
                .unwrap_or(&self.rules)
                .network_json,
            &self.policy,
        );
        self.revision += 1;
        self.policy_revision += 1;
    }
    pub fn update(&mut self) {
        if self.pending.is_some() || self.candidate.is_some() {
            return;
        }
        self.last_attempt = Instant::now();
        self.status = "Checking EasyList + EasyPrivacy…".into();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(download());
        });
    }
    /// Commit only after download, parsing AND native compilation succeed.
    pub fn poll(&mut self) -> Result<bool> {
        if self.pending.is_none()
            && self.candidate.is_none()
            && now().saturating_sub(self.last_updated) >= DAY
            && self.last_attempt.elapsed().as_secs() >= DAY
            && !std::env::args().any(|a| a == "--smoke-test")
        {
            self.update();
        }
        if let Some(result) = self.pending.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.pending = None;
            match result {
                Ok(candidate) => {
                    self.native = crate::protection_native::RuleStore::new(
                        &candidate.rules.network_json,
                        &self.policy,
                    );
                    self.candidate = Some(candidate);
                }
                Err(error) => {
                    self.status = format!("Update failed; keeping working filters: {error}");
                }
            }
        }
        if let Some(error) = self.native.error() {
            if self.candidate.take().is_some() {
                self.native = crate::protection_native::RuleStore::new(
                    &self.rules.network_json,
                    &self.policy,
                );
            }
            self.status = format!("Native filter error: {error}");
            return Ok(false);
        }
        if self.candidate.is_some() && self.native.ready() {
            let candidate = self.candidate.take().unwrap();
            let value = json!({"easylist":candidate.lists[0],"easyprivacy":candidate.lists[1],"updated":candidate.checked});
            if let Some(path) = &self.path
                && let Err(error) = workspace_core::write_atomic(
                    &path.join("filter-lists.json"),
                    value.to_string().as_bytes(),
                )
            {
                self.native = crate::protection_native::RuleStore::new(
                    &self.rules.network_json,
                    &self.policy,
                );
                self.status =
                    format!("Update could not be saved; keeping working filters: {error}");
                return Ok(false);
            }
            self.rules = candidate.rules;
            self.last_updated = candidate.checked;
            self.revision += 1;
            self.status = "EasyList + EasyPrivacy are up to date".into();
            return Ok(true);
        }
        Ok(false)
    }
}
fn download() -> Result<Candidate> {
    use std::io::Read;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(45))
        .https_only(true)
        .build()?;
    let mut lists = [String::new(), String::new()];
    for (i, url) in SOURCES.iter().enumerate() {
        client
            .get(*url)
            .send()?
            .error_for_status()?
            .take((MAX_LIST + 1) as u64)
            .read_to_string(&mut lists[i])?;
        validate(&lists[i])?;
    }
    let rules = Arc::new(Rules::parse(&lists)?);
    Ok(Candidate {
        lists,
        rules,
        checked: now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_rules_block_ads_trackers_and_preserve_content() {
        let rules = Rules::parse(&[EASYLIST.into(), EASYPRIVACY.into()]).unwrap();
        let policy = Policy::default();
        assert!(rules.blocks(
            &policy,
            "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js",
            "https://example.org",
            "script"
        ));
        assert!(rules.blocks(
            &policy,
            "https://www.google-analytics.com/analytics.js",
            "https://example.org",
            "script"
        ));
        assert!(!rules.blocks(
            &policy,
            "https://example.org/article.js",
            "https://example.org",
            "script"
        ));
        let mut off = policy.clone();
        off.exceptions.insert("example.org".into());
        assert!(!rules.blocks(
            &off,
            "https://www.google-analytics.com/analytics.js",
            "https://example.org",
            "script"
        ));
        assert!(rules.blocks(
            &policy,
            "https://ads.example.net/?fp=1&poru=2",
            "https://example.org",
            "subdocument"
        ));
        assert!(rules.network_json.contains("ignore-previous-rules"));
    }
    #[test]
    fn invalid_update_and_policy_persistence() {
        assert!(validate("<html>error</html>").is_err());
        assert!(validate("[Adblock Plus 2.0]\n||one.test^").is_err());
        let dir = std::env::temp_dir().join(format!("axolotl-protection-{}", uuid::Uuid::new_v4()));
        let mut p = Protection::load(Some(dir.clone()));
        assert!(p.policy.enabled);
        p.toggle_site("https://EXAMPLE.org/a").unwrap();
        p.set_enabled(false).unwrap();
        let loaded = Protection::load(Some(dir.clone()));
        assert!(!loaded.policy.enabled);
        assert!(loaded.policy.exceptions.contains("example.org"));
        std::fs::write(dir.join("filter-lists.json"), "{broken").unwrap();
        let fallback = Protection::load(Some(dir.clone()));
        assert!(fallback.rules.converted > 1000);
        assert_eq!(fallback.last_updated, 0);
        assert!(!fallback.policy.allows("https://example.org"));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn failed_update_keeps_active_rules_and_cache() {
        let dir = std::env::temp_dir().join(format!("axolotl-update-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("filter-lists.json");
        let before =
            serde_json::json!({"easylist":EASYLIST,"easyprivacy":EASYPRIVACY,"updated":now()})
                .to_string();
        std::fs::write(&cache, &before).unwrap();
        let mut p = Protection::load(Some(dir.clone()));
        let rules = p.rules.clone();
        let timestamp = p.last_updated;
        let (tx, rx) = mpsc::channel();
        p.pending = Some(rx);
        tx.send(Err(anyhow::anyhow!("HTTP 503"))).unwrap();
        assert!(!p.poll().unwrap());
        assert!(Arc::ptr_eq(&rules, &p.rules));
        assert_eq!(timestamp, p.last_updated);
        assert_eq!(std::fs::read_to_string(cache).unwrap(), before);
        assert!(p.status.contains("keeping working filters"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
