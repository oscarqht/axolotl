use crate::{PaneId, SplitId};
use serde::{Deserialize, Serialize};

/// Horizontal splits place panes side by side; vertical splits stack them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// Side selected by a page-content edge drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitEdge {
    Left,
    Right,
    Top,
    Bottom,
}

impl SplitEdge {
    pub fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Horizontal,
            _ => Axis::Vertical,
        }
    }

    pub fn is_first(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }

    /// Normalized distances make corner selection independent of pane aspect ratio.
    pub fn at(bounds: Rect, x: f32, y: f32) -> Option<Self> {
        if bounds.width <= 0. || bounds.height <= 0. || !x.is_finite() || !y.is_finite() {
            return None;
        }
        let x = (x - bounds.x) / bounds.width;
        let y = (y - bounds.y) / bounds.height;
        if !(0. ..1.).contains(&x) || !(0. ..1.).contains(&y) {
            return None;
        }
        let dx = x.min(1. - x);
        let dy = y.min(1. - y);
        if dx > 1. / 3. && dy > 1. / 3. {
            return None;
        }
        Some(if dy <= dx {
            if y < 0.5 {
                Self::Top
            } else {
                Self::Bottom
            }
        } else if x < 0.5 {
            Self::Left
        } else {
            Self::Right
        })
    }

    pub fn preview(self, mut bounds: Rect) -> Rect {
        match self {
            Self::Left | Self::Right => {
                bounds.width /= 2.;
                if self == Self::Right {
                    bounds.x += bounds.width;
                }
            }
            Self::Top | Self::Bottom => {
                bounds.height /= 2.;
                if self == Self::Bottom {
                    bounds.y += bounds.height;
                }
            }
        }
        bounds
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Layout {
    Pane(PaneId),
    Split {
        id: SplitId,
        axis: Axis,
        ratio: f32,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneRect {
    pub id: PaneId,
    pub bounds: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Divider {
    pub id: SplitId,
    pub axis: Axis,
    pub bounds: Rect,
    /// Full parent bounds, used to convert a dragged pointer into a ratio.
    pub container: Rect,
}

impl Divider {
    /// Convert a logical pointer position to a resize ratio, snapping the gutter's
    /// center within ten pixels of a fraction of this split's immediate parent.
    /// Layout placement still enforces the existing pane minimum sizes.
    pub fn drag_ratio(&self, x: f32, y: f32) -> f32 {
        let (position, start, length, gap) = match self.axis {
            Axis::Horizontal => (x, self.container.x, self.container.width, self.bounds.width),
            Axis::Vertical => (
                y,
                self.container.y,
                self.container.height,
                self.bounds.height,
            ),
        };
        let nearest = [0.25, 1. / 3., 0.5, 2. / 3., 0.75]
            .into_iter()
            // Compare in pointer coordinates so subtracting a nested parent's
            // origin cannot round an exact ten-pixel distance above the limit.
            .map(|fraction| start + length * fraction)
            .min_by(|a, b| (position - a).abs().total_cmp(&(position - b).abs()));
        let position = nearest
            .filter(|stop| {
                // Equivalent fraction calculations can differ by one float
                // precision unit. Keep the inclusive boundary at logical 10px.
                let rounding = f32::EPSILON * position.abs().max(stop.abs());
                position.is_finite() && (position - stop).abs() <= 10. + rounding
            })
            .unwrap_or(position);
        (position - start - gap / 2.) / (length - gap).max(1.)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub panes: Vec<PaneRect>,
    pub dividers: Vec<Divider>,
}

impl Layout {
    pub fn pane_ids(&self) -> Vec<PaneId> {
        match self {
            Self::Pane(id) => vec![*id],
            Self::Split { first, second, .. } => {
                let mut ids = first.pane_ids();
                ids.extend(second.pane_ids());
                ids
            }
        }
    }

    pub fn geometry(&self, bounds: Rect, gap: f32, min_size: f32) -> Geometry {
        let mut geometry = Geometry::default();
        let bounds = Rect::new(
            finite(bounds.x),
            finite(bounds.y),
            positive(bounds.width),
            positive(bounds.height),
        );
        self.place(bounds, positive(gap), positive(min_size), &mut geometry);
        geometry
    }

    pub(crate) fn replace_pane(&mut self, pane: PaneId, replacement: Layout) -> bool {
        match self {
            Self::Pane(id) if *id == pane => {
                *self = replacement;
                true
            }
            Self::Pane(_) => false,
            Self::Split { first, second, .. } => {
                if first.pane_ids().contains(&pane) {
                    first.replace_pane(pane, replacement)
                } else {
                    second.replace_pane(pane, replacement)
                }
            }
        }
    }

    pub(crate) fn remove_pane(&mut self, pane: PaneId) -> bool {
        match self {
            Self::Pane(_) => false,
            Self::Split { first, second, .. } => {
                if matches!(first.as_ref(), Self::Pane(id) if *id == pane) {
                    *self = *second.clone();
                    true
                } else if matches!(second.as_ref(), Self::Pane(id) if *id == pane) {
                    *self = *first.clone();
                    true
                } else {
                    first.remove_pane(pane) || second.remove_pane(pane)
                }
            }
        }
    }

    pub(crate) fn resize(&mut self, split: SplitId, value: f32) -> bool {
        match self {
            Self::Pane(_) => false,
            Self::Split {
                id,
                ratio,
                first,
                second,
                ..
            } => {
                if *id == split {
                    if !value.is_finite() {
                        return false;
                    }
                    *ratio = value.clamp(0.1, 0.9);
                    true
                } else {
                    first.resize(split, value) || second.resize(split, value)
                }
            }
        }
    }

    fn minimum(&self, min: f32, gap: f32) -> (f32, f32) {
        match self {
            Self::Pane(_) => (min, min),
            Self::Split {
                axis,
                first,
                second,
                ..
            } => {
                let a = first.minimum(min, gap);
                let b = second.minimum(min, gap);
                match axis {
                    Axis::Horizontal => (a.0 + gap + b.0, a.1.max(b.1)),
                    Axis::Vertical => (a.0.max(b.0), a.1 + gap + b.1),
                }
            }
        }
    }

    fn place(&self, bounds: Rect, gap: f32, min: f32, out: &mut Geometry) {
        let Self::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } = self
        else {
            if let Self::Pane(id) = self {
                out.panes.push(PaneRect { id: *id, bounds });
            }
            return;
        };
        let horizontal = *axis == Axis::Horizontal;
        let length = if horizontal {
            bounds.width
        } else {
            bounds.height
        };
        let gap = gap.min(length);
        let available = length - gap;
        let a_min = first.minimum(min, gap);
        let b_min = second.minimum(min, gap);
        let (a_min, b_min) = if horizontal {
            (a_min.0, b_min.0)
        } else {
            (a_min.1, b_min.1)
        };
        let minimum_total = a_min + b_min;
        let first_length = if minimum_total > available && minimum_total > 0.0 {
            available * a_min / minimum_total
        } else {
            (available * finite(*ratio).clamp(0.1, 0.9)).clamp(a_min, available - b_min)
        };
        let mut a = bounds;
        let mut b = bounds;
        let mut divider = bounds;
        if horizontal {
            a.width = first_length;
            divider.x += first_length;
            divider.width = gap;
            b.x += first_length + gap;
            b.width = available - first_length;
        } else {
            a.height = first_length;
            divider.y += first_length;
            divider.height = gap;
            b.y += first_length + gap;
            b.height = available - first_length;
        }
        out.dividers.push(Divider {
            id: *id,
            axis: *axis,
            bounds: divider,
            container: bounds,
        });
        first.place(a, gap, min, out);
        second.place(b, gap, min, out);
    }
}

fn finite(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}
fn positive(value: f32) -> f32 {
    finite(value).max(0.0)
}
