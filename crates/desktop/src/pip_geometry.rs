//! Persist native outer bounds independently of page/session state. Native screen
//! coordinates keep multi-display placement and titlebar/taskbar insets accurate.
use anyhow::Result;
use gpui::Window;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

mod native;

thread_local! {
    // Smoke runs have no profile; still remember adjustments between PiP windows.
    static LAST: RefCell<Option<Rect>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.width > 0.
            && self.height > 0.
    }
    #[cfg(target_os = "macos")]
    pub fn overlap(self, other: Self) -> f64 {
        ((self.x + self.width).min(other.x + other.width) - self.x.max(other.x)).max(0.)
            * ((self.y + self.height).min(other.y + other.height) - self.y.max(other.y)).max(0.)
    }
    fn fit(self, area: Self) -> Self {
        let width = self.width.min(area.width);
        let height = self.height.min(area.height);
        Self {
            x: self.x.clamp(area.x, area.x + area.width - width),
            y: self.y.clamp(area.y, area.y + area.height - height),
            width,
            height,
        }
    }
    fn bottom_right(self, area: Self, padding: f64, bottom_origin: bool) -> Self {
        let width = self.width.min((area.width - 2. * padding).max(1.));
        let height = self.height.min((area.height - 2. * padding).max(1.));
        Self {
            x: area.x + area.width - width - padding,
            y: if bottom_origin {
                area.y + padding
            } else {
                area.y + area.height - height - padding
            },
            width,
            height,
        }
        .fit(area)
    }
}
#[derive(Serialize, Deserialize)]
struct Saved {
    version: u32,
    bounds: Rect,
}
fn load(path: &Path) -> Option<Rect> {
    let value: Saved = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (value.version == 1 && value.bounds.valid()).then_some(value.bounds)
}
fn save(path: &Path, bounds: Rect) -> std::io::Result<()> {
    workspace_core::write_atomic(path, &serde_json::to_vec(&Saved { version: 1, bounds })?)
}

pub(super) struct Geometry {
    native: native::Native,
    path: Option<PathBuf>,
    last: Rect,
}
impl Geometry {
    pub fn open(window: &Window, anchor: isize) -> Result<Self> {
        let native = native::Native::new(window, anchor)?;
        let path = crate::persistence::data_dir().map(|dir| dir.join("pip-window-state.json"));
        let saved = path
            .as_ref()
            .and_then(|p| load(p))
            .or_else(|| LAST.with(|last| *last.borrow()));
        let area = native.area(saved)?;
        let bounds = saved.map(|bounds| bounds.fit(area)).unwrap_or_else(|| {
            native.capture().unwrap_or(area).bottom_right(
                area,
                native.padding(),
                cfg!(target_os = "macos"),
            )
        });
        native.apply(bounds)?;
        let last = native.capture()?;
        Ok(Self { native, path, last })
    }
    /// Called for both move and resize, and once more before native teardown.
    pub fn remember(&mut self) {
        let Ok(bounds) = self.native.capture() else {
            return;
        };
        if !bounds.valid() {
            return;
        }
        // Do not write initial placement; only user/native geometry adjustments.
        if self.last == bounds {
            return;
        }
        LAST.with(|last| *last.borrow_mut() = Some(bounds));
        if let Some(path) = &self.path
            && let Err(error) = save(path, bounds)
        {
            eprintln!("PiP window geometry: {error}");
            return;
        }
        self.last = bounds;
    }
    #[cfg(feature = "smoke")]
    pub fn test_state(&self) -> serde_json::Value {
        serde_json::json!({"bounds":self.native.capture().ok(),"area":self.native.area(None).ok(),"padding":self.native.padding()})
    }
    #[cfg(feature = "smoke")]
    pub fn test_adjust(&self) -> Result<Box<dyn FnOnce()>> {
        let area = self.native.area(None)?;
        let target = Rect {
            x: area.x + 80.,
            y: area.y + 90.,
            width: 440.,
            height: 290.,
        }
        .fit(area);
        let native = self.native.clone();
        Ok(Box::new(move || {
            if let Err(error) = native.apply(target) {
                eprintln!("PiP geometry test: {error}");
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_bottom_right_respects_padding_and_both_coordinate_origins() {
        let area = Rect {
            x: -1920.,
            y: 40.,
            width: 1920.,
            height: 1000.,
        };
        let initial = Rect {
            x: 0.,
            y: 0.,
            width: 360.,
            height: 226.,
        };
        assert_eq!(
            initial.bottom_right(area, 24., false),
            Rect {
                x: -384.,
                y: 790.,
                ..initial
            }
        );
        assert_eq!(
            initial.bottom_right(area, 24., true),
            Rect {
                x: -384.,
                y: 64.,
                ..initial
            }
        );
        let tiny = Rect {
            x: 0.,
            y: 0.,
            width: 200.,
            height: 150.,
        };
        assert_eq!(
            initial.bottom_right(tiny, 24., false),
            Rect {
                x: 24.,
                y: 24.,
                width: 152.,
                height: 102.
            }
        );
    }
    #[test]
    fn adjusted_geometry_round_trips_and_disconnected_screen_bounds_are_clamped() {
        let directory =
            std::env::temp_dir().join(format!("axolotl-pip-geometry-{}", uuid::Uuid::new_v4()));
        let path = directory.join("pip-window-state.json");
        let bounds = Rect {
            x: -1600.,
            y: 190.,
            width: 470.,
            height: 310.,
        };
        save(&path, bounds).unwrap();
        assert_eq!(load(&path), Some(bounds));
        assert_eq!(
            bounds.fit(Rect {
                x: -1920.,
                y: 40.,
                width: 1920.,
                height: 1000.
            }),
            bounds
        );
        assert_eq!(
            bounds.fit(Rect {
                x: 0.,
                y: 0.,
                width: 300.,
                height: 200.
            }),
            Rect {
                x: 0.,
                y: 0.,
                width: 300.,
                height: 200.
            }
        );
        for json in [
            "{",
            r#"{"version":2,"bounds":{"x":0,"y":0,"width":400,"height":300}}"#,
            r#"{"version":1,"bounds":{"x":0,"y":0,"width":-1,"height":300}}"#,
        ] {
            std::fs::write(&path, json).unwrap();
            assert!(load(&path).is_none());
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
