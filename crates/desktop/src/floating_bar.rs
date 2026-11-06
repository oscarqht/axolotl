//! GPUI-rendered floating tab controls. No browser engine or script runs here.
use anyhow::Result;
use gpui::{prelude::*, *};
use gpui_component::{
    Root,
    input::{Input, InputEvent, InputState},
};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc, sync::mpsc::Receiver};
use workspace_core::Rect;
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub const MENU_HEIGHT: f32 = 332.;
pub const MENU_WIDTH: f32 = 220.;
#[derive(Default)]
struct StripState {
    model: Value,
    focus_editor: bool,
    history: Vec<crate::browsing_history::HistoryItem>,
    preview: Option<usize>,
    dirty: bool,
    dismiss: bool,
    midpoints: Vec<f32>,
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    scroll_delta: f32,
}
pub struct FloatingBar {
    overlay: Rc<RefCell<Option<Overlay>>>,
    shared: Rc<RefCell<StripState>>,
    events: Receiver<Value>,
    app: AsyncApp,
    placed: Option<Rect>,
    owner_placement: Option<(i32, i32, u32)>,
    pub bounds: Rect,
    pub expanded: bool,
    pub transitioning: bool,
    pub menu_open: bool,
    pub history_open: bool,
    pub midpoints: Vec<f32>,
}
struct Overlay {
    view: Entity<NativeStrip>,
    window: AnyWindowHandle,
    native: NativeWindow,
}
impl FloatingBar {
    pub fn new(parent: &Window, cx: &mut App) -> Result<Self> {
        let (sender, events) = crate::ui_events::channel();
        let shared = Rc::new(RefCell::new(StripState::default()));
        let parent = HasWindowHandle::window_handle(parent)?.as_raw();
        let overlay = Rc::new(RefCell::new(None));
        let weak = Rc::downgrade(&overlay);
        let state = shared.clone();
        cx.defer(move |cx| {
            let Some(overlay) = weak.upgrade() else {
                return;
            };
            let mut strip = None;
            let mut native = None;
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(176.), px(48.)),
                    ))),
                    titlebar: None,
                    kind: WindowKind::PopUp,
                    focus: false,
                    is_movable: false,
                    is_resizable: false,
                    is_minimizable: false,
                    window_background: WindowBackgroundAppearance::Transparent,
                    ..Default::default()
                },
                |window, cx| {
                    native = Some(NativeWindow::attach(window, parent));
                    let view = cx.new(|cx| NativeStrip::new(state, sender, window, cx));
                    strip = Some(view.clone());
                    cx.new(|cx| Root::new(view, window, cx).transparent_overlay())
                },
            );
            match result {
                Ok(window) => match native.unwrap() {
                    Ok(native) => {
                        *overlay.borrow_mut() = Some(Overlay {
                            view: strip.unwrap(),
                            window: window.into(),
                            native,
                        })
                    }
                    Err(error) => {
                        eprintln!("Attaching native chips: {error:#}");
                        let _ = window.update(cx, |_, window, _| window.remove_window());
                    }
                },
                Err(error) => eprintln!("Creating native chips: {error:#}"),
            }
            cx.refresh_windows();
        });
        Ok(Self {
            overlay,
            shared,
            events,
            app: cx.to_async(),
            placed: None,
            owner_placement: None,
            bounds: Rect::new(0., 0., 1., 1.),
            expanded: false,
            transitioning: false,
            menu_open: false,
            history_open: false,
            midpoints: vec![],
        })
    }
    pub fn place(&mut self, pane: Rect, mut state: Value, cx: &mut App) -> Result<()> {
        if state["visible"] == false {
            if let Some(overlay) = self.overlay.borrow().as_ref() {
                let native = overlay.native;
                cx.foreground_executor()
                    .spawn(async move {
                        native.hide();
                    })
                    .detach();
            }
            self.placed = None;
            return Ok(());
        }
        let editing = !state["editing"].is_null();
        let count = state["tabs"].as_array().map_or(0, Vec::len);
        self.bounds = overlay_bounds(
            pane,
            count,
            self.expanded,
            editing,
            self.menu_open,
            self.history_open,
        );
        state["menu"] = json!(self.menu_open);
        let changed = self.shared.borrow().model != state
            || self.shared.borrow().focus_editor
            || self.shared.borrow().dirty;
        self.shared.borrow_mut().dirty = false;
        self.shared.borrow_mut().model = state;
        if let Some(overlay) = self.overlay.borrow().as_ref() {
            let native = overlay.native;
            cx.foreground_executor()
                .spawn(async move {
                    let _ = native.raise();
                })
                .detach();
            let owner_placement = native.parent_placement();
            if self.placed != Some(self.bounds)
                || self.owner_placement != Some(owner_placement)
                || (editing && !overlay.native.has_focus())
            {
                let native = overlay.native;
                let bounds = self.bounds;
                cx.foreground_executor()
                    .spawn(async move {
                        if let Err(error) = native.place(bounds) {
                            eprintln!("Placing native chips: {error:#}");
                        }
                        if editing && !native.has_focus() {
                            let _ = native.focus();
                        }
                    })
                    .detach();
                self.placed = Some(self.bounds);
                self.owner_placement = Some(owner_placement);
            }
            if changed {
                overlay.view.update(cx, |_, cx| cx.notify());
            }
        }
        Ok(())
    }
    pub fn dismiss_controls(&mut self) {
        self.menu_open = false;
        self.history_open = false;
        let mut shared = self.shared.borrow_mut();
        shared.dismiss = true;
        shared.dirty = true;
    }
    pub fn reset(&mut self) {
        self.expanded = false;
        self.menu_open = false;
        self.history_open = false;
        self.midpoints.clear();
        self.shared.borrow_mut().midpoints.clear();
        self.shared.borrow_mut().model = Value::Null;
        self.shared.borrow_mut().dismiss = true;
    }
    pub fn raise(&self) -> Result<()> {
        if let Some(o) = self.overlay.borrow().as_ref() {
            o.native.raise()?;
        }
        Ok(())
    }
    pub fn invalidate(&self) {
        self.shared.borrow_mut().model = Value::Null;
    }
    pub fn focus_editor(&self) -> Result<()> {
        self.shared.borrow_mut().focus_editor = true;
        Ok(())
    }
    pub fn has_focus(&self) -> bool {
        self.overlay
            .borrow()
            .as_ref()
            .is_some_and(|o| o.native.has_focus())
    }
    pub fn next_event(&self) -> Option<Value> {
        self.events.try_recv().ok()
    }
    pub fn preview(&self, index: Option<usize>) -> Result<()> {
        self.shared.borrow_mut().preview = index;
        self.shared.borrow_mut().dirty = true;
        Ok(())
    }
    pub fn show_history(
        &self,
        _: &str,
        items: &[crate::browsing_history::HistoryItem],
    ) -> Result<()> {
        self.shared.borrow_mut().history = items.to_vec();
        self.shared.borrow_mut().dirty = true;
        Ok(())
    }
    pub fn clear_history(&self) -> Result<()> {
        self.shared.borrow_mut().history.clear();
        self.shared.borrow_mut().dirty = true;
        Ok(())
    }
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    pub fn test_mouse(&self, kind: objc2_app_kit::NSEventType, x: f64, y: f64) -> Result<()> {
        use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSView};
        use objc2_foundation::NSPoint;
        let overlay = self.overlay.borrow();
        let native = overlay
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("overlay not ready"))?
            .native;
        if let RawWindowHandle::AppKit(raw) = native.raw {
            unsafe {
                let view = &*(raw.ns_view.as_ptr() as *const NSView);
                let window = view.window().unwrap();
                let local = NSPoint::new(x - self.bounds.x as f64, y - self.bounds.y as f64);
                let location = view.convertPoint_toView(local, None);
                let event=NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(kind,location,NSEventModifierFlags::empty(),objc2_foundation::NSProcessInfo::processInfo().systemUptime(),window.windowNumber(),None,0,1,1.).ok_or_else(||anyhow::anyhow!("create native GPUI mouse event"))?;
                window.sendEvent(&event);
            }
        }
        Ok(())
    }
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    pub fn capture_visible(&self, visible: bool) -> Result<()> {
        use objc2_app_kit::NSView;
        if let Some(o) = self.overlay.borrow().as_ref() {
            if let RawWindowHandle::AppKit(raw) = o.native.raw {
                unsafe {
                    (&*(raw.ns_view.as_ptr() as *const NSView)).setHidden(!visible);
                }
            }
        }
        Ok(())
    }
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    pub fn native_snapshot(&self, path: String) {
        use objc2_app_kit::{NSBitmapImageFileType, NSView};
        use objc2_foundation::NSDictionary;
        if let Some(o) = self.overlay.borrow().as_ref() {
            if let RawWindowHandle::AppKit(raw) = o.native.raw {
                unsafe {
                    let view = &*(raw.ns_view.as_ptr() as *const NSView);
                    if let Some(bitmap) = view.bitmapImageRepForCachingDisplayInRect(view.bounds())
                    {
                        view.cacheDisplayInRect_toBitmapImageRep(view.bounds(), &bitmap);
                        if let Some(png) = bitmap.representationUsingType_properties(
                            NSBitmapImageFileType::PNG,
                            &NSDictionary::new(),
                        ) {
                            let _ = std::fs::write(path, png.to_vec());
                        }
                    }
                }
            }
        }
    }
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    pub fn test_scroll(&self, dx: i32) -> Result<()> {
        // Scroll the GPUI list directly; page WebKit scrolling has its own OS probes.
        self.shared.borrow_mut().scroll_delta += dx as f32;
        self.shared.borrow_mut().dirty = true;
        Ok(())
    }
    #[cfg(all(feature = "smoke", target_os = "windows"))]
    pub fn test_frontmost(&self) -> Result<bool> {
        self.overlay
            .borrow()
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Overlay not ready"))?
            .native
            .frontmost()
    }
    #[cfg(feature = "smoke")]
    pub fn test_viewport(&self, cx: &mut App) -> Result<Size<Pixels>> {
        let overlay = self.overlay.borrow();
        let overlay = overlay
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("overlay not ready"))?;
        cx.update_window(overlay.window, |_, window, _| window.viewport_size())
    }
    #[cfg(feature = "smoke")]
    pub fn test_expand(&self, value: bool, cx: &mut App) {
        if let Some(o) = self.overlay.borrow().as_ref() {
            o.view.update(cx, |view, _| view.expand(value));
        }
    }
    #[cfg(feature = "smoke")]
    pub fn test_action(&self, kind: &str, tab: u64, cx: &mut App) {
        if let Some(o) = self.overlay.borrow().as_ref() {
            o.view.update(cx, |view, _| view.action(kind, tab));
        }
    }
    #[cfg(feature = "smoke")]
    pub fn test_editor(&self, cx: &App) -> (Option<u64>, String) {
        let overlay = self.overlay.borrow();
        let view = overlay.as_ref().unwrap().view.read(cx);
        (view.editing, view.input.read(cx).value().to_string())
    }
    #[cfg(feature = "smoke")]
    pub fn test_navigate(&self, value: &str, cx: &mut App) {
        if let Some(o) = self.overlay.borrow().as_ref() {
            o.view.update(cx, |view, _| {
                view.send("navigate", json!({"tab":view.editing,"value":value}))
            });
        }
    }
    #[cfg(feature = "smoke")]
    pub fn evaluate(&self, _: &str, _: impl Fn(String) + Send + 'static) -> Result<()> {
        anyhow::bail!(
            "JavaScript tab-strip probes have been removed; use native GPUI control assertions"
        )
    }
}
impl Drop for FloatingBar {
    fn drop(&mut self) {
        if let Some(o) = self.overlay.borrow_mut().take() {
            let mut app = self.app.clone();
            self.app
                .foreground_executor()
                .spawn(async move {
                    let _ = app.update_window(o.window, |_, w, _| w.remove_window());
                })
                .detach();
        }
    }
}

fn overlay_bounds(
    pane: Rect,
    count: usize,
    expanded: bool,
    editing: bool,
    menu: bool,
    history: bool,
) -> Rect {
    let available = (pane.width - 24.).max(1.).min(pane.width.max(1.));
    let mut width = if expanded || editing {
        (118. + 176. * count as f32 + if editing { 148. } else { 0. }).min(available)
    } else {
        176_f32.min(pane.width.max(1.))
    };
    if menu {
        width = width.max(MENU_WIDTH).min(pane.width.max(1.));
    }
    let height = (48.
        + if menu {
            MENU_HEIGHT
        } else if editing && history {
            320.
        } else {
            0.
        })
    .min(pane.height.max(1.));
    Rect::new(
        pane.x + (pane.width - width) / 2.,
        (pane.y + pane.height - 12. - height).max(pane.y),
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::overlay_bounds;
    use workspace_core::Rect;
    #[test]
    fn overlay_remains_inside_short_and_narrow_panes() {
        for (w, h) in [(120., 30.), (640., 400.), (1000., 700.)] {
            let p = Rect::new(30., 40., w, h);
            for menu in [false, true] {
                for editing in [false, true] {
                    let r = overlay_bounds(p, 30, true, editing, menu, true);
                    assert!(r.x >= p.x && r.y >= p.y);
                    assert!(r.x + r.width <= p.x + p.width + 0.1);
                    assert!(r.y + r.height <= p.y + p.height + 0.1);
                }
            }
        }
    }
    #[test]
    fn overlay_grows_for_tabs_and_caps_at_pane_edges() {
        let p = Rect::new(0., 0., 1000., 700.);
        assert_eq!(overlay_bounds(p, 1, false, false, false, false).width, 176.);
        assert_eq!(overlay_bounds(p, 1, true, false, false, false).width, 294.);
        assert_eq!(overlay_bounds(p, 2, true, false, false, false).width, 470.);
        assert_eq!(
            overlay_bounds(p, 100, true, false, false, false).width,
            976.
        );
        assert!(overlay_bounds(p, 1, true, true, false, false).width > 294.);
    }
}

struct NativeStrip {
    shared: Rc<RefCell<StripState>>,
    sender: crate::ui_events::Sender<Value>,
    input: Entity<InputState>,
    editing: Option<u64>,
    drag: Option<(u64, bool, Point<Pixels>, bool)>,
    scroll: ScrollHandle,
    menu_tab: Option<u64>,
    rename: bool,
    pinned: bool,
    suppressed: bool,
    last_active: u64,
    history_selected: Option<usize>,
    pane: Option<u64>,
    history_visible: bool,
    hovered: bool,
    collapse: Option<Task<()>>,
}
impl NativeStrip {
    fn new(
        shared: Rc<RefCell<StripState>>,
        sender: crate::ui_events::Sender<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(&input, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let value = if this.rename {
                    input.read(cx).value().to_string()
                } else {
                    this.shared
                        .borrow()
                        .history
                        .get(this.history_selected.unwrap_or(usize::MAX))
                        .map(|i| i.url.clone())
                        .unwrap_or_else(|| input.read(cx).value().to_string())
                };
                this.send(
                    if this.rename { "set-title" } else { "navigate" },
                    json!({"tab":this.editing,"value":value}),
                );
                if this.rename {
                    this.rename = false;
                    this.editing = None;
                    this.menu_tab = None;
                    this.send("menu-close", json!({}));
                }
            } else if matches!(event, InputEvent::Change) && !this.rename {
                this.history_selected = None;
                this.send(
                    "history-query",
                    json!({"tab":this.editing,"value":input.read(cx).value().to_string()}),
                );
            }
        })
        .detach();
        Self {
            shared,
            sender,
            input,
            editing: None,
            drag: None,
            scroll: ScrollHandle::new(),
            menu_tab: None,
            rename: false,
            pinned: false,
            suppressed: false,
            last_active: 0,
            history_selected: None,
            pane: None,
            history_visible: false,
            hovered: false,
            collapse: None,
        }
    }
    fn send(&self, kind: &str, mut value: Value) {
        value["type"] = json!(kind);
        value["pane"] = self.shared.borrow().model["pane"].clone();
        let _ = self.sender.send(value);
    }
    fn move_drag(&mut self, e: &MouseMoveEvent) {
        if let Some((id, whole, origin, started)) = self.drag {
            if !e.dragging() {
                return;
            }
            if !started && (e.position - origin).magnitude() < 5. {
                return;
            }
            if !started {
                self.send(
                    "drag-start",
                    json!({"tab":id,"whole":whole,"x":f32::from(origin.x),"y":f32::from(origin.y)}),
                );
            }
            self.suppressed = true;
            self.drag = Some((id, whole, origin, true));
            self.send(
                "drag-move",
                json!({"x":f32::from(e.position.x),"y":f32::from(e.position.y)}),
            );
        }
    }
    fn finish_drag(&mut self, e: &MouseUpEvent) {
        if let Some(d) = self.drag.take()
            && d.3
        {
            self.send(
                "drag-end",
                json!({"x":f32::from(e.position.x),"y":f32::from(e.position.y)}),
            );
        }
    }
    fn expand(&mut self, value: bool) {
        self.send("expand", json!({"value":value}));
    }
    fn action(&self, kind: &str, tab: u64) {
        if kind == "scroll-left" || kind == "scroll-right" {
            let offset = self.scroll.offset();
            self.scroll.set_offset(point(
                offset.x + px(if kind == "scroll-left" { 190. } else { -190. }),
                offset.y,
            ));
            return;
        }
        if kind == "protection" {
            let state = self.shared.borrow();
            let active = state.model["tabs"]
                .as_array()
                .and_then(|tabs| tabs.iter().find(|t| t["id"] == tab));
            if !active.is_some_and(|t| {
                t["protectionSite"].is_string()
                    && t["protectionGlobal"] == true
                    && t["protectionReady"] != false
            }) {
                return;
            }
        }
        self.send(kind, json!({"tab":tab}));
    }
    fn button(
        &self,
        id: impl Into<ElementId>,
        icon: &'static str,
        kind: &'static str,
        tab: u64,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .size(px(28.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .hover(|s| s.bg(rgb(0x889999)))
            .child(crate::icons::icon(icon, 16.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.action(kind, tab);
                cx.notify();
            }))
    }
}
impl Render for NativeStrip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let moves = cx.entity().downgrade();
        let releases = cx.entity().downgrade();
        #[cfg(all(feature = "smoke", target_os = "macos"))]
        {
            let dx = std::mem::take(&mut self.shared.borrow_mut().scroll_delta);
            if dx != 0. {
                let offset = self.scroll.offset();
                self.scroll.set_offset(point(offset.x + px(dx), offset.y));
            }
        }
        if std::mem::take(&mut self.shared.borrow_mut().dismiss) {
            self.menu_tab = None;
            self.rename = false;
            self.pinned = false;
            self.drag = None;
            self.collapse = None;
        }
        let s = self.shared.borrow().model.clone();
        let pane = s["pane"].as_u64();
        if pane != self.pane {
            self.pane = pane;
            self.shared.borrow_mut().midpoints.clear();
            self.drag = None;
            self.menu_tab = None;
            self.rename = false;
            self.pinned = false;
            self.suppressed = false;
            self.collapse = None;
            self.editing = None;
            self.history_selected = None;
            self.history_visible = false;
            self.scroll.set_offset(point(px(0.), px(0.)));
        }
        let tabs = s["tabs"].as_array().cloned().unwrap_or_default();
        let active = s["active"].as_u64().unwrap_or(0);
        let editing = s["editing"].as_u64();
        let expanded = s["expanded"].as_bool().unwrap_or(false) || editing.is_some() || self.rename;
        if self.last_active != active {
            self.last_active = active;
            self.scroll
                .scroll_to_item(tabs.iter().position(|t| t["id"] == active).unwrap_or(0));
        }
        let focus = std::mem::take(&mut self.shared.borrow_mut().focus_editor);
        if editing != self.editing && !self.rename {
            self.editing = editing;
            self.history_selected = None;
            if let Some(id) = editing
                && let Some(tab) = tabs.iter().find(|t| t["id"] == id)
            {
                self.input.update(cx, |input, cx| {
                    input.set_value(tab["url"].as_str().unwrap_or("").to_owned(), window, cx);
                    input.focus(window, cx);
                    window.dispatch_action(Box::new(gpui_component::input::SelectAll), cx);
                });
            }
        }
        if focus && self.editing.is_some() {
            self.input.update(cx, |input, cx| input.focus(window, cx));
        }
        let total = tabs
            .iter()
            .map(|t| t["notifications"].as_u64().unwrap_or(0))
            .sum::<u64>();
        let c = crate::chrome_theme::ChromeColors::current(cx);
        let mut row = div()
            .id("chips-row")
            .flex()
            .items_center()
            .gap(px(4.))
            .h(px(40.))
            .flex_shrink_0()
            .opacity(if self.hovered { 0.85 } else { 0.5 })
            .w_full()
            .p(px(5.))
            .rounded_full()
            .bg(rgb(c.surface))
            .border_1()
            .border_color(rgb(c.border))
            .text_color(rgb(c.ink));
        row = row.child(
            div()
                .id("panel-handle")
                .size(px(28.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .cursor_grab()
                .child(crate::icons::icon("dots-grid3x3", 18.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, _, _| {
                        this.suppressed = false;
                        this.drag = Some((active, true, e.position, false));
                    }),
                ),
        );
        if expanded {
            let mut list = div()
                .id("tab-list")
                .flex()
                .gap(px(4.))
                .flex_1()
                .min_w_0()
                .overflow_x_scroll()
                .track_scroll(&self.scroll);
            for (index, tab) in tabs.iter().enumerate() {
                let id = tab["id"].as_u64().unwrap_or(0);
                let title = tab["title"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("New tab")
                    .to_owned();
                let count = tab["notifications"].as_u64().unwrap_or(0);
                let shared = self.shared.clone();
                let sender = self.sender.clone();
                let pane = s["pane"].clone();
                let scroll = self.scroll.clone();
                let chip = div()
                    .id(("chip", id))
                    .w(px(if self.editing == Some(id) { 320. } else { 172. }))
                    .h(px(28.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(6.))
                    .rounded_md()
                    .bg(rgb(if active == id {
                        c.active_tab
                    } else {
                        c.surface
                    }))
                    .border_1()
                    .border_color(rgb(if active == id { c.accent } else { c.border }))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, _, _| {
                            if this.editing != Some(id) {
                                this.suppressed = false;
                                this.drag = Some((id, false, e.position, false));
                            }
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, _, cx| {
                            this.menu_tab = Some(id);
                            this.action("menu-open", id);
                            cx.notify();
                        }),
                    )
                    .on_click(cx.listener(move |this, e: &ClickEvent, _, _| {
                        if this.suppressed {
                            return;
                        }
                        this.action(
                            if e.click_count() == 2 {
                                "edit"
                            } else {
                                "select"
                            },
                            id,
                        );
                    }));
                list=list.child(chip.child(if self.editing==Some(id){div().flex_1().min_w_0().child(Input::new(&self.input).appearance(false)).into_any_element()}else{div().flex_1().min_w_0().truncate().child(title).into_any_element()})
                    .when(count>0,|el|el.child(div().text_size(px(10.)).child(count.to_string())))
                    .child(self.button(("close",id),"xmark","close",id,cx))
                    .relative().child(canvas(move |bounds,_,_| bounds, move |bounds,_,window,_| {
                        let midpoint=f32::from(bounds.origin.x+bounds.size.width/2.);
                        let _=(&scroll,window);
                        let mut state=shared.borrow_mut();
                        if state.midpoints.len()<=index {state.midpoints.resize(index+1,f32::NAN);}
                        if !state.midpoints[index].is_finite() || (state.midpoints[index]-midpoint).abs()>0.1 {
                            state.midpoints[index]=midpoint;
                            let _=sender.send(json!({"type":"chip-metric","pane":pane,"index":index,"midpoint":midpoint}));
                        }
                    }).absolute().size_full()));
            }
            let required = tabs
                .iter()
                .map(|t| {
                    if self.editing == t["id"].as_u64() {
                        320.
                    } else {
                        172.
                    }
                })
                .sum::<f32>()
                + 4. * tabs.len().saturating_sub(1) as f32;
            let overflow = required > f32::from(window.viewport_size().width) - 116. + 1.;
            if overflow {
                row =
                    row.child(self.button("scroll-prev", "arrow-left", "scroll-left", active, cx));
            }
            let viewport = div()
                .relative()
                .flex()
                .flex_1()
                .min_w_0()
                .h(px(28.))
                .child(list.on_hover(cx.listener(|this, h: &bool, _, _| {
                    if *h {
                        this.collapse = None;
                        this.expand(true);
                    }
                })))
                .when(overflow, |el| {
                    el.child(
                        canvas(
                            |bounds, _, _| bounds,
                            move |bounds, _, window, _| {
                                let color: Hsla = rgb(c.surface).into();
                                let clear = Hsla { a: 0., ..color };
                                let width = px(24.).min(bounds.size.width / 2.);
                                let left =
                                    Bounds::new(bounds.origin, size(width, bounds.size.height));
                                let right = Bounds::new(
                                    point(bounds.right() - width, bounds.top()),
                                    size(width, bounds.size.height),
                                );
                                window.paint_quad(fill(
                                    left,
                                    linear_gradient(
                                        90.,
                                        linear_color_stop(color, 0.),
                                        linear_color_stop(clear, 1.),
                                    ),
                                ));
                                window.paint_quad(fill(
                                    right,
                                    linear_gradient(
                                        90.,
                                        linear_color_stop(clear, 0.),
                                        linear_color_stop(color, 1.),
                                    ),
                                ));
                            },
                        )
                        .absolute()
                        .size_full(),
                    )
                });
            row = row.child(viewport);
            if overflow {
                row = row.child(self.button(
                    "scroll-next",
                    "arrow-right",
                    "scroll-right",
                    active,
                    cx,
                ));
            }
        } else {
            let index = tabs.iter().position(|t| t["id"] == active).unwrap_or(0) + 1;
            row = row.child(
                div()
                    .id("tab-count")
                    .flex_1()
                    .min_w_0()
                    .cursor_pointer()
                    .child(format!(
                        "{index}/{}{}",
                        tabs.len(),
                        if total > 0 {
                            format!(" * {total}")
                        } else {
                            String::new()
                        }
                    ))
                    .on_hover(cx.listener(|this, h: &bool, _, _| {
                        if *h {
                            this.collapse = None;
                            this.expand(true);
                        }
                    }))
                    .on_click(cx.listener(|this, _, _, _| {
                        this.pinned = !this.pinned;
                        this.expand(true);
                    })),
            );
        }
        row = row
            .child(self.button("protection", "shield-check", "protection", active, cx))
            .child(self.button("new", "plus", "new", active, cx));
        let history = self.shared.borrow().history.clone();
        let show_history = editing.is_some() && !history.is_empty();
        if show_history != self.history_visible {
            self.history_visible = show_history;
            self.send("history-open", json!({"value":show_history}));
        }
        let mut suggestions = div()
            .id("history")
            .flex()
            .flex_col()
            .w_full()
            .max_h(px(300.))
            .overflow_y_scroll()
            .bg(rgb(c.surface))
            .rounded_md();
        for (i, item) in history.iter().enumerate() {
            let url = item.url.clone();
            suggestions = suggestions.child(
                div()
                    .id(("history", i))
                    .px(px(10.))
                    .py(px(6.))
                    .cursor_pointer()
                    .bg(rgb(if Some(i) == self.history_selected {
                        c.hover
                    } else {
                        c.surface
                    }))
                    .child(item.title.clone())
                    .child(div().truncate().text_color(rgb(c.muted)).child(url.clone()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.send("navigate", json!({"tab":this.editing,"value":url}));
                            cx.stop_propagation();
                        }),
                    ),
            );
        }
        let mut menu = div()
            .id("tab-menu")
            .flex()
            .flex_col()
            .w_full()
            .bg(rgb(c.surface))
            .text_color(rgb(c.ink))
            .rounded_md()
            .border_1()
            .border_color(rgb(c.border));
        if let Some(id) = self.menu_tab {
            let tab = tabs
                .iter()
                .find(|t| t["id"] == id)
                .cloned()
                .unwrap_or_default();
            let rename_tab = tab.clone();
            menu = menu.child(
                div()
                    .id("rename")
                    .px(px(12.))
                    .py(px(6.))
                    .cursor_pointer()
                    .child("Set tab title")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.rename = true;
                        this.editing = Some(id);
                        this.menu_tab = None;
                        this.send("menu-close", json!({}));
                        this.expand(true);
                        let value = rename_tab["customTitle"]
                            .as_str()
                            .or(rename_tab["title"].as_str())
                            .unwrap_or("")
                            .to_owned();
                        this.input.update(cx, |input, cx| {
                            input.set_value(value, window, cx);
                            input.focus(window, cx);
                        });
                        cx.notify();
                    })),
            );
            for (label, kind) in [
                ("Edit address", "edit"),
                ("Back", "back"),
                ("Forward", "forward"),
                ("Reload", "reload"),
                ("Pin tab", "pin"),
                ("Sleep tab", "sleep"),
                ("Keep alive", "keep-alive"),
                ("Close tab", "close"),
            ] {
                let enabled = match kind {
                    "back" => tab["canBack"] == true,
                    "forward" => tab["canForward"] == true,
                    "reload" => tab["canReload"] == true,
                    _ => true,
                };
                menu = menu.child(
                    div()
                        .id(kind)
                        .opacity(if enabled { 1. } else { 0.4 })
                        .px(px(12.))
                        .py(px(6.))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(c.hover)))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !enabled {
                                return;
                            }
                            this.action(kind, id);
                            this.menu_tab = None;
                            this.send("menu-close", json!({}));
                            cx.notify();
                        })),
                );
            }
        }
        let show_menu = self.menu_tab.is_some();
        div()
            .id("native-chips")
            .size_full()
            .flex()
            .flex_col()
            .justify_end()
            .p(px(4.))
            .gap(px(4.))
            .text_size(px(12.))
            .on_hover(cx.listener(|this, h: &bool, _, cx| {
                if this.hovered != *h {
                    this.hovered = *h;
                    cx.notify();
                }
                if *h {
                    this.collapse = None;
                } else if this.drag.is_none()
                    && !this.pinned
                    && this.editing.is_none()
                    && this.menu_tab.is_none()
                {
                    this.collapse = Some(cx.spawn(async move |view, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(450))
                            .await;
                        let _ = view.update(cx, |this, _| this.expand(false));
                    }));
                }
            }))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "down" {
                    let len = this.shared.borrow().history.len();
                    if len > 0 {
                        this.history_selected =
                            Some(this.history_selected.map_or(0, |i| (i + 1) % len));
                        cx.stop_propagation();
                        cx.notify();
                    }
                }
                if e.keystroke.key == "up" {
                    let len = this.shared.borrow().history.len();
                    if len > 0 {
                        this.history_selected = Some(
                            this.history_selected
                                .map_or(len - 1, |i| (i + len - 1) % len),
                        );
                        cx.stop_propagation();
                        cx.notify();
                    }
                }
                if e.keystroke.modifiers.control || e.keystroke.modifiers.platform {
                    match e.keystroke.key.as_str() {
                        "l" => this.action(
                            "edit",
                            this.shared.borrow().model["active"].as_u64().unwrap_or(0),
                        ),
                        "t" => this.action("new", 0),
                        "w" => this.action(
                            "close",
                            this.shared.borrow().model["active"].as_u64().unwrap_or(0),
                        ),
                        "r" => this.action(
                            "reload",
                            this.shared.borrow().model["active"].as_u64().unwrap_or(0),
                        ),
                        _ => {}
                    }
                }
                if e.keystroke.key == "escape" {
                    this.drag = None;
                    this.menu_tab = None;
                    this.rename = false;
                    this.pinned = false;
                    this.send("cancel", json!({}));
                    this.send("menu-close", json!({}));
                    cx.notify();
                }
            }))
            .when(show_menu, |el| el.child(menu))
            .when(show_history, |el| el.child(suggestions))
            .child(row)
            .relative()
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        #[cfg(target_os = "windows")]
                        if let Ok(handle) = HasWindowHandle::window_handle(window)
                            && let RawWindowHandle::Win32(raw) = handle.as_raw()
                        {
                            window.on_mouse_event(move |_: &MouseDownEvent, phase, _, _| {
                                if phase == DispatchPhase::Capture {
                                    unsafe {
                                        // Child HWNDs do not activate themselves on click. Give
                                        // menus and title inputs the same keyboard ownership as the URL editor.
                                        let _ =
                                            windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(
                                                Some(windows::Win32::Foundation::HWND(
                                                    raw.hwnd.get() as _,
                                                )),
                                            );
                                    }
                                }
                            });
                        }
                        window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = moves.update(cx, |this, _| this.move_drag(e));
                            }
                        });
                        window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture && e.button == MouseButton::Left {
                                let _ = releases.update(cx, |this, _| this.finish_drag(e));
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn overlay_window_proc(
    hwnd: windows::Win32::Foundation::HWND,
    message: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    id: usize,
    _data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::{
        Foundation::LRESULT,
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass},
            WindowsAndMessaging::*,
        },
    };
    // GPUI 0.2.2 reserves invisible resize-frame insets even for PopUp windows.
    // This borderless overlay must use every pixel of its declared client size.
    // The class background brush is black; GDI must not fill behind the
    // premultiplied DirectComposition surface.
    if message == WM_ERASEBKGND {
        return LRESULT(1);
    }
    if message == WM_NCCALCSIZE || message == WM_NCPAINT {
        return LRESULT(0);
    }
    if message == WM_NCHITTEST {
        return LRESULT(HTCLIENT as isize);
    }
    unsafe {
        if message == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(overlay_window_proc), id);
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

#[derive(Clone, Copy)]
struct NativeWindow {
    raw: RawWindowHandle,
    parent: RawWindowHandle,
}
impl NativeWindow {
    fn attach(window: &Window, parent: RawWindowHandle) -> Result<Self> {
        let raw = HasWindowHandle::window_handle(window)?.as_raw();
        #[cfg(target_os = "windows")]
        unsafe {
            use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};
            if let (RawWindowHandle::Win32(child), RawWindowHandle::Win32(owner)) = (raw, parent) {
                let child = HWND(child.hwnd.get() as _);
                let owner = HWND(owner.hwnd.get() as _);
                if !windows::Win32::UI::Shell::SetWindowSubclass(
                    child,
                    Some(overlay_window_proc),
                    0x41584f4c,
                    0,
                )
                .as_bool()
                {
                    anyhow::bail!("Installing borderless overlay window procedure");
                }
                // Keep a top-level composition surface: WS_CHILD surfaces cannot
                // alpha-blend over sibling WebView2 HWNDs.
                SetWindowLongPtrW(child, GWLP_HWNDPARENT, owner.0 as isize);
                let style = GetWindowLongPtrW(child, GWL_STYLE) as u32;
                SetWindowLongPtrW(
                    child,
                    GWL_STYLE,
                    ((style & !(WS_CHILD.0 | WS_CAPTION.0 | WS_THICKFRAME.0)) | WS_POPUP.0)
                        as isize,
                );
                // DirectComposition's premultiplied surface also needs a full
                // DWM frame extension to blend with the page rather than black.
                // https://chromium.googlesource.com/chromium/src/+/HEAD/ui/views/widget/widget_hwnd_utils.cc
                use windows::Win32::{
                    Graphics::Dwm::DwmExtendFrameIntoClientArea, UI::Controls::MARGINS,
                };
                DwmExtendFrameIntoClientArea(
                    child,
                    &MARGINS {
                        cxLeftWidth: -1,
                        cxRightWidth: -1,
                        cyTopHeight: -1,
                        cyBottomHeight: -1,
                    },
                )?;
            }
        }
        #[cfg(target_os = "macos")]
        unsafe {
            use objc2_app_kit::{NSView, NSWindowOrderingMode};
            if let (RawWindowHandle::AppKit(child), RawWindowHandle::AppKit(owner)) = (raw, parent)
            {
                let child = (&*(child.ns_view.as_ptr() as *const NSView))
                    .window()
                    .unwrap();
                let owner = (&*(owner.ns_view.as_ptr() as *const NSView))
                    .window()
                    .unwrap();
                owner.addChildWindow_ordered(&child, NSWindowOrderingMode::Above);
            }
        }
        Ok(Self { raw, parent })
    }
    fn parent_placement(&self) -> (i32, i32, u32) {
        #[cfg(target_os = "windows")]
        unsafe {
            use windows::Win32::{
                Foundation::{HWND, POINT},
                Graphics::Gdi::ClientToScreen,
                UI::HiDpi::GetDpiForWindow,
            };
            if let RawWindowHandle::Win32(raw) = self.parent {
                let hwnd = HWND(raw.hwnd.get() as _);
                let mut p = POINT::default();
                let _ = ClientToScreen(hwnd, &mut p);
                return (p.x, p.y, GetDpiForWindow(hwnd));
            }
        }
        (0, 0, 0)
    }
    fn hide(&self) {
        #[cfg(target_os = "windows")]
        unsafe {
            use windows::Win32::{
                Foundation::HWND,
                UI::WindowsAndMessaging::{SW_HIDE, ShowWindow},
            };
            if let RawWindowHandle::Win32(raw) = self.raw {
                let _ = ShowWindow(HWND(raw.hwnd.get() as _), SW_HIDE);
            }
        }
        #[cfg(target_os = "macos")]
        unsafe {
            use objc2_app_kit::NSView;
            if let RawWindowHandle::AppKit(raw) = self.raw {
                if let Some(w) = (&*(raw.ns_view.as_ptr() as *const NSView)).window() {
                    w.orderOut(None);
                }
            }
        }
    }
    fn place(&self, r: Rect) -> Result<()> {
        #[cfg(target_os = "macos")]
        unsafe {
            use objc2_app_kit::NSView;
            use objc2_foundation::{NSPoint, NSRect, NSSize};
            if let (RawWindowHandle::AppKit(raw), RawWindowHandle::AppKit(owner)) =
                (self.raw, self.parent)
            {
                let child = (&*(raw.ns_view.as_ptr() as *const NSView))
                    .window()
                    .unwrap();
                let owner_view = &*(owner.ns_view.as_ptr() as *const NSView);
                let owner = owner_view.window().unwrap();
                let y = if owner_view.isFlipped() {
                    r.y as f64
                } else {
                    owner_view.bounds().size.height - (r.y + r.height) as f64
                };
                let rect = NSRect::new(
                    NSPoint::new(r.x as f64, y),
                    NSSize::new(r.width as f64, r.height as f64),
                );
                let rect = owner.convertRectToScreen(owner_view.convertRect_toView(rect, None));
                child.setFrame_display(rect, true);
            }
        }

        #[cfg(target_os = "windows")]
        unsafe {
            use windows::Win32::{
                Foundation::HWND,
                UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::*},
            };
            if let RawWindowHandle::Win32(raw) = self.raw {
                let hwnd = HWND(raw.hwnd.get() as _);
                let parent = if let RawWindowHandle::Win32(raw) = self.parent {
                    HWND(raw.hwnd.get() as _)
                } else {
                    hwnd
                };
                if IsIconic(parent).as_bool() {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    return Ok(());
                }
                let scale = GetDpiForWindow(parent) as f32 / 96.;
                let mut origin = windows::Win32::Foundation::POINT {
                    x: (r.x * scale).round() as i32,
                    y: (r.y * scale).round() as i32,
                };
                windows::Win32::Graphics::Gdi::ClientToScreen(parent, &mut origin).ok()?;
                SetWindowPos(
                    hwnd,
                    Some(HWND_TOP),
                    origin.x,
                    origin.y,
                    (r.width * scale).round() as i32,
                    (r.height * scale).round() as i32,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
                )?;
                windows::Win32::Graphics::Gdi::SetWindowRgn(hwnd, None, true);
            }
        }
        Ok(())
    }
    fn raise(&self) -> Result<()> {
        #[cfg(target_os = "windows")]
        unsafe {
            use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};
            if let RawWindowHandle::Win32(r) = self.raw {
                let owner = if let RawWindowHandle::Win32(p) = self.parent {
                    HWND(p.hwnd.get() as _)
                } else {
                    HWND(r.hwnd.get() as _)
                };
                let foreground = GetForegroundWindow();
                if foreground != owner && foreground != HWND(r.hwnd.get() as _) {
                    return Ok(());
                }
                SetWindowPos(
                    HWND(r.hwnd.get() as _),
                    Some(HWND_TOP),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )?;
            }
        }
        Ok(())
    }
    fn focus(&self) -> Result<()> {
        #[cfg(target_os = "windows")]
        unsafe {
            use windows::Win32::{Foundation::HWND, UI::Input::KeyboardAndMouse::SetFocus};
            if let RawWindowHandle::Win32(r) = self.raw {
                let hwnd = HWND(r.hwnd.get() as _);
                let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd);
                let _ = SetFocus(Some(hwnd));
            }
        }
        #[cfg(target_os = "macos")]
        unsafe {
            use objc2_app_kit::NSView;
            if let RawWindowHandle::AppKit(r) = self.raw {
                let view = &*(r.ns_view.as_ptr() as *const NSView);
                if let Some(window) = view.window() {
                    window.makeKeyWindow();
                }
            }
        }
        Ok(())
    }
    fn has_focus(&self) -> bool {
        #[cfg(target_os = "windows")]
        unsafe {
            use windows::Win32::{
                Foundation::HWND,
                UI::{Input::KeyboardAndMouse::GetFocus, WindowsAndMessaging::IsChild},
            };
            if let RawWindowHandle::Win32(r) = self.raw {
                let hwnd = HWND(r.hwnd.get() as _);
                return GetFocus() == hwnd
                    || IsChild(hwnd, GetFocus()).as_bool()
                    || windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() == hwnd;
            }
        }
        #[cfg(target_os = "macos")]
        unsafe {
            use objc2_app_kit::NSView;
            if let RawWindowHandle::AppKit(r) = self.raw {
                return (&*(r.ns_view.as_ptr() as *const NSView))
                    .window()
                    .is_some_and(|w| w.isKeyWindow());
            }
        }
        false
    }
    #[cfg(all(feature = "smoke", target_os = "windows"))]
    fn frontmost(&self) -> Result<bool> {
        use windows::Win32::{
            Foundation::{HWND, POINT, RECT},
            UI::WindowsAndMessaging::*,
        };
        unsafe {
            if let RawWindowHandle::Win32(r) = self.raw {
                let hwnd = HWND(r.hwnd.get() as _);
                let owner = if let RawWindowHandle::Win32(raw) = self.parent {
                    HWND(raw.hwnd.get() as _)
                } else {
                    HWND::default()
                };
                let actual_owner = GetWindow(hwnd, GW_OWNER)?;
                if actual_owner != owner || !IsWindowVisible(hwnd).as_bool() {
                    anyhow::bail!(
                        "Overlay ownership/visibility: actual owner {actual_owner:?}, expected {owner:?}, visible {}, hwnd {hwnd:?}",
                        IsWindowVisible(hwnd).as_bool()
                    );
                }
                // An unrelated foreground app can cover the entire browser during
                // unattended smoke runs. Ownership still guarantees ordering above
                // its page children; screen hit-testing is meaningful when foreground.
                let foreground = GetForegroundWindow();
                if foreground != owner && foreground != hwnd {
                    return Ok(true);
                }
                let mut rect = RECT::default();
                GetWindowRect(hwnd, &mut rect)?;
                let hit = WindowFromPoint(POINT {
                    x: (rect.left + rect.right) / 2,
                    y: (rect.top + rect.bottom) / 2,
                });
                // External always-on-top tools can cover an active browser too.
                // The regression here is specifically a page child covering its overlay.
                if hit != hwnd
                    && !IsChild(hwnd, hit).as_bool()
                    && (hit == owner || IsChild(owner, hit).as_bool())
                {
                    anyhow::bail!(
                        "Overlay hit test: hit {hit:?}, overlay {hwnd:?}, owner {owner:?}, foreground {foreground:?}, rect {rect:?}"
                    );
                }
                return Ok(true);
            }
        }
        Ok(false)
    }
}
