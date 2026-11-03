//! Browser workspace state, independent of a rendering toolkit or native web engine.
mod layout;
mod model;
mod navigation;

pub use layout::{Axis, Divider, Geometry, Layout, PaneRect, Rect};
pub use model::{Pane, PaneId, Space, SpaceId, SplitId, Tab, TabId, Workspace};
pub use navigation::{normalize_url, NavigationError};

#[cfg(test)]
mod tests;
