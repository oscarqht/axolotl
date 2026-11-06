//! Native header/chip and resize surfaces sit above live page views.
use crate::{
    app::{BrowserApp, DragInput},
    native_webview::{BuildNativeChild, NativeWebView},
};
use anyhow::Result;
use gpui::*;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};
use workspace_core::{Geometry, PaneId, PaneRect, Rect, SplitEdge};

pub(crate) const FRAME_INSET: f32 = 2.;
pub(crate) const PAGE_RADIUS: f32 = 8.;

struct Surface {
    view: NativeWebView,
    events: Receiver<Value>,
    bounds: Rect,
    state: String,
}
impl Surface {
    fn new(window: &Window, cx: &App) -> Result<Self> {
        let (tx, events) = crate::ui_events::channel();
        let view = crate::browser::with_inline_html(
            wry::WebViewBuilder::new()
                .with_focused(false)
                .with_visible(false)
                .with_transparent(true)
                .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
                .with_ipc_handler(move |r| {
                    if let Ok(v) = serde_json::from_str(r.body()) {
                        let _ = tx.send(v);
                    }
                }),
            crate::icons::html(include_str!("../assets/floating-panel.html")),
        )
        .build_native_child(window, cx)?;
        view.focus_parent()?;
        Ok(Self {
            view,
            events,
            bounds: Rect::default(),
            state: String::new(),
        })
    }
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_mouse(&self, kind: objc2_app_kit::NSEventType, x: f64, y: f64) -> Result<()> {
        use objc2_app_kit::{NSEvent, NSEventModifierFlags};
        use objc2_foundation::NSPoint;
        use wry::WebViewExtMacOS;
        let window = self.view.ns_window();
        if kind == objc2_app_kit::NSEventType::LeftMouseDown {
            window.makeKeyWindow();
            self.view.focus()?;
        }
        let root = window.contentView().unwrap();
        let location = NSPoint::new(
            x,
            if root.isFlipped() {
                y
            } else {
                root.frame().size.height - y
            },
        );
        let location = root.convertPoint_toView(location, None);
        let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind, location, NSEventModifierFlags::empty(), objc2_foundation::NSProcessInfo::processInfo().systemUptime(), window.windowNumber(), None, 0, 1, 1.,
        ).ok_or_else(|| anyhow::anyhow!("create native drag event"))?;
        match kind {
            objc2_app_kit::NSEventType::LeftMouseDown => self.view.webview().mouseDown(&event),
            objc2_app_kit::NSEventType::LeftMouseDragged => {
                self.view.webview().mouseDragged(&event)
            }
            objc2_app_kit::NSEventType::LeftMouseUp => self.view.webview().mouseUp(&event),
            _ => window.sendEvent(&event),
        }
        Ok(())
    }
    fn place(&mut self, bounds: Rect, state: Value, visible: bool) -> Result<()> {
        if bounds != self.bounds {
            self.view.set_bounds(crate::browser::native_bounds(Bounds {
                origin: point(px(bounds.x), px(bounds.y)),
                size: size(px(bounds.width.max(1.)), px(bounds.height.max(1.))),
            }))?;
            self.bounds = bounds;
        }
        if state["panel"] == true {
            // The native surface, including its backing background, must share
            // the same outer clip as the HTML frame.
            #[cfg(target_os = "macos")]
            {
                use objc2_quartz_core::CACornerMask;
                use wry::WebViewExtMacOS;
                let native = self.view.webview();
                native.setWantsLayer(true);
                if let Some(layer) = native.layer() {
                    if layer.cornerRadius() != 10. {
                        layer.setCornerRadius(10.);
                    }
                    if layer.maskedCorners() != CACornerMask::all() {
                        layer.setMaskedCorners(CACornerMask::all());
                    }
                    if !layer.masksToBounds() {
                        layer.setMasksToBounds(true);
                    }
                }
            }
            #[cfg(target_os = "windows")]
            self.view.set_panel_corner_radius(10.)?;
        }
        let state = state.to_string();
        if state != self.state {
            self.view
                .evaluate_script(&format!("window.update({state})"))?;
            self.state = state;
        }
        self.view.set_visible(visible)?;
        Ok(())
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        let _ = self.view.set_visible(false);
    }
}
struct Chrome {
    header: Surface,
    grip: Surface,
    chip: Surface,
}
#[derive(Default)]
pub struct FloatingPanels {
    chrome: HashMap<PaneId, Chrome>,
    pub revealed: HashSet<PaneId>,
    front: Option<PaneId>,
    outside: HashMap<PaneId, Instant>,
    #[cfg(feature = "smoke")]
    test_pointer: Option<Point<Pixels>>,
    resize: Option<(PaneId, Rect, Rect)>,
    // Hold the source coordinate system until pointer capture ends.
    drag_origin: Option<(PaneId, Rect)>,
}

fn contains(r: Rect, x: f32, y: f32) -> bool {
    x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height
}
fn area(window: &Window) -> Rect {
    let s = window.viewport_size();
    crate::render::panel_content_rect(s.width.into(), s.height.into())
}

impl BrowserApp {
    pub(crate) fn cancel_floating_interaction(&mut self) {
        if let Some((id, bounds, _)) = self.floating_panels.resize.take() {
            self.model.resize_floating(id, bounds);
        }
        self.floating_panels.drag_origin = None;
    }
    pub(crate) fn panel_geometry(&self, window: &Window) -> Geometry {
        let area = area(window);
        let space = self.model.active_space();
        let mut geometry = space.layout.geometry(area, 6., 100.);
        let placements = space.floating_placements(area);
        let mut floating: Vec<_> = space.floating.iter().collect();
        floating.sort_by_key(|f| Some(f.pane) == self.floating_panels.front);
        for f in floating {
            if f.edge.is_none() || f.pinned || self.floating_panels.revealed.contains(&f.pane) {
                let mut r = placements.iter().find(|p| p.pane == f.pane).unwrap().panel;
                r.x += FRAME_INSET;
                r.width = (r.width - 2. * FRAME_INSET).max(1.);
                r.y += 30.;
                r.height = (r.height - 30. - FRAME_INSET).max(1.);
                geometry.panes.push(PaneRect {
                    id: f.pane,
                    bounds: r,
                });
            }
        }
        geometry
    }

    pub(crate) fn prepare_floating_panels(&mut self, window: &Window, cx: &App) {
        let floating = self.model.active_space().floating.clone();
        let ids: HashSet<_> = floating.iter().map(|f| f.pane).collect();
        self.floating_panels.chrome.retain(|id, _| ids.contains(id));
        self.floating_panels.revealed.retain(|id| ids.contains(id));
        self.floating_panels
            .outside
            .retain(|id, _| ids.contains(id));
        let area = area(window);
        let placements = self.model.active_space().floating_placements(area);
        for f in floating {
            let open =
                f.edge.is_none() || f.pinned || self.floating_panels.revealed.contains(&f.pane);
            let placement = placements.iter().find(|p| p.pane == f.pane).unwrap();
            let r = placement.panel;
            let chip = placement.chip.unwrap_or_default();
            let title = self
                .model
                .pane(f.pane)
                .unwrap()
                .active_tab()
                .display_title()
                .to_owned();
            let tab = self.model.pane(f.pane).unwrap().active_tab;
            let url = &self.model.tab(tab).unwrap().url;
            let icon = url::Url::parse(url)
                .ok()
                .filter(|u| matches!(u.scheme(), "http" | "https"))
                .map(|u| format!("{}/favicon.ico", u.origin().ascii_serialization()));
            let dragging = self.tab_drag_active();
            // Sleeping/unavailable pages use GPUI controls, which must remain clickable.
            let framed = self.host.has_view(tab) && !self.sleep.is_sleeping(tab);
            let result = (|| -> Result<()> {
                if let std::collections::hash_map::Entry::Vacant(e) =
                    self.floating_panels.chrome.entry(f.pane)
                {
                    e.insert(Chrome {
                        header: Surface::new(window, cx)?,
                        grip: Surface::new(window, cx)?,
                        chip: Surface::new(window, cx)?,
                    });
                }
                let chrome = self.floating_panels.chrome.get_mut(&f.pane).unwrap();
                let header = if framed {
                    r
                } else {
                    Rect::new(r.x, r.y, r.width, 30.)
                };
                // A chip being dragged keeps its captured surface stationary.
                let header = self
                    .floating_panels
                    .drag_origin
                    .filter(|(id, r)| *id == f.pane && r.width >= 200.)
                    .map_or(header, |(_, r)| r);
                chrome.header.place(
                    header,
                    json!({"title":title,"icon":icon,"edge":f.edge,"pinned":f.pinned,"grip":false,"panel":framed}),
                    open,
                )?;
                let chip = self
                    .floating_panels
                    .drag_origin
                    .filter(|(id, r)| *id == f.pane && r.width < 200.)
                    .map_or(chip, |(_, r)| r);
                chrome.chip.place(
                    chip,
                    json!({"title":title,"icon":icon,"edge":null,"chipSide":f.edge,"pinned":f.pinned,"grip":false}),
                    f.edge.is_some(),
                )?;
                chrome.grip.place(
                    Rect::new(r.x + r.width - 21., r.y + r.height - 21., 19., 19.),
                    json!({"grip":true}),
                    open,
                )?;
                if !dragging {
                    if open {
                        crate::native_webview::raise(&chrome.header.view)?;
                        if self.host.has_view(tab) {
                            self.host.raise(tab)?;
                        }
                        if let Some(bar) = self.bars.get(&f.pane) {
                            bar.raise()?;
                        }
                    }
                    if f.edge.is_some() {
                        crate::native_webview::raise(&chrome.chip.view)?;
                    }
                    if open {
                        crate::native_webview::raise(&chrome.grip.view)?;
                    }
                }
                Ok(())
            })();
            self.report(result);
        }
        if !self.tab_drag_active() {
            if let Some(id) = self.floating_panels.front
                && let Some(c) = self.floating_panels.chrome.get(&id)
                && let Some(f) = self.model.active_space().floating_panel(id)
                && (f.edge.is_none() || f.pinned || self.floating_panels.revealed.contains(&id))
            {
                let tab = self.model.pane(id).unwrap().active_tab;
                let _ = crate::native_webview::raise(&c.header.view);
                if self.host.has_view(tab) {
                    let _ = self.host.raise(tab);
                }
                if let Some(bar) = self.bars.get(&id) {
                    let _ = bar.raise();
                }
                let _ = crate::native_webview::raise(&c.grip.view);
            }
            for f in &self.model.active_space().floating {
                if f.edge.is_some()
                    && let Some(c) = self.floating_panels.chrome.get(&f.pane)
                {
                    let _ = crate::native_webview::raise(&c.chip.view);
                }
            }
        }
    }

    pub(crate) fn reveal_floating(&mut self, pane: PaneId) {
        let Some(f) = self.model.active_space().floating_panel(pane) else {
            return;
        };
        let edge = f.edge;
        self.floating_panels.front = Some(pane);
        let keep: HashSet<_> = self
            .model
            .active_space()
            .floating
            .iter()
            .filter(|f| f.pinned || f.edge != edge)
            .map(|f| f.pane)
            .collect();
        self.floating_panels.revealed.retain(|id| keep.contains(id));
        self.floating_panels.revealed.insert(pane);
        self.floating_panels.outside.remove(&pane);
    }

    pub(crate) fn poll_floating_panels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut events = vec![];
        for (id, c) in &self.floating_panels.chrome {
            for (kind, s) in [("header", &c.header), ("grip", &c.grip), ("chip", &c.chip)] {
                while let Ok(v) = s.events.try_recv() {
                    events.push((*id, kind, s.bounds, v));
                }
            }
        }
        for (pane, kind, bounds, event) in events {
            if self.model.active_space().floating_panel(pane).is_none() {
                continue;
            }
            let position = point(
                px(bounds.x + event["x"].as_f64().unwrap_or(0.) as f32),
                px(bounds.y + event["y"].as_f64().unwrap_or(0.) as f32),
            );
            match event["type"].as_str().unwrap_or("") {
                "ready" => {
                    if let Some(c) = self.floating_panels.chrome.get_mut(&pane) {
                        match kind {
                            "grip" => c.grip.state.clear(),
                            "chip" => c.chip.state.clear(),
                            _ => c.header.state.clear(),
                        }
                    }
                }
                "hover" if self.floating_panels.drag_origin.is_none() => self.reveal_floating(pane),
                "pin" => {
                    self.model.pin_floating(pane);
                    self.reveal_floating(pane);
                }
                "close" => {
                    self.model.close_pane(pane);
                }
                "drag-start" => {
                    let tab = self.model.pane(pane).unwrap().active_tab;
                    self.arm_tab_drag(pane, tab, position);
                    let drag = self.tab_drag.as_mut().unwrap();
                    drag.input = DragInput::NativeBar;
                    drag.whole = true;
                    self.floating_panels.drag_origin = Some((pane, bounds));
                }
                "drag-move" => self.update_native_tab_drag(pane, position, window, cx),
                "drag-end" => {
                    // A drop over a sibling chip on the same edge reorders chips.
                    let target = self
                        .floating_panels
                        .chrome
                        .iter()
                        .find(|(id, c)| {
                            **id != pane
                                && contains(c.chip.bounds, position.x.into(), position.y.into())
                        })
                        .map(|(id, _)| *id);
                    let source_edge = self
                        .model
                        .active_space()
                        .floating_panel(pane)
                        .and_then(|f| f.edge);
                    if let Some(target) = target.filter(|id| {
                        source_edge.is_some()
                            && self
                                .model
                                .active_space()
                                .floating_panel(*id)
                                .is_some_and(|f| f.edge == source_edge)
                    }) {
                        let items = &self.model.active_space().floating;
                        let from = items.iter().position(|f| f.pane == pane).unwrap();
                        let to = items.iter().position(|f| f.pane == target).unwrap();
                        let a = items[from].bounds;
                        let b = items[to].bounds;
                        let vertical =
                            matches!(source_edge, Some(SplitEdge::Left | SplitEdge::Right));
                        let mut source_bounds = a;
                        let mut target_bounds = b;
                        if vertical {
                            source_bounds.y = b.y;
                            target_bounds.y = a.y;
                        } else {
                            source_bounds.x = b.x;
                            target_bounds.x = a.x;
                        }
                        self.model.resize_floating(pane, source_bounds);
                        self.model.resize_floating(target, target_bounds);
                        self.model
                            .reorder_floating(pane, to + usize::from(to > from));
                        self.cancel_tab_drag(cx);
                    } else {
                        self.finish_native_tab_drag(pane, Some(position), window, cx);
                    }
                    self.floating_panels.drag_origin = None;
                }
                "resize-start" => {
                    let r = self
                        .model
                        .active_space()
                        .floating_placements(area(window))
                        .into_iter()
                        .find(|p| p.pane == pane)
                        .unwrap()
                        .panel;
                    let original = self
                        .model
                        .active_space()
                        .floating_panel(pane)
                        .unwrap()
                        .bounds;
                    self.floating_panels.resize = Some((pane, original, r));
                }
                "resize" | "resize-end" => {
                    if let Some((id, original, mut r)) =
                        self.floating_panels.resize.filter(|(id, _, _)| *id == pane)
                    {
                        let f = self.model.active_space().floating_panel(id).unwrap();
                        let dx = event["dx"].as_f64().unwrap_or(0.) as f32;
                        let dy = event["dy"].as_f64().unwrap_or(0.) as f32;
                        r.width += if f.edge == Some(SplitEdge::Right) {
                            -dx
                        } else {
                            dx
                        };
                        r.height += if f.edge == Some(SplitEdge::Bottom) {
                            -dy
                        } else {
                            dy
                        };
                        r.width = r.width.clamp(240., area(window).width.max(240.));
                        r.height = r.height.clamp(160., area(window).height.max(160.));
                        if f.edge.is_some() {
                            r.x = original.x;
                            r.y = original.y;
                        }
                        self.model.resize_floating(id, r);
                    }
                    if event["type"] == "resize-end" {
                        self.floating_panels.resize = None;
                    }
                }
                "cancel" => {
                    self.cancel_tab_drag(cx);
                    self.floating_panels.resize = None;
                    self.floating_panels.drag_origin = None;
                }
                _ => (),
            }
            cx.notify();
        }
        // Native page children do not send shell mouse-move events; query the OS.
        let mut pointer = window.mouse_position();
        #[cfg(target_os = "macos")]
        if let Some(c) = self.floating_panels.chrome.values().next() {
            use wry::WebViewExtMacOS;
            let w = c.header.view.ns_window();
            let p = w.mouseLocationOutsideOfEventStream();
            if let Some(root) = w.contentView() {
                pointer = point(px(p.x as f32), px((root.frame().size.height - p.y) as f32));
            }
        }
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::{
                Foundation::{HWND, POINT},
                Graphics::Gdi::ScreenToClient,
                UI::WindowsAndMessaging::GetCursorPos,
            };
            use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(h) = window.window_handle()
                && let RawWindowHandle::Win32(h) = h.as_raw()
            {
                unsafe {
                    let mut p = POINT::default();
                    if GetCursorPos(&mut p).is_ok()
                        && ScreenToClient(HWND(h.hwnd.get() as _), &mut p).as_bool()
                    {
                        let scale = window.scale_factor();
                        pointer = point(px(p.x as f32 / scale), px(p.y as f32 / scale));
                    }
                }
            }
        }
        // Queued AppKit test events do not move the physical OS pointer.
        #[cfg(feature = "smoke")]
        if let Some(test_pointer) = self.floating_panels.test_pointer {
            pointer = test_pointer;
        }
        let area = area(window);
        let now = Instant::now();
        let mut hide = vec![];
        let placements = self.model.active_space().floating_placements(area);
        for id in &self.floating_panels.revealed {
            let Some(f) = self.model.active_space().floating_panel(*id) else {
                continue;
            };
            let busy = f.pinned
                || self.tab_drag.is_some()
                || self.floating_panels.resize.is_some()
                || self.editing.is_some()
                || self
                    .bars
                    .get(id)
                    .is_some_and(|b| b.menu_open || b.history_open);
            if busy
                || placements
                    .iter()
                    .find(|p| p.pane == *id)
                    .is_some_and(|p| contains(p.panel, pointer.x.into(), pointer.y.into()))
                || self
                    .floating_panels
                    .chrome
                    .get(id)
                    .is_some_and(|c| contains(c.chip.bounds, pointer.x.into(), pointer.y.into()))
            {
                self.floating_panels.outside.remove(id);
            } else if now.duration_since(*self.floating_panels.outside.entry(*id).or_insert(now))
                > Duration::from_millis(400)
            {
                hide.push(*id);
            }
        }
        for id in hide {
            self.floating_panels.revealed.remove(&id);
            self.floating_panels.outside.remove(&id);
            cx.notify();
        }
    }
}

#[cfg(all(feature = "smoke", target_os = "macos"))]
impl BrowserApp {
    pub(crate) fn floating_test_pointer(&mut self, position: Point<Pixels>) {
        self.floating_panels.test_pointer = Some(position);
    }
    pub(crate) fn floating_test_mouse(
        &self,
        pane: PaneId,
        surface: &str,
        kind: objc2_app_kit::NSEventType,
        p: Point<Pixels>,
    ) -> Result<()> {
        let c = &self.floating_panels.chrome[&pane];
        let s = match surface {
            "chip" => &c.chip,
            "grip" => &c.grip,
            _ => &c.header,
        };
        s.test_mouse(kind, f32::from(p.x).into(), f32::from(p.y).into())
    }
    pub(crate) fn floating_test_bounds(&self, pane: PaneId, surface: &str) -> Rect {
        let c = &self.floating_panels.chrome[&pane];
        match surface {
            "chip" => c.chip.bounds,
            "grip" => c.grip.bounds,
            _ => c.header.bounds,
        }
    }
    pub(crate) fn floating_test_native_frame(&self, pane: PaneId) -> Rect {
        use wry::WebViewExtMacOS;
        let frame = self.floating_panels.chrome[&pane]
            .header
            .view
            .webview()
            .frame();
        Rect::new(
            frame.origin.x as f32,
            frame.origin.y as f32,
            frame.size.width as f32,
            frame.size.height as f32,
        )
    }
    pub(crate) fn floating_test_eval(
        &self,
        pane: PaneId,
        surface: &str,
        script: &str,
        cb: impl Fn(String) + Send + 'static,
    ) -> Result<()> {
        let c = &self.floating_panels.chrome[&pane];
        let s = match surface {
            "chip" => &c.chip,
            "grip" => &c.grip,
            _ => &c.header,
        };
        s.view.evaluate_script_with_callback(script, cb)?;
        Ok(())
    }
}

#[cfg(all(feature = "smoke", target_os = "macos"))]
impl BrowserApp {
    pub(crate) fn floating_snapshot(&self, path: &str) -> Result<()> {
        use objc2_app_kit::NSBitmapImageFileType;
        use objc2_foundation::NSDictionary;
        use wry::WebViewExtMacOS;
        let chrome = self
            .floating_panels
            .chrome
            .values()
            .next()
            .ok_or_else(|| anyhow::anyhow!("no floating chrome"))?;
        let root = chrome
            .header
            .view
            .ns_window()
            .contentView()
            .ok_or_else(|| anyhow::anyhow!("no native content view"))?;
        // Exclude transparent controls from the cache: AppKit can otherwise
        // include stale backing pixels and then paint them again in the WebKit
        // snapshots below. Capture each surface exactly once.
        for c in self.floating_panels.chrome.values() {
            for surface in [&c.header, &c.grip, &c.chip] {
                surface.view.set_visible(false)?;
            }
        }
        for bar in self.bars.values() {
            bar.capture_visible(false)?;
        }
        let captured = (|| -> Result<Vec<u8>> {
            let bitmap = root
                .bitmapImageRepForCachingDisplayInRect(root.bounds())
                .ok_or_else(|| anyhow::anyhow!("no native bitmap"))?;
            root.cacheDisplayInRect_toBitmapImageRep(root.bounds(), &bitmap);
            let png = unsafe {
                bitmap.representationUsingType_properties(
                    NSBitmapImageFileType::PNG,
                    &NSDictionary::new(),
                )
            }
            .ok_or_else(|| anyhow::anyhow!("no native png"))?;
            Ok(png.to_vec())
        })();
        for bar in self.bars.values() {
            bar.capture_visible(true)?;
        }
        for f in &self.model.active_space().floating {
            let c = &self.floating_panels.chrome[&f.pane];
            let open =
                f.edge.is_none() || f.pinned || self.floating_panels.revealed.contains(&f.pane);
            c.header.view.set_visible(open)?;
            c.grip.view.set_visible(open)?;
            c.chip.view.set_visible(f.edge.is_some())?;
        }
        std::fs::write(path, captured?)?;
        // Transparent WKWebView layers are omitted by NSView's cache renderer.
        // Capture their actual native surfaces separately at their live bounds.
        let mut surfaces = Vec::new();
        for (id, bar) in &self.bars {
            let file = format!("{path}.{id}.bar.png");
            bar.native_snapshot(file.clone());
            surfaces.push(json!({"file":file,"bounds":bar.bounds}));
        }
        for f in &self.model.active_space().floating {
            let c = &self.floating_panels.chrome[&f.pane];
            let open =
                f.edge.is_none() || f.pinned || self.floating_panels.revealed.contains(&f.pane);
            for (kind, s, shown) in [
                ("header", &c.header, open),
                ("grip", &c.grip, open),
                ("chip", &c.chip, f.edge.is_some()),
            ] {
                if shown {
                    let file = format!("{path}.{}.{kind}.png", f.pane);
                    snapshot_surface(&s.view, file.clone());
                    surfaces.push(json!({"file":file,"bounds":s.bounds}));
                }
            }
        }
        std::fs::write(format!("{path}.json"),json!({"width":root.bounds().size.width,"height":root.bounds().size.height,"surfaces":surfaces}).to_string())?;
        Ok(())
    }
}

#[cfg(all(feature = "smoke", target_os = "macos"))]
pub(crate) fn snapshot_surface(view: &NativeWebView, path: String) {
    use block2::RcBlock;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError};
    use wry::WebViewExtMacOS;
    let callback = RcBlock::new(move |image: *mut NSImage, _error: *mut NSError| unsafe {
        if let Some(image) = image.as_ref()
            && let Some(tiff) = image.TIFFRepresentation()
            && let Some(bitmap) = NSBitmapImageRep::imageRepWithData(&tiff)
            && let Some(png) = bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &NSDictionary::new(),
            )
        {
            let _ = std::fs::write(&path, png.to_vec());
        }
    });
    unsafe {
        view.webview()
            .takeSnapshotWithConfiguration_completionHandler(None, &callback);
    }
}
