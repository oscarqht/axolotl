//! Durable application state lives in app.sqlite3; models stay in memory.
pub use crate::persistence_worker::error_status;
use crate::persistence_worker::{Value, store};
use serde::{Serialize, de::DeserializeOwned};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    cell::Cell,
    ops::{Deref, DerefMut},
    path::PathBuf,
    time::Instant,
};
use workspace_core::Workspace;

static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

/// `AXOLOTL_DATA_DIR` overrides the OS app-data directory (used for testing).
pub(crate) fn data_dir() -> Option<PathBuf> {
    if std::env::args().any(|arg| arg == "--smoke-test")
        || (cfg!(test) && std::env::var_os("AXOLOTL_DATA_DIR").is_none())
    {
        return None;
    }
    let dir = if let Some(dir) = std::env::var_os("AXOLOTL_DATA_DIR") {
        PathBuf::from(dir)
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/Axolotl")
    } else if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var_os("APPDATA")?).join("Axolotl")
    } else if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("axolotl")
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".local/share/axolotl")
    };
    Some(dir)
}
#[cfg(not(test))]
pub fn load<T: DeserializeOwned>(key: &str) -> anyhow::Result<Option<T>> {
    store()?
        .load(key)
        .map(|s| serde_json::from_str(&s).map_err(Into::into))
        .transpose()
}
#[cfg(not(test))]
pub fn save<T: Serialize>(key: &str, value: &T) -> anyhow::Result<()> {
    store()?.save(vec![(
        key.to_owned(),
        Value::Json(serde_json::to_string(value)?),
    )])
}
#[cfg(test)]
pub fn load_from<T: DeserializeOwned>(
    dir: &std::path::Path,
    key: &str,
) -> anyhow::Result<Option<T>> {
    crate::app_database::AppDatabase::open(dir)?
        .load(key)?
        .map(|s| serde_json::from_str(&s).map_err(Into::into))
        .transpose()
}
#[cfg(test)]
pub fn save_to<T: Serialize>(dir: &std::path::Path, key: &str, value: &T) -> anyhow::Result<()> {
    crate::app_database::AppDatabase::open(dir)?
        .write_batch(&[(key.into(), serde_json::to_string(value)?)])
}
pub fn flush() -> anyhow::Result<()> {
    store()?.flush()
}

/// All mutable access (including direct field edits) advances the revision.
/// Immutable access and idle UI polling are constant time. Native views and
/// interaction state live outside this wrapper and never dirty the snapshot.
#[derive(Clone, Debug)]
pub struct TrackedWorkspace {
    workspace: Workspace,
    revision: u64,
    dirty_since: Cell<Option<Instant>>,
}
impl From<Workspace> for TrackedWorkspace {
    fn from(workspace: Workspace) -> Self {
        Self {
            workspace,
            revision: NEXT_REVISION.fetch_add(1, Ordering::Relaxed),
            dirty_since: Cell::new(None),
        }
    }
}
impl Deref for TrackedWorkspace {
    type Target = Workspace;
    fn deref(&self) -> &Workspace {
        &self.workspace
    }
}
impl DerefMut for TrackedWorkspace {
    fn deref_mut(&mut self) -> &mut Workspace {
        self.revision = NEXT_REVISION.fetch_add(1, Ordering::Relaxed);
        if self.dirty_since.get().is_none() {
            self.dirty_since.set(Some(Instant::now()));
        }
        &mut self.workspace
    }
}
pub fn restore(homepage_url: &str) -> TrackedWorkspace {
    let result = (|| -> anyhow::Result<_> {
        Ok(store()?
            .load("session")
            .map(|json| Workspace::from_json(&json))
            .transpose()?)
    })();
    match result {
        Ok(workspace) => workspace
            .unwrap_or_else(|| Workspace::new(homepage_url))
            .into(),
        Err(error) => {
            crate::persistence_worker::report(format!("{error:#}"));
            Workspace::new(homepage_url).into()
        }
    }
}

pub struct SessionSaver {
    revision: Option<u64>,
}
impl SessionSaver {
    pub fn new(initial: &TrackedWorkspace) -> Self {
        Self {
            revision: Some(initial.revision),
        }
    }
    fn snapshot(&mut self, workspace: &TrackedWorkspace) -> Option<(Value, Instant)> {
        if self.revision == Some(workspace.revision) {
            return None;
        }
        self.revision = Some(workspace.revision);
        Some((
            Value::Workspace(workspace.workspace.clone()),
            workspace.dirty_since.take().unwrap_or_else(Instant::now),
        ))
    }
    /// Queue only changed revisions. Serialization and SQLite run on the writer.
    pub fn tick<T: Serialize>(&mut self, workspace: &TrackedWorkspace, sleep: &T, _: Instant) {
        if store().is_err() {
            return;
        }
        if let Some((value, since)) = self.snapshot(workspace) {
            self.submit_snapshot(workspace, value, sleep, since);
        }
    }
    fn submit(&mut self, entries: Vec<(String, Value)>, since: Instant) {
        if let Err(error) = store().and_then(|s| s.save_since(entries, since)) {
            crate::persistence_worker::report(format!("{error:#}"));
        }
    }
    fn submit_snapshot<T: Serialize>(
        &mut self,
        workspace: &TrackedWorkspace,
        snapshot: Value,
        sleep: &T,
        since: Instant,
    ) {
        match sleep_json(workspace, sleep) {
            Ok(json) => self.submit(
                vec![
                    ("session".into(), snapshot),
                    ("sleep".into(), Value::Json(json)),
                ],
                since,
            ),
            Err(error) => crate::persistence_worker::report(error),
        }
    }
    /// Every workspace revision pairs with metadata for exactly its tab IDs,
    /// including when a close event reaches the poll before render cleanup.
    pub fn checkpoint<T: Serialize>(&mut self, workspace: &TrackedWorkspace, sleep: &T) {
        self.revision = Some(workspace.revision);
        self.submit_snapshot(
            workspace,
            Value::Workspace(workspace.workspace.clone()),
            sleep,
            workspace.dirty_since.take().unwrap_or_else(Instant::now),
        );
    }
}
fn sleep_json<T: Serialize>(workspace: &Workspace, sleep: &T) -> anyhow::Result<String> {
    let mut value = serde_json::to_value(sleep)?;
    let entries = value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("sleep metadata must be an object"))?;
    entries.retain(|id, _| {
        id.parse()
            .ok()
            .is_some_and(|id| workspace.tab(id).is_some())
    });
    Ok(serde_json::to_string(&value)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_polls_do_not_capture_or_serialize_workspace() {
        let mut workspace: TrackedWorkspace = Workspace::default().into();
        let mut saver = SessionSaver::new(&workspace);
        assert!(saver.snapshot(&workspace).is_none());
        for _ in 0..10000 {
            assert!(saver.snapshot(&workspace).is_none());
        }
        workspace.add_space("Two", "https://example.com");
        assert!(saver.snapshot(&workspace).is_some());
        workspace.spaces[0].name = "Direct edit".into();
        assert!(saver.snapshot(&workspace).is_some());
        let tab = workspace.focused_pane().active_tab;
        workspace.tab_mut(tab).unwrap().title = "Title".into();
        assert!(saver.snapshot(&workspace).is_some());
    }
    #[test]
    fn workspace_snapshot_drops_metadata_for_tabs_closed_before_render_cleanup() {
        let mut workspace = Workspace::default();
        let closed = workspace.add_tab("https://closed.test");
        let keep = workspace.focused_pane().tabs[0].id;
        let sleep = serde_json::json!({closed.to_string(): {"sleeping":true}, keep.to_string(): {"keep_alive":true}});
        workspace.close_tab(workspace.focused_pane().id, closed);
        let snapshot: serde_json::Value =
            serde_json::from_str(&sleep_json(&workspace, &sleep).unwrap()).unwrap();
        assert!(snapshot.get(closed.to_string()).is_none());
        assert_eq!(snapshot[keep.to_string()]["keep_alive"], true);
    }
    #[test]
    #[ignore = "manual idle persistence benchmark"]
    fn measure_idle_snapshot_overhead() {
        let mut workspace: TrackedWorkspace = Workspace::default().into();
        for _ in 0..250 {
            workspace.add_tab("https://example.com/a-longer-url");
        }
        let mut saver = SessionSaver::new(&workspace);
        saver.snapshot(&workspace);
        let start = Instant::now();
        for _ in 0..10000 {
            std::hint::black_box(workspace.to_json());
        }
        let before = start.elapsed();
        let start = Instant::now();
        for _ in 0..10000 {
            std::hint::black_box(saver.snapshot(&workspace));
        }
        eprintln!(
            "{}-tab idle polls, 10000 iterations: serialize {:?}; revision {:?}",
            workspace.focused_pane().tabs.len(),
            before,
            start.elapsed()
        );
    }
}
