//! Session persistence glue: where the file lives and when to write it.
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use workspace_core::{Workspace, load_workspace, write_atomic};

const DEBOUNCE: Duration = Duration::from_millis(500);

/// `AXOLOTL_DATA_DIR` overrides the OS app-data directory (used for testing).
pub fn session_path() -> Option<PathBuf> {
    // Smoke runs must neither read nor clobber the user's real session.
    if std::env::args().any(|arg| arg == "--smoke-test") {
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
    Some(dir.join("session.json"))
}

/// Restore the saved workspace, or the default start state when the file is
/// absent, corrupt or from an unknown version. A bad file is kept aside.
pub fn restore() -> Workspace {
    let Some(path) = session_path() else {
        return Workspace::new("about:blank");
    };
    match load_workspace(&path) {
        Ok(Some(workspace)) => workspace,
        Ok(None) => Workspace::new("about:blank"),
        Err(error) => {
            eprintln!("Axolotl: ignoring saved session: {error}");
            let _ = std::fs::rename(&path, path.with_extension("json.bad"));
            Workspace::new("about:blank")
        }
    }
}

/// Writes a snapshot once the model has been stable for the debounce period.
pub struct SessionSaver {
    path: Option<PathBuf>,
    written: Option<String>,
    pending: Option<(String, Instant)>,
}

impl SessionSaver {
    pub fn new(initial: &Workspace) -> Self {
        Self {
            path: session_path(),
            written: Some(initial.to_json()),
            pending: None,
        }
    }

    /// Called from the UI poll loop.
    pub fn tick(&mut self, workspace: &Workspace, now: Instant) {
        let json = workspace.to_json();
        if self.written.as_deref() == Some(json.as_str()) {
            self.pending = None;
            return;
        }
        match &self.pending {
            Some((pending, since)) if *pending == json => {
                if now.duration_since(*since) >= DEBOUNCE {
                    self.write(json);
                }
            }
            _ => self.pending = Some((json, now)),
        }
    }

    /// Unconditional save for shutdown.
    pub fn flush(&mut self, workspace: &Workspace) {
        let json = workspace.to_json();
        if self.written.as_deref() != Some(json.as_str()) {
            self.write(json);
        }
    }

    fn write(&mut self, json: String) {
        self.pending = None;
        let Some(path) = &self.path else { return };
        match write_atomic(path, json.as_bytes()) {
            Ok(()) => self.written = Some(json),
            Err(error) => eprintln!("Axolotl: failed to save session: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saver(name: &str) -> (SessionSaver, PathBuf) {
        let path = std::env::temp_dir()
            .join(format!("axolotl-saver-{}-{name}", std::process::id()))
            .join("session.json");
        let workspace = Workspace::new("about:blank");
        let mut saver = SessionSaver::new(&workspace);
        saver.path = Some(path.clone());
        (saver, path)
    }

    #[test]
    fn writes_only_after_model_is_stable_and_flush_is_immediate() {
        let (mut saver, path) = saver("debounce");
        let mut workspace = Workspace::new("about:blank");
        let t0 = Instant::now();
        saver.tick(&workspace, t0);
        assert!(!path.exists(), "unchanged model is not written");
        workspace.add_space("Two", "https://example.com/");
        saver.tick(&workspace, t0);
        saver.tick(&workspace, t0 + Duration::from_millis(100));
        assert!(!path.exists(), "debounced");
        workspace.add_tab("https://rust-lang.org/");
        saver.tick(&workspace, t0 + Duration::from_millis(400));
        saver.tick(&workspace, t0 + Duration::from_millis(800));
        assert!(!path.exists(), "a new change restarts the debounce");
        saver.tick(&workspace, t0 + Duration::from_millis(1000));
        assert!(path.exists());
        let saved = load_workspace(&path).unwrap().unwrap();
        assert_eq!(saved.spaces, workspace.spaces);
        workspace.add_tab("https://gpui.rs/");
        saver.flush(&workspace);
        assert_eq!(load_workspace(&path).unwrap().unwrap().spaces, workspace.spaces);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
