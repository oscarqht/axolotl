//! Versioned, validated persistence of the whole workspace (all spaces).
//!
//! Wire types are separate from the model so the file format is explicit and
//! decoding can never produce a workspace that violates model invariants.
use crate::{Axis, Layout, Pane, PinnedTab, Space, Tab, Workspace};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fmt, fs, io,
    path::{Path, PathBuf},
};

pub const SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug)]
pub enum SnapshotError {
    Io(io::Error),
    Parse(serde_json::Error),
    UnsupportedVersion(u32),
    Invalid(&'static str),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "session file I/O failed: {e}"),
            Self::Parse(e) => write!(f, "session file is not valid: {e}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported session version {v}"),
            Self::Invalid(why) => write!(f, "session file is inconsistent: {why}"),
        }
    }
}

impl std::error::Error for SnapshotError {}

#[derive(Serialize, Deserialize)]
struct Header {
    version: u32,
}

#[derive(Serialize, Deserialize)]
struct WorkspaceDto {
    version: u32,
    next_id: u64,
    active_space: u64,
    spaces: Vec<SpaceDto>,
    #[serde(default)]
    pinned_tabs: Vec<PinnedDto>,
}

#[derive(Serialize, Deserialize)]
struct PinnedDto {
    tab: TabDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    width: f32,
    height: f32,
}

#[derive(Serialize, Deserialize)]
struct SpaceDto {
    id: u64,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    custom_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    focused_pane: u64,
    layout: LayoutDto,
    #[serde(default)]
    floating: Vec<crate::FloatingPanel>,
    panes: Vec<PaneDto>,
}

#[derive(Serialize, Deserialize)]
struct PaneDto {
    id: u64,
    active_tab: u64,
    tabs: Vec<TabDto>,
}

#[derive(Serialize, Deserialize)]
struct TabDto {
    id: u64,
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    custom_title: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AxisDto {
    Horizontal,
    Vertical,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LayoutDto {
    Pane {
        id: u64,
    },
    Split {
        id: u64,
        axis: AxisDto,
        ratio: f32,
        first: Box<LayoutDto>,
        second: Box<LayoutDto>,
    },
}

impl From<&Layout> for LayoutDto {
    fn from(layout: &Layout) -> Self {
        match layout {
            Layout::Pane(id) => Self::Pane { id: *id },
            Layout::Split {
                id,
                axis,
                ratio,
                first,
                second,
            } => Self::Split {
                id: *id,
                axis: match axis {
                    Axis::Horizontal => AxisDto::Horizontal,
                    Axis::Vertical => AxisDto::Vertical,
                },
                ratio: *ratio,
                first: Box::new(first.as_ref().into()),
                second: Box::new(second.as_ref().into()),
            },
        }
    }
}

impl LayoutDto {
    fn into_layout(self, ids: &mut HashSet<u64>, depth: usize) -> Result<Layout, SnapshotError> {
        if depth > 256 {
            return Err(SnapshotError::Invalid("layout nesting is too deep"));
        }
        Ok(match self {
            Self::Pane { id } => Layout::Pane(id),
            Self::Split {
                id,
                axis,
                ratio,
                first,
                second,
            } => {
                if !ids.insert(id) {
                    return Err(SnapshotError::Invalid("duplicate id"));
                }
                if !ratio.is_finite() || !(0. ..=1.).contains(&ratio) {
                    return Err(SnapshotError::Invalid("split ratio out of range"));
                }
                Layout::Split {
                    id,
                    axis: match axis {
                        AxisDto::Horizontal => Axis::Horizontal,
                        AxisDto::Vertical => Axis::Vertical,
                    },
                    ratio,
                    first: Box::new(first.into_layout(ids, depth + 1)?),
                    second: Box::new(second.into_layout(ids, depth + 1)?),
                }
            }
        })
    }
}

impl Workspace {
    /// Serialize every space, its layout tree, tab stacks and focus state.
    pub fn to_json(&self) -> String {
        let dto = WorkspaceDto {
            version: SNAPSHOT_VERSION,
            next_id: self.next_id,
            active_space: self.active_space,
            pinned_tabs: self
                .pinned_tabs
                .iter()
                .map(|p| PinnedDto {
                    tab: TabDto {
                        id: p.tab.id,
                        url: p.tab.url.clone(),
                        title: p.tab.title.clone(),
                        custom_title: p.tab.custom_title.clone(),
                    },
                    icon: p.icon.clone(),
                    width: p.width,
                    height: p.height,
                })
                .collect(),
            spaces: self
                .spaces
                .iter()
                .map(|s| SpaceDto {
                    id: s.id,
                    name: s.name.clone(),
                    custom_title: s.custom_title.clone(),
                    icon: s.icon.clone(),
                    focused_pane: s.focused_pane,
                    layout: (&s.layout).into(),
                    floating: s.floating.clone(),
                    panes: s
                        .panes
                        .iter()
                        .map(|p| PaneDto {
                            id: p.id,
                            active_tab: p.active_tab,
                            tabs: p
                                .tabs
                                .iter()
                                .map(|t| TabDto {
                                    id: t.id,
                                    url: t.url.clone(),
                                    title: t.title.clone(),
                                    custom_title: t.custom_title.clone(),
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
        };
        serde_json::to_string_pretty(&dto).expect("workspace snapshot serializes")
    }

    /// Decode and validate a snapshot. Never returns a workspace that breaks
    /// the invariants the rest of the model relies on (and panics on).
    pub fn from_json(text: &str) -> Result<Self, SnapshotError> {
        let header: Header = serde_json::from_str(text).map_err(SnapshotError::Parse)?;
        if header.version != SNAPSHOT_VERSION {
            return Err(SnapshotError::UnsupportedVersion(header.version));
        }
        let dto: WorkspaceDto = serde_json::from_str(text).map_err(SnapshotError::Parse)?;
        if dto.spaces.is_empty() {
            return Err(SnapshotError::Invalid("no spaces"));
        }
        let mut ids = HashSet::new();
        let mut spaces = Vec::new();
        for s in dto.spaces {
            if !ids.insert(s.id) {
                return Err(SnapshotError::Invalid("duplicate id"));
            }
            if s.panes.is_empty() {
                return Err(SnapshotError::Invalid("space without panes"));
            }
            let mut panes = Vec::new();
            for p in s.panes {
                if !ids.insert(p.id) {
                    return Err(SnapshotError::Invalid("duplicate id"));
                }
                if p.tabs.is_empty() {
                    return Err(SnapshotError::Invalid("pane without tabs"));
                }
                let mut tabs = Vec::new();
                for t in p.tabs {
                    if !ids.insert(t.id) {
                        return Err(SnapshotError::Invalid("duplicate id"));
                    }
                    tabs.push(Tab {
                        id: t.id,
                        url: t.url,
                        title: t.title,
                        custom_title: t.custom_title.filter(|c| !c.trim().is_empty()),
                    });
                }
                if !tabs.iter().any(|t| t.id == p.active_tab) {
                    return Err(SnapshotError::Invalid("active tab not in pane"));
                }
                panes.push(Pane {
                    id: p.id,
                    tabs,
                    active_tab: p.active_tab,
                });
            }
            let layout = s.layout.into_layout(&mut ids, 0)?;
            let mut layout_panes = layout.pane_ids();
            for f in &s.floating {
                if !crate::floating::valid_bounds(f.bounds) {
                    return Err(SnapshotError::Invalid("invalid floating bounds"));
                }
                layout_panes.push(f.pane);
            }
            let mut pane_ids: Vec<_> = panes.iter().map(|p| p.id).collect();
            layout_panes.sort_unstable();
            pane_ids.sort_unstable();
            if layout_panes != pane_ids {
                return Err(SnapshotError::Invalid("layout does not match panes"));
            }
            if !pane_ids.contains(&s.focused_pane) {
                return Err(SnapshotError::Invalid("focused pane not in space"));
            }
            spaces.push(Space {
                id: s.id,
                name: s.name,
                custom_title: s.custom_title.filter(|c| !c.trim().is_empty()),
                icon: s
                    .icon
                    .map(|icon| icon.trim().to_owned())
                    .filter(|icon| !icon.is_empty()),
                layout,
                floating: s.floating,
                panes,
                focused_pane: s.focused_pane,
            });
        }
        if !spaces.iter().any(|s| s.id == dto.active_space) {
            return Err(SnapshotError::Invalid("active space missing"));
        }
        let mut pinned_tabs = Vec::new();
        for p in dto.pinned_tabs {
            if !ids.insert(p.tab.id) {
                return Err(SnapshotError::Invalid("duplicate id"));
            }
            if !p.width.is_finite()
                || !p.height.is_finite()
                || !(240. ..=4096.).contains(&p.width)
                || !(160. ..=4096.).contains(&p.height)
            {
                return Err(SnapshotError::Invalid("invalid pinned popup size"));
            }
            pinned_tabs.push(PinnedTab {
                tab: Tab {
                    id: p.tab.id,
                    url: p.tab.url,
                    title: p.tab.title,
                    custom_title: p.tab.custom_title.filter(|c| !c.trim().is_empty()),
                },
                icon: p
                    .icon
                    .filter(|s| !s.trim().is_empty() && s.chars().count() <= 32),
                width: p.width,
                height: p.height,
            });
        }
        // Never reissue an id that is already in use, even if next_id was tampered with.
        let next_id = dto.next_id.max(ids.iter().max().map_or(1, |m| m + 1));
        Ok(Self {
            spaces,
            pinned_tabs,
            active_space: dto.active_space,
            next_id,
        })
    }
}

/// Write via a sibling temp file and rename so a crash never leaves a partial file.
pub fn save_workspace(path: &Path, workspace: &Workspace) -> Result<(), SnapshotError> {
    write_atomic(path, workspace.to_json().as_bytes()).map_err(SnapshotError::Io)
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use io::Write;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp: PathBuf = path.as_os_str().to_owned().into();
    tmp.as_mut_os_string().push(".tmp");
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// `Ok(None)` when no session has been saved yet.
pub fn load_workspace(path: &Path) -> Result<Option<Workspace>, SnapshotError> {
    match fs::read_to_string(path) {
        Ok(text) => Workspace::from_json(&text).map(Some),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(SnapshotError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Workspace {
        let mut w = Workspace::new("https://example.com/");
        let right = w.split_focused(Axis::Horizontal, "https://rust-lang.org/");
        w.split_focused(Axis::Vertical, "https://gpui.rs/");
        w.focus_pane(right);
        let tab = w.add_tab("https://docs.rs/");
        w.tab_mut(tab).unwrap().title = "Docs — rs".into();
        w.set_custom_title(tab, "My docs");
        let split = match &w.active_space().layout {
            Layout::Split { id, .. } => *id,
            _ => unreachable!(),
        };
        w.resize_split(split, 0.3);
        let second = w.add_space("Work", "https://news.ycombinator.com/");
        w.split_focused(Axis::Vertical, "about:blank");
        w.switch_space(second);
        w
    }

    #[test]
    fn round_trip_preserves_everything_and_id_allocation() {
        let w = sample();
        let mut back = Workspace::from_json(&w.to_json()).unwrap();
        assert_eq!(back.spaces, w.spaces);
        assert_eq!(back.active_space, w.active_space);
        assert_eq!(back.to_json(), w.to_json());
        // New ids continue past the restored ones.
        let before: HashSet<_> = back
            .spaces
            .iter()
            .flat_map(|s| s.panes.iter().flat_map(|p| p.tabs.iter().map(|t| t.id)))
            .collect();
        assert!(!before.contains(&back.add_tab("about:blank")));
    }

    #[test]
    fn rejects_garbage_and_truncated_input() {
        assert!(Workspace::from_json("").is_err());
        assert!(Workspace::from_json("not json").is_err());
        let json = sample().to_json();
        assert!(Workspace::from_json(&json[..json.len() / 2]).is_err());
    }

    #[test]
    fn rejects_unknown_version() {
        let json = sample()
            .to_json()
            .replacen("\"version\": 1", "\"version\": 99", 1);
        assert!(matches!(
            Workspace::from_json(&json),
            Err(SnapshotError::UnsupportedVersion(99))
        ));
    }

    fn mutated(f: impl FnOnce(&mut serde_json::Value)) -> Result<Workspace, SnapshotError> {
        let mut v: serde_json::Value = serde_json::from_str(&sample().to_json()).unwrap();
        f(&mut v);
        Workspace::from_json(&v.to_string())
    }

    #[test]
    fn rejects_inconsistent_state() {
        use serde_json::json;
        assert!(mutated(|v| v["spaces"] = json!([])).is_err());
        assert!(mutated(|v| v["active_space"] = json!(9999)).is_err());
        assert!(mutated(|v| v["spaces"][0]["focused_pane"] = json!(9999)).is_err());
        assert!(mutated(|v| v["spaces"][0]["panes"][0]["active_tab"] = json!(9999)).is_err());
        assert!(mutated(|v| v["spaces"][0]["panes"][0]["tabs"] = json!([])).is_err());
        assert!(mutated(|v| v["spaces"][0]["panes"] = json!([])).is_err());
        // Layout references a pane that does not exist.
        assert!(mutated(|v| v["spaces"][0]["panes"].as_array_mut().unwrap().truncate(1)).is_err());
        // Duplicate ids across spaces.
        assert!(mutated(|v| v["spaces"][1]["id"] = v["spaces"][0]["id"].clone()).is_err());
        assert!(mutated(|v| v["spaces"][0]["layout"]["ratio"] = json!(7.0)).is_err());
    }

    #[test]
    fn tampered_next_id_cannot_reissue_ids() {
        let w = mutated(|v| v["next_id"] = serde_json::json!(1)).unwrap();
        assert!(w.next_id > 1);
    }

    #[test]
    fn custom_title_round_trips_and_is_optional() {
        let w = sample();
        let back = Workspace::from_json(&w.to_json()).unwrap();
        let titled = back
            .spaces
            .iter()
            .flat_map(|s| &s.panes)
            .flat_map(|p| &p.tabs)
            .filter(|t| t.custom_title.as_deref() == Some("My docs"))
            .count();
        assert_eq!(titled, 1);
        // Files written before custom titles existed omit the field entirely.
        assert!(!Workspace::default().to_json().contains("custom_title"));
    }

    #[test]
    fn missing_title_defaults_to_empty() {
        let w = mutated(|v| {
            v["spaces"][0]["panes"][0]["tabs"][0]
                .as_object_mut()
                .unwrap()
                .remove("title");
        })
        .unwrap();
        assert_eq!(w.spaces[0].panes[0].tabs[0].title, "");
    }

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("axolotl-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("nested").join("session.json")
    }

    #[test]
    fn save_and_load_round_trip_on_disk() {
        let path = temp_path("roundtrip");
        assert!(load_workspace(&path).unwrap().is_none());
        let w = sample();
        save_workspace(&path, &w).unwrap();
        save_workspace(&path, &w).unwrap(); // overwrite
        assert!(!path.with_extension("json.tmp").exists());
        let back = load_workspace(&path).unwrap().unwrap();
        assert_eq!(back.spaces, w.spaces);
        fs::write(&path, "{ corrupt").unwrap();
        assert!(load_workspace(&path).is_err());
        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }
}
