use crate::{Axis, Layout};

pub type SpaceId = u64;
pub type PaneId = u64;
pub type TabId = u64;
pub type SplitId = u64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub id: TabId,
    pub url: String,
    pub title: String,
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
    pub layout: Layout,
    pub panes: Vec<Pane>,
    pub focused_pane: PaneId,
}

impl Space {
    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == id)
    }
    pub fn focused_pane(&self) -> &Pane {
        self.pane(self.focused_pane)
            .expect("space has a focused pane")
    }
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub spaces: Vec<Space>,
    pub active_space: SpaceId,
    next_id: u64,
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
            .find(|t| t.id == id)
    }

    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.spaces
            .iter_mut()
            .flat_map(|s| &mut s.panes)
            .flat_map(|p| &mut p.tabs)
            .find(|t| t.id == id)
    }

    pub fn add_space(&mut self, name: impl Into<String>, url: &str) -> SpaceId {
        let id = self.allocate();
        let pane = self.new_pane(url);
        let pane_id = pane.id;
        self.spaces.push(Space {
            id,
            name: name.into(),
            layout: Layout::Pane(pane_id),
            panes: vec![pane],
            focused_pane: pane_id,
        });
        self.active_space = id;
        id
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
        });
        pane.active_tab = id;
        id
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
        if source == destination {
            return false;
        }
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

    /// Closing the last tab replaces it with a fresh blank tab.
    pub fn close_tab(&mut self, pane: PaneId, tab: TabId) -> bool {
        let Some(pane_ref) = self.active_space().pane(pane) else {
            return false;
        };
        let Some(index) = pane_ref.tabs.iter().position(|t| t.id == tab) else {
            return false;
        };
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
            }],
        }
    }
}
