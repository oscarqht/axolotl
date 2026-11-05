//! Browser workspace state, independent of a rendering toolkit or native web engine.
mod layout;
mod model;
mod navigation;
mod snapshot;

pub use layout::{Axis, Divider, Geometry, Layout, PaneRect, Rect, SplitEdge};
pub use model::{Pane, PaneId, Space, SpaceId, SplitId, Tab, TabId, Workspace};
pub use navigation::{normalize_url, NavigationError};
pub use snapshot::{load_workspace, save_workspace, write_atomic, SnapshotError, SNAPSHOT_VERSION};

#[cfg(test)]
mod tests;
