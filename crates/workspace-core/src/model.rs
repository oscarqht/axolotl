use crate::{Axis, Layout, SplitEdge};

pub type SpaceId = u64;
pub type PaneId = u64;
pub type TabId = u64;
pub type SplitId = u64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub id: TabId,
    pub url: String,
    /// The page's own title. Always tracks the document, even while overridden.
    pub title: String,
    /// A user-chosen fixed title that supersedes `title` until cleared.
    pub custom_title: Option<String>,
}

impl Tab {
    /// The title shown in chrome: the custom title if set, else the page title.
    pub fn display_title(&self) -> &str {
        self.custom_title.as_deref().unwrap_or(&self.title)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    pub id: PaneId,
    pub tabs: Vec<Tab>,
    pub active_tab: TabId,
}

impl Pane {
    pub fn active_tab(&self) -> &Tab {
        self.tabs
            .iter()
            .find(|t| t.id == self.active_tab)
            .expect("pane has an active tab")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Space {
    pub id: SpaceId,
    pub name: String,
    pub custom_title: Option<String>,
    pub layout: Layout,
    pub panes: Vec<Pane>,
    pub focused_pane: PaneId,
}

impl Space {
    pub fn display_title(&self) -> &str {
        self.custom_title
            .as_deref()
            .unwrap_or_else(|| self.focused_pane().active_tab().display_title())
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == id)
    }
    pub fn focused_pane(&self) -> &Pane {
        self.pane(self.focused_pane)
            .expect("space has a focused pane")
    }
}

/// A window-wide tab. Its ID continues to own the same native page.
#[derive(Clone, Debug, PartialEq)]
pub struct PinnedTab {
    pub tab: Tab,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub spaces: Vec<Space>,
    pub pinned_tabs: Vec<PinnedTab>,
    pub active_space: SpaceId,
    pub(crate) next_id: u64,
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new("about:blank")
    }
}

impl Workspace {
    pub fn new(initial_url: &str) -> Self {
        let mut workspace = Self {
            spaces: Vec::new(),
            pinned_tabs: Vec::new(),
            active_space: 0,
            next_id: 1,
        };
        workspace.add_space("Home", initial_url);
        workspace
    }

    pub fn active_space(&self) -> &Space {
        self.spaces
            .iter()
            .find(|s| s.id == self.active_space)
            .expect("workspace has an active space")
    }

    pub fn active_space_mut(&mut self) -> &mut Space {
        self.spaces
            .iter_mut()
            .find(|s| s.id == self.active_space)
            .expect("workspace has an active space")
    }

    pub fn focused_pane(&self) -> &Pane {
        self.active_space().focused_pane()
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.spaces.iter().find_map(|s| s.pane(id))
    }

    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.spaces
            .iter()
            .flat_map(|s| &s.panes)
            .flat_map(|p| &p.tabs)
            .chain(self.pinned_tabs.iter().map(|p| &p.tab))
            .find(|t| t.id == id)
    }

    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.spaces
            .iter_mut()
            .flat_map(|s| &mut s.panes)
            .flat_map(|p| &mut p.tabs)
            .chain(self.pinned_tabs.iter_mut().map(|p| &mut p.tab))
            .find(|t| t.id == id)
    }

    /// Remove a tab from its pane without closing its page. Keep the final pane valid.
    pub fn pin_tab(&mut self, pane: PaneId, tab: TabId) -> bool {
        let Some(index) = self.active_space().panes.iter().position(|p| p.id == pane) else {
            return false;
        };
        let Some(slot) = self.active_space().panes[index]
            .tabs
            .iter()
            .position(|t| t.id == tab)
        else {
            return false;
        };
        let moved = self.active_space_mut().panes[index].tabs.remove(slot);
        if self.active_space().panes[index].tabs.is_empty() {
            if self.active_space().panes.len() > 1 {
                let space = self.active_space_mut();
                space.layout.remove_pane(pane);
                space.panes.remove(index);
                if space.focused_pane == pane {
                    space.focused_pane = space.panes[0].id;
                }
            } else {
                self.add_tab("about:blank");
            }
        } else if self.active_space().panes[index].active_tab == tab {
            let p = &mut self.active_space_mut().panes[index];
            p.active_tab = p.tabs[slot.min(p.tabs.len() - 1)].id;
        }
        self.pinned_tabs.push(PinnedTab {
            tab: moved,
            width: 480.,
            height: 640.,
        });
        true
    }

    pub fn unpin_tab(&mut self, tab: TabId) -> bool {
        let Some(index) = self.pinned_tabs.iter().position(|p| p.tab.id == tab) else {
            return false;
        };
        let moved = self.pinned_tabs.remove(index).tab;
        let pane = self.active_space().focused_pane;
        let p = self
            .active_space_mut()
            .panes
            .iter_mut()
            .find(|p| p.id == pane)
            .unwrap();
        p.active_tab = tab;
        p.tabs.push(moved);
        true
    }

    /// Insertion slot is measured before removal, matching the tab strip convention.
    pub fn move_pin(&mut self, tab: TabId, slot: usize) -> bool {
        let Some(from) = self.pinned_tabs.iter().position(|p| p.tab.id == tab) else {
            return false;
        };
        let to = slot.min(self.pinned_tabs.len());
        let to = to - usize::from(to > from);
        if from == to {
            return false;
        }
        let pin = self.pinned_tabs.remove(from);
        self.pinned_tabs.insert(to, pin);
        true
    }

    pub fn resize_pin(&mut self, tab: TabId, width: f32, height: f32) -> bool {
        if !width.is_finite() || !height.is_finite() {
            return false;
        }
        let Some(pin) = self.pinned_tabs.iter_mut().find(|p| p.tab.id == tab) else {
            return false;
        };
        pin.width = width.clamp(240., 4096.);
        pin.height = height.clamp(160., 4096.);
        true
    }

    /// Set (or, for blank input, clear) a fixed tab title. Returns false for unknown tabs.
    pub fn set_custom_title(&mut self, id: TabId, title: &str) -> bool {
        let title = title.trim();
        let Some(tab) = self.tab_mut(id) else {
            return false;
        };
        tab.custom_title = (!title.is_empty()).then(|| title.to_owned());
        true
    }

    /// Set or clear the space title without changing its selection or page titles.
    pub fn set_space_title(&mut self, id: SpaceId, title: &str) -> bool {
        let Some(space) = self.spaces.iter_mut().find(|space| space.id == id) else {
            return false;
        };
        let title = title.trim();
        space.custom_title = (!title.is_empty()).then(|| title.to_owned());
        true
    }

    pub fn add_space(&mut self, name: impl Into<String>, url: &str) -> SpaceId {
        let id = self.allocate();
        let pane = self.new_pane(url);
        let pane_id = pane.id;
        self.spaces.push(Space {
            id,
            name: name.into(),
            custom_title: None,
            layout: Layout::Pane(pane_id),
            panes: vec![pane],
            focused_pane: pane_id,
        });
        self.active_space = id;
        id
    }

    /// Move a space to an insertion slot in the original strip (0..=len).
    /// Selection and all space contents remain attached to their stable IDs.
    pub fn move_space(&mut self, id: SpaceId, slot: usize) -> bool {
        let Some(from) = self.spaces.iter().position(|space| space.id == id) else {
            return false;
        };
        let slot = slot.min(self.spaces.len());
        let to = if slot > from { slot - 1 } else { slot };
        if from == to {
            return false;
        }
        let space = self.spaces.remove(from);
        self.spaces.insert(to, space);
        true
    }

    pub fn switch_space(&mut self, id: SpaceId) -> bool {
        if !self.spaces.iter().any(|s| s.id == id) {
            return false;
        }
        self.active_space = id;
        true
    }

    /// The last space is retained so there is always a navigation target.
    pub fn close_space(&mut self, id: SpaceId) -> bool {
        if self.spaces.len() == 1 {
            return false;
        }
        let Some(index) = self.spaces.iter().position(|s| s.id == id) else {
            return false;
        };
        self.spaces.remove(index);
        if self.active_space == id {
            self.active_space = self.spaces[index.min(self.spaces.len() - 1)].id;
        }
        true
    }

    pub fn focus_pane(&mut self, id: PaneId) -> bool {
        let space = self.active_space_mut();
        if space.pane(id).is_none() {
            return false;
        }
        space.focused_pane = id;
        true
    }

    pub fn add_tab(&mut self, url: &str) -> TabId {
        let id = self.allocate();
        let pane_id = self.active_space().focused_pane;
        let pane = self
            .active_space_mut()
            .panes
            .iter_mut()
            .find(|p| p.id == pane_id)
            .unwrap();
        pane.tabs.push(Tab {
            id,
            url: url.to_owned(),
            title: String::new(),
            custom_title: None,
        });
        pane.active_tab = id;
        id
    }

    /// Open a foreground sibling immediately after its source in the active space.
    /// Ignore stale events from closed tabs or inactive spaces.
    pub fn add_tab_after(&mut self, source: TabId, url: &str) -> Option<TabId> {
        let (pane_index, tab_index) =
            self.active_space()
                .panes
                .iter()
                .enumerate()
                .find_map(|(index, pane)| {
                    pane.tabs
                        .iter()
                        .position(|tab| tab.id == source)
                        .map(|tab| (index, tab))
                })?;
        let id = self.allocate();
        let space = self.active_space_mut();
        let pane = &mut space.panes[pane_index];
        pane.tabs.insert(
            tab_index + 1,
            Tab {
                id,
                url: url.to_owned(),
                title: String::new(),
                custom_title: None,
            },
        );
        pane.active_tab = id;
        space.focused_pane = pane.id;
        Some(id)
    }

    pub fn switch_tab(&mut self, pane: PaneId, tab: TabId) -> bool {
        let space = self.active_space_mut();
        let Some(pane) = space.panes.iter_mut().find(|p| p.id == pane) else {
            return false;
        };
        if !pane.tabs.iter().any(|t| t.id == tab) {
            return false;
        }
        pane.active_tab = tab;
        space.focused_pane = pane.id;
        true
    }

    /// Move an existing tab between panes in the active space, retaining its identity.
    /// The insertion index is in the destination's original stack (0..=len).
    pub fn move_tab(
        &mut self,
        source: PaneId,
        tab: TabId,
        destination: PaneId,
        index: usize,
    ) -> bool {
        let space = self.active_space_mut();
        let Some(source_index) = space.panes.iter().position(|p| p.id == source) else {
            return false;
        };
        let Some(destination_index) = space.panes.iter().position(|p| p.id == destination) else {
            return false;
        };
        if index > space.panes[destination_index].tabs.len() {
            return false;
        }
        let Some(tab_index) = space.panes[source_index]
            .tabs
            .iter()
            .position(|t| t.id == tab)
        else {
            return false;
        };
        if source == destination {
            let pane = &mut space.panes[source_index];
            let moved = pane.tabs.remove(tab_index);
            pane.tabs
                .insert(index - usize::from(index > tab_index), moved);
            pane.active_tab = tab;
            space.focused_pane = source;
            return true;
        }
        let source_pane = &mut space.panes[source_index];
        let moved = source_pane.tabs.remove(tab_index);
        if source_pane.active_tab == tab && !source_pane.tabs.is_empty() {
            source_pane.active_tab = source_pane.tabs[tab_index.min(source_pane.tabs.len() - 1)].id;
        }
        space.panes[destination_index].tabs.insert(index, moved);
        space.panes[destination_index].active_tab = tab;
        space.focused_pane = destination;
        if space.panes[source_index].tabs.is_empty() {
            space.layout.remove_pane(source);
            space.panes.remove(source_index);
        }
        true
    }

    /// Move a complete stack without replacing tabs or native browser identities.
    /// A center drop merges; an edge drop repositions the existing pane leaf.
    pub fn move_pane(
        &mut self,
        source: PaneId,
        destination: PaneId,
        edge: Option<SplitEdge>,
    ) -> bool {
        if source == destination
            || self.active_space().pane(source).is_none()
            || self.active_space().pane(destination).is_none()
        {
            return false;
        }
        let split = edge.map(|_| self.allocate());
        let space = self.active_space_mut();
        space.layout.remove_pane(source);
        if let Some(edge) = edge {
            let (first, second) = if edge.is_first() {
                (source, destination)
            } else {
                (destination, source)
            };
            space.layout.replace_pane(
                destination,
                Layout::Split {
                    id: split.unwrap(),
                    axis: edge.axis(),
                    ratio: 0.5,
                    first: Box::new(Layout::Pane(first)),
                    second: Box::new(Layout::Pane(second)),
                },
            );
            space.focused_pane = source;
        } else {
            let index = space.panes.iter().position(|p| p.id == source).unwrap();
            let moved = space.panes.remove(index);
            let target = space
                .panes
                .iter_mut()
                .find(|p| p.id == destination)
                .unwrap();
            target.tabs.extend(moved.tabs);
            target.active_tab = moved.active_tab;
            space.focused_pane = destination;
        }
        true
    }

    pub fn cycle_tab(&mut self, pane: PaneId, forward: bool) -> bool {
        let Some(p) = self.active_space().pane(pane) else {
            return false;
        };
        let index = p.tabs.iter().position(|t| t.id == p.active_tab).unwrap();
        let next = (index + if forward { 1 } else { p.tabs.len() - 1 }) % p.tabs.len();
        self.switch_tab(pane, p.tabs[next].id)
    }

    /// Move the existing tab into a new half of the destination leaf atomically.
    pub fn split_tab(
        &mut self,
        source: PaneId,
        tab: TabId,
        destination: PaneId,
        edge: SplitEdge,
    ) -> Option<PaneId> {
        let space = self.active_space();
        let source_pane = space.pane(source)?;
        if !source_pane.tabs.iter().any(|t| t.id == tab)
            || (source == destination && source_pane.tabs.len() == 1)
            || space.pane(destination).is_none()
        {
            return None;
        }
        let id = self.allocate();
        let split = self.allocate();
        let space = self.active_space_mut();
        let (first, second) = if edge.is_first() {
            (id, destination)
        } else {
            (destination, id)
        };
        space.layout.replace_pane(
            destination,
            Layout::Split {
                id: split,
                axis: edge.axis(),
                ratio: 0.5,
                first: Box::new(Layout::Pane(first)),
                second: Box::new(Layout::Pane(second)),
            },
        );
        space.panes.push(Pane {
            id,
            tabs: Vec::new(),
            active_tab: tab,
        });
        assert!(self.move_tab(source, tab, id, 0));
        Some(id)
    }

    /// Empty panes collapse; the final pane receives a fresh blank tab.
    pub fn close_tab(&mut self, pane: PaneId, tab: TabId) -> bool {
        let Some(pane_ref) = self.active_space().pane(pane) else {
            return false;
        };
        let Some(index) = pane_ref.tabs.iter().position(|t| t.id == tab) else {
            return false;
        };
        if pane_ref.tabs.len() == 1 && self.active_space().panes.len() > 1 {
            return self.close_pane(pane);
        }
        let replacement = (pane_ref.tabs.len() == 1).then(|| self.allocate());
        let pane = self
            .active_space_mut()
            .panes
            .iter_mut()
            .find(|p| p.id == pane)
            .unwrap();
        pane.tabs.remove(index);
        if let Some(id) = replacement {
            pane.tabs.push(Tab {
                id,
                url: "about:blank".into(),
                title: String::new(),
                custom_title: None,
            });
        }
        if pane.active_tab == tab {
            pane.active_tab = pane.tabs[index.min(pane.tabs.len() - 1)].id;
        }
        true
    }

    pub fn split_focused(&mut self, axis: Axis, url: &str) -> PaneId {
        let split_id = self.allocate();
        let pane = self.new_pane(url);
        let id = pane.id;
        let space = self.active_space_mut();
        let old = space.focused_pane;
        space.layout.replace_pane(
            old,
            Layout::Split {
                id: split_id,
                axis,
                ratio: 0.5,
                first: Box::new(Layout::Pane(old)),
                second: Box::new(Layout::Pane(id)),
            },
        );
        space.panes.push(pane);
        space.focused_pane = id;
        id
    }

    /// Arrange four panes in a grid, keeping every tab (excess panes merge into the first).
    pub fn grid(&mut self, url: &str) {
        while self.active_space().panes.len() < 4 {
            let pane = self.new_pane(url);
            self.active_space_mut().panes.push(pane);
        }
        let root = self.allocate();
        let left = self.allocate();
        let right = self.allocate();
        let space = self.active_space_mut();
        if space.panes.len() > 4 {
            let extras = space.panes.split_off(4);
            for extra in extras {
                if extra.id == space.focused_pane {
                    space.focused_pane = space.panes[0].id;
                    space.panes[0].active_tab = extra.active_tab;
                }
                space.panes[0].tabs.extend(extra.tabs);
            }
        }
        let ids: Vec<_> = space.panes.iter().map(|p| p.id).collect();
        let column = |id, a, b| Layout::Split {
            id,
            axis: Axis::Vertical,
            ratio: 0.5,
            first: Box::new(Layout::Pane(a)),
            second: Box::new(Layout::Pane(b)),
        };
        space.layout = Layout::Split {
            id: root,
            axis: Axis::Horizontal,
            ratio: 0.5,
            first: Box::new(column(left, ids[0], ids[1])),
            second: Box::new(column(right, ids[2], ids[3])),
        };
    }

    /// A pane close closes its tabs; the final pane cannot be closed.
    pub fn close_pane(&mut self, id: PaneId) -> bool {
        let space = self.active_space_mut();
        if space.panes.len() == 1 {
            return false;
        }
        let Some(index) = space.panes.iter().position(|p| p.id == id) else {
            return false;
        };
        if !space.layout.remove_pane(id) {
            return false;
        }
        space.panes.remove(index);
        if space.focused_pane == id {
            space.focused_pane = space.layout.pane_ids()[0];
        }
        true
    }

    pub fn resize_split(&mut self, id: SplitId, ratio: f32) -> bool {
        self.active_space_mut().layout.resize(id, ratio)
    }

    fn allocate(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn new_pane(&mut self, url: &str) -> Pane {
        let id = self.allocate();
        let tab_id = self.allocate();
        Pane {
            id,
            active_tab: tab_id,
            tabs: vec![Tab {
                id: tab_id,
                url: url.to_owned(),
                title: String::new(),
                custom_title: None,
            }],
        }
    }
}
