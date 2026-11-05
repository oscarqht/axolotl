//! Window geometry is independent of the browsing session. Coordinates are logical
//! points, with normal bounds kept separately from zoom/full-screen state.
use gpui::{App, Bounds, DisplayId, Window, WindowBounds, px, size};
#[cfg(not(target_os = "macos"))]
use gpui::{Pixels, point};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
impl Rect {
    fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.width >= 640.
            && self.height >= 420.
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
    #[cfg(not(target_os = "macos"))]
    fn bounds(self) -> Bounds<Pixels> {
        Bounds::new(
            point(px(self.x as f32), px(self.y as f32)),
            size(px(self.width as f32), px(self.height as f32)),
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedWindow {
    version: u32,
    normal: Option<Rect>,
    maximized: bool,
    fullscreen: bool,
    display: Option<uuid::Uuid>,
}
impl SavedWindow {
    fn load(_path: &std::path::Path) -> Option<Self> {
        #[cfg(test)]
        let result = crate::persistence::load_from(_path, "window");
        #[cfg(not(test))]
        let result = crate::persistence::load("window");
        let saved: Self = match result {
            Ok(value) => value?,
            Err(error) => {
                eprintln!("Axolotl: failed to restore window state: {error}");
                return None;
            }
        };
        (saved.version == 1 && saved.normal.is_some_and(Rect::valid)).then_some(saved)
    }
    pub fn restore() -> Self {
        path().and_then(|p| Self::load(&p)).unwrap_or_default()
    }
    pub fn startup(&self, cx: &App) -> (WindowBounds, Option<DisplayId>) {
        // macOS uses native screen coordinates and visibleFrame below, including
        // menu bar/Dock insets. GPUI's macOS display bounds are display-local.
        #[cfg(target_os = "macos")]
        {
            (
                WindowBounds::Windowed(Bounds::centered(None, size(px(1280.), px(860.)), cx)),
                None,
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            let display = cx
                .displays()
                .into_iter()
                .find(|d| d.uuid().ok() == self.display)
                .or_else(|| cx.primary_display());
            let normal = self
                .normal
                .zip(display.as_ref())
                .map(|(rect, display)| {
                    let b = display.bounds();
                    rect.fit(Rect {
                        x: b.origin.x.into(),
                        y: b.origin.y.into(),
                        width: b.size.width.into(),
                        height: b.size.height.into(),
                    })
                    .bounds()
                })
                .unwrap_or_else(|| Bounds::centered(None, size(px(1280.), px(860.)), cx));
            let mode = if self.fullscreen {
                WindowBounds::Fullscreen(normal)
            } else if self.maximized {
                WindowBounds::Maximized(normal)
            } else {
                WindowBounds::Windowed(normal)
            };
            (mode, display.map(|d| d.id()))
        }
    }
}
fn path() -> Option<PathBuf> {
    crate::persistence::data_dir()
}

struct Store {
    state: SavedWindow,
    written: Option<SavedWindow>,
    changed: Instant,
    path: Option<PathBuf>,
}
impl Store {
    fn set(&mut self, state: SavedWindow) {
        if self.state != state {
            self.state = state;
            self.changed = Instant::now();
        }
    }
    fn flush(&mut self) {
        if self.written.as_ref() == Some(&self.state) || self.state.normal.is_none() {
            return;
        }
        let Some(_path) = &self.path else {
            return;
        };
        #[cfg(test)]
        let result = crate::persistence::save_to(_path, "window", &self.state);
        #[cfg(not(test))]
        let result = crate::persistence::save("window", &self.state);
        match result {
            Ok(()) => self.written = Some(self.state.clone()),
            Err(error) => eprintln!("Axolotl: failed to save window state: {error}"),
        }
    }
    fn tick(&mut self) {
        // The shared worker owns the fixed 500 ms batching deadline.
        self.flush();
    }
}

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub(crate) use mac::WindowSaver;
#[cfg(all(feature = "smoke", target_os = "macos"))]
pub(crate) use mac::probe;

#[cfg(not(target_os = "macos"))]
pub(crate) struct WindowSaver(Store);
#[cfg(not(target_os = "macos"))]
impl WindowSaver {
    pub fn new(state: SavedWindow, window: &mut Window, cx: &mut App) -> Self {
        let mut saver = Self(Store {
            state,
            written: None,
            changed: Instant::now(),
            path: path(),
        });
        saver.capture(window, cx);
        saver
    }
    fn capture(&mut self, window: &Window, cx: &App) {
        let mut state = self.0.state.clone();
        state.version = 1;
        state.fullscreen = window.is_fullscreen();
        if !state.fullscreen {
            state.maximized = window.is_maximized();
        }
        if !state.fullscreen && !state.maximized {
            let b = window.window_bounds().get_bounds();
            let rect = Rect {
                x: b.origin.x.into(),
                y: b.origin.y.into(),
                width: b.size.width.into(),
                height: b.size.height.into(),
            };
            if rect.valid() {
                state.normal = Some(rect);
                state.display = window.display(cx).and_then(|d| d.uuid().ok());
            }
        }
        self.0.set(state);
    }
    pub fn tick(&mut self, window: &Window, cx: &App) {
        self.capture(window, cx);
        self.0.tick();
    }
    pub fn flush(&mut self) {
        self.0.flush();
    }
}

#[cfg(not(target_os = "macos"))]
impl Drop for WindowSaver {
    fn drop(&mut self) {
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry_clamps_to_usable_screen_including_negative_origins() {
        let area = Rect {
            x: -1440.,
            y: 30.,
            width: 1440.,
            height: 870.,
        };
        let oversized = Rect {
            x: 40000.,
            y: -40000.,
            width: 1800.,
            height: 1000.,
        };
        assert_eq!(oversized.fit(area), area);
        let normal = Rect {
            x: -1300.,
            y: 90.,
            width: 900.,
            height: 650.,
        };
        assert_eq!(normal.fit(area), normal);
    }
    #[test]
    fn rejects_invalid_geometry_and_unknown_versions() {
        let dir =
            std::env::temp_dir().join(format!("axolotl-window-unit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.clone();
        for json in [
            "{}",
            r#"{"version":1,"normal":{"x":0,"y":0,"width":-1,"height":860},"maximized":false,"fullscreen":false,"display":null}"#,
            r#"{"version":2,"normal":{"x":0,"y":0,"width":1280,"height":860},"maximized":false,"fullscreen":false,"display":null}"#,
        ] {
            crate::persistence::save_to(
                &path,
                "window",
                &serde_json::from_str::<serde_json::Value>(json).unwrap(),
            )
            .unwrap();
            assert!(SavedWindow::load(&path).is_none());
        }
        let state = SavedWindow {
            version: 1,
            normal: Some(Rect {
                x: -900.,
                y: 20.,
                width: 900.,
                height: 650.,
            }),
            maximized: true,
            fullscreen: true,
            display: None,
        };
        let mut store = Store {
            state: state.clone(),
            written: None,
            changed: Instant::now(),
            path: Some(path.clone()),
        };
        store.flush();
        assert_eq!(SavedWindow::load(&path), Some(state));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
