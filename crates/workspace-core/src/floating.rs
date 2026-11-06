//! Space-owned overlays retain the same panes and tab IDs as the tiled layout.
use crate::{Pane, PaneId, Rect, Space, SplitEdge, TabId, Workspace};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FloatingPanel {
    pub pane: PaneId,
    pub bounds: Rect,
    pub edge: Option<SplitEdge>,
    pub pinned: bool,
}

impl FloatingPanel {
    pub fn fitted(&self, area: Rect) -> Rect {
        let w = self.bounds.width.clamp(240., 4096.).min(area.width.max(1.));
        let h = self
            .bounds
            .height
            .clamp(160., 4096.)
            .min(area.height.max(1.));
        let mut x = self
            .bounds
            .x
            .clamp(area.x, area.x + (area.width - w).max(0.));
        let mut y = self
            .bounds
            .y
            .clamp(area.y, area.y + (area.height - h).max(0.));
        match self.edge {
            Some(SplitEdge::Left) => x = area.x,
            Some(SplitEdge::Right) => x = area.x + area.width - w,
            Some(SplitEdge::Top) => y = area.y,
            Some(SplitEdge::Bottom) => y = area.y + area.height - h,
            None => (),
        }
        Rect::new(x, y, w, h)
    }
}

impl Space {
    pub fn floating_panel(&self, pane: PaneId) -> Option<&FloatingPanel> {
        self.floating.iter().find(|f| f.pane == pane)
    }
}

impl Workspace {
    // Removing the final tiled leaf leaves a fresh start page behind.
    pub(crate) fn repair_tiled(&mut self) {
        let ids = self.active_space().layout.pane_ids();
        if ids.iter().any(|id| self.active_space().pane(*id).is_some()) {
            return;
        }
        let pane = self.new_pane("about:blank");
        let space = self.active_space_mut();
        space.layout = crate::Layout::Pane(pane.id);
        if space.pane(space.focused_pane).is_none() {
            space.focused_pane = pane.id;
        }
        space.panes.push(pane);
    }

    pub fn float_pane(&mut self, pane: PaneId, bounds: Rect, edge: Option<SplitEdge>) -> bool {
        if !valid_bounds(bounds) || self.active_space().pane(pane).is_none() {
            return false;
        }
        if self.active_space().floating_panel(pane).is_none() {
            if self.active_space().layout.pane_ids().len() == 1 {
                let replacement = self.new_pane("about:blank");
                self.active_space_mut().layout = crate::Layout::Pane(replacement.id);
                self.active_space_mut().panes.push(replacement);
            } else {
                self.active_space_mut().layout.remove_pane(pane);
            }
            self.active_space_mut().floating.push(FloatingPanel {
                pane,
                bounds,
                edge,
                pinned: false,
            });
        } else {
            let f = self
                .active_space_mut()
                .floating
                .iter_mut()
                .find(|f| f.pane == pane)
                .unwrap();
            f.bounds = bounds;
            f.edge = edge;
        }
        self.active_space_mut().focused_pane = pane;
        true
    }

    pub fn float_tab(
        &mut self,
        source: PaneId,
        tab: TabId,
        bounds: Rect,
        edge: Option<SplitEdge>,
    ) -> Option<PaneId> {
        let p = self.active_space().pane(source)?;
        if !valid_bounds(bounds) || !p.tabs.iter().any(|t| t.id == tab) {
            return None;
        }
        if p.tabs.len() == 1 {
            return self.float_pane(source, bounds, edge).then_some(source);
        }
        let id = self.allocate();
        self.active_space_mut().panes.push(Pane {
            id,
            tabs: vec![],
            active_tab: tab,
        });
        self.active_space_mut().floating.push(FloatingPanel {
            pane: id,
            bounds,
            edge,
            pinned: false,
        });
        self.move_tab(source, tab, id, 0).then_some(id)
    }

    pub fn resize_floating(&mut self, pane: PaneId, bounds: Rect) -> bool {
        if !valid_bounds(bounds) {
            return false;
        }
        let Some(f) = self
            .active_space_mut()
            .floating
            .iter_mut()
            .find(|f| f.pane == pane)
        else {
            return false;
        };
        f.bounds = Rect::new(
            bounds.x,
            bounds.y,
            bounds.width.clamp(240., 4096.),
            bounds.height.clamp(160., 4096.),
        );
        true
    }

    pub fn pin_floating(&mut self, pane: PaneId) -> bool {
        let Some(f) = self
            .active_space_mut()
            .floating
            .iter_mut()
            .find(|f| f.pane == pane)
        else {
            return false;
        };
        f.pinned = !f.pinned;
        true
    }

    /// Reorder edge chips using the same before-removal insertion convention as tabs.
    pub fn reorder_floating(&mut self, pane: PaneId, slot: usize) -> bool {
        let items = &mut self.active_space_mut().floating;
        let Some(from) = items.iter().position(|f| f.pane == pane) else {
            return false;
        };
        let to = slot.min(items.len());
        let item = items.remove(from);
        items.insert(to - usize::from(to > from), item);
        true
    }
}

pub(crate) fn valid_bounds(r: Rect) -> bool {
    [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite())
        && (240. ..=4096.).contains(&r.width)
        && (160. ..=4096.).contains(&r.height)
}

/// Resolved runtime bounds keep edge chips near their saved docking anchor.
#[derive(Clone, Copy, Debug)]
pub struct FloatingPlacement {
    pub pane: PaneId,
    pub panel: Rect,
    pub chip: Option<Rect>,
}

impl Space {
    pub fn floating_placements(&self, area: Rect) -> Vec<FloatingPlacement> {
        let mut placed: Vec<FloatingPlacement> = Vec::new();
        for f in &self.floating {
            let mut panel = f.fitted(area);
            let chip = f.edge.map(|edge| {
                let vertical = matches!(edge, SplitEdge::Left | SplitEdge::Right);
                let start = if vertical { area.y } else { area.x };
                let available = if vertical { area.height } else { area.width };
                let length = available.clamp(1., 156.);
                let thickness = if vertical { area.width } else { area.height }.clamp(1., 28.);
                let end = start + (available - length).max(0.);
                let preferred = if vertical { f.bounds.y } else { f.bounds.x }.clamp(start, end);
                let mut answer = None;
                // Normally a single edge lane suffices; overflow wraps inward.
                for lane in 0..=self.floating.len() {
                    let inset = lane as f32 * (thickness + 4.);
                    if inset + thickness > if vertical { area.width } else { area.height } {
                        break;
                    }
                    let rect = |position| match edge {
                        SplitEdge::Left => Rect::new(area.x + inset, position, thickness, length),
                        SplitEdge::Right => Rect::new(
                            area.x + area.width - thickness - inset,
                            position,
                            thickness,
                            length,
                        ),
                        SplitEdge::Top => Rect::new(position, area.y + inset, length, thickness),
                        SplitEdge::Bottom => Rect::new(
                            position,
                            area.y + area.height - thickness - inset,
                            length,
                            thickness,
                        ),
                    };
                    let mut candidates = vec![preferred, start, end];
                    for r in placed.iter().filter_map(|p| p.chip) {
                        let (a, b) = if vertical {
                            (r.y, r.y + r.height)
                        } else {
                            (r.x, r.x + r.width)
                        };
                        candidates.push((a - length - 4.).clamp(start, end));
                        candidates.push((b + 4.).clamp(start, end));
                    }
                    candidates.sort_by(|a, b| {
                        (a - preferred)
                            .abs()
                            .total_cmp(&(b - preferred).abs())
                            .then(a.total_cmp(b))
                    });
                    if let Some(r) = candidates.into_iter().map(rect).find(|r| {
                        placed
                            .iter()
                            .filter_map(|p| p.chip)
                            .all(|other| !overlaps(*r, other, 3.))
                    }) {
                        answer = Some(r);
                        break;
                    }
                }
                let chip = answer.unwrap_or_else(|| match edge {
                    SplitEdge::Left => Rect::new(area.x, preferred, thickness, length),
                    SplitEdge::Right => Rect::new(
                        area.x + area.width - thickness,
                        preferred,
                        thickness,
                        length,
                    ),
                    SplitEdge::Top => Rect::new(preferred, area.y, length, thickness),
                    SplitEdge::Bottom => Rect::new(
                        preferred,
                        area.y + area.height - thickness,
                        length,
                        thickness,
                    ),
                });
                // Keep the persistent chip outside the revealed page and controls.
                let mut content = area;
                match edge {
                    SplitEdge::Left => {
                        let inset =
                            (chip.x - area.x + chip.width + 4.).min((area.width - 1.).max(0.));
                        content.x += inset;
                        content.width -= inset;
                    }
                    SplitEdge::Right => {
                        content.width = (chip.x - area.x - 4.).max(1.);
                    }
                    SplitEdge::Top => {
                        let inset =
                            (chip.y - area.y + chip.height + 4.).min((area.height - 1.).max(0.));
                        content.y += inset;
                        content.height -= inset;
                    }
                    SplitEdge::Bottom => {
                        content.height = (chip.y - area.y - 4.).max(1.);
                    }
                }
                panel = f.fitted(content);
                if vertical {
                    panel.y = chip
                        .y
                        .clamp(area.y, area.y + (area.height - panel.height).max(0.));
                } else {
                    panel.x = chip
                        .x
                        .clamp(area.x, area.x + (area.width - panel.width).max(0.));
                }
                chip
            });
            placed.push(FloatingPlacement {
                pane: f.pane,
                panel,
                chip,
            });
        }
        placed
    }
}

fn overlaps(a: Rect, b: Rect, gap: f32) -> bool {
    a.x < b.x + b.width + gap
        && a.x + a.width + gap > b.x
        && a.y < b.y + b.height + gap
        && a.y + a.height + gap > b.y
}
