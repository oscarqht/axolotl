//! Windows WebView2 construction pumps messages. Run it between GPUI updates.
#[cfg(not(target_os = "windows"))]
pub type NativeWebView = wry::WebView;

pub trait BuildNativeChild {
    fn build_native_child(
        self,
        window: &gpui::Window,
        cx: &gpui::App,
    ) -> anyhow::Result<NativeWebView>;
}

impl BuildNativeChild for wry::WebViewBuilder<'static> {
    fn build_native_child(
        self,
        window: &gpui::Window,
        cx: &gpui::App,
    ) -> anyhow::Result<NativeWebView> {
        #[cfg(not(target_os = "windows"))]
        {
            let _ = cx;
            Ok(self.build_as_child(window)?)
        }
        #[cfg(target_os = "windows")]
        {
            NativeWebView::new(self, window, cx)
        }
    }
}

#[cfg(target_os = "windows")]
pub use windows_view::NativeWebView;
#[cfg(target_os = "windows")]
pub use windows_view::is_creating;

#[cfg(target_os = "windows")]
mod windows_view {
    use anyhow::{Result, anyhow};
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        rc::Rc,
    };
    use wry::raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};

    type Creation = Box<dyn FnOnce()>;
    thread_local! {
        static CREATIONS: RefCell<VecDeque<Creation>> = RefCell::new(VecDeque::new());
        static AFTER_CREATIONS: RefCell<Vec<Creation>> = RefCell::new(Vec::new());
        static CREATING: Cell<bool> = const { Cell::new(false) };
    }

    pub fn is_creating() -> bool {
        CREATING.with(Cell::get)
    }

    struct Parent(RawWindowHandle);
    impl HasWindowHandle for Parent {
        fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
            // Only used on the UI thread while the owning GPUI window exists.
            Ok(unsafe { WindowHandle::borrow_raw(self.0) })
        }
    }

    type Operation = Box<dyn FnOnce(&wry::WebView) -> wry::Result<()>>;
    #[derive(Default)]
    struct PanelClip {
        radius: Cell<Option<f32>>,
        geometry: Cell<Option<(i32, i32, i32, bool)>>,
        bottom_only: Cell<bool>,
    }

    impl PanelClip {
        fn apply(&self, view: &wry::WebView) -> Result<()> {
            use windows::Win32::{
                Foundation::{HWND, RECT},
                Graphics::Gdi::{
                    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, ERROR, RGN_OR,
                    SetWindowRgn,
                },
                UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::GetClientRect},
            };
            use wry::WebViewExtWindows;
            let Some(radius) = self.radius.get() else {
                return Ok(());
            };
            unsafe {
                // Clip Wry's child container: GPUI's overflow mask cannot reach
                // the WebView2 HWND rendered above it. Coordinates are physical.
                let mut container = HWND::default();
                view.controller().ParentWindow(&mut container)?;
                let mut rect = RECT::default();
                GetClientRect(container, &mut rect)?;
                let diameter =
                    (radius * 2. * GetDpiForWindow(container) as f32 / 96.).round() as i32;
                let geometry = (rect.right, rect.bottom, diameter, self.bottom_only.get());
                if self.geometry.get() == Some(geometry) {
                    return Ok(());
                }
                let region =
                    CreateRoundRectRgn(0, 0, rect.right + 1, rect.bottom + 1, diameter, diameter);
                if region.is_invalid() {
                    return Err(anyhow!("Creating browser panel corner region"));
                }
                if self.bottom_only.get() {
                    let top =
                        CreateRectRgn(0, 0, rect.right + 1, (diameter / 2 + 1).min(rect.bottom));
                    if top.is_invalid() {
                        let _ = DeleteObject(region.into());
                        return Err(anyhow!("Creating square panel top region"));
                    }
                    let combined = CombineRgn(region, region, top, RGN_OR);
                    let _ = DeleteObject(top.into());
                    if combined == ERROR {
                        let _ = DeleteObject(region.into());
                        return Err(anyhow!("Combining panel corner regions"));
                    }
                }
                // Windows owns the region after success; release it only on failure.
                if SetWindowRgn(container, Some(region), true) == 0 {
                    let _ = DeleteObject(region.into());
                    return Err(anyhow!("Clipping browser panel corners"));
                }
                self.geometry.set(Some(geometry));
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct State {
        view: Option<Rc<wry::WebView>>,
        error: Option<String>,
        bounds: Option<wry::Rect>,
        visible: Option<bool>,
        operations: Vec<Operation>,
        protection_script: Rc<RefCell<crate::protection_native::windows::ScriptState>>,
        panel_clip: Rc<PanelClip>,
    }
    pub struct NativeWebView(Rc<RefCell<State>>);

    pub struct CaptureVisibility {
        state: std::rc::Weak<RefCell<State>>,
        view: Rc<wry::WebView>,
        original: wry::Rect,
    }
    impl Drop for CaptureVisibility {
        fn drop(&mut self) {
            let (visible, bounds) = self
                .state
                .upgrade()
                .map(|state| {
                    let state = state.borrow();
                    (
                        state.visible.unwrap_or(false),
                        state.bounds.unwrap_or(self.original),
                    )
                })
                .unwrap_or((false, self.original));
            let _ = self.view.set_visible(visible);
            let _ = self.view.set_bounds(bounds);
        }
    }

    impl NativeWebView {
        pub fn new(
            builder: wry::WebViewBuilder<'static>,
            window: &gpui::Window,
            cx: &gpui::App,
        ) -> Result<Self> {
            let parent = Parent(HasWindowHandle::window_handle(window)?.as_raw());
            let state = Rc::new(RefCell::new(State::default()));
            let weak = Rc::downgrade(&state);
            // The raw handle alone does not keep a window alive. Check its GPUI
            // handle before construction, then release the App borrow before Wry.
            let handle = window.window_handle();
            let mut app = cx.to_async();
            CREATIONS.with(|queue| {
                queue.borrow_mut().push_back(Box::new(move || {
                    if weak.upgrade().is_none() {
                        return;
                    }
                    if handle.update(&mut app, |_, _, _| ()).is_err() {
                        return;
                    }
                    let result = builder.build_as_child(&parent);
                    let Some(state) = weak.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(view) => {
                            let (bounds, visible, operations) = {
                                let mut state = state.borrow_mut();
                                (
                                    state.bounds,
                                    state.visible,
                                    std::mem::take(&mut state.operations),
                                )
                            };
                            let setup = (|| -> wry::Result<()> {
                                if let Some(bounds) = bounds {
                                    view.set_bounds(bounds)?;
                                }
                                if let Some(visible) = visible {
                                    view.set_visible(visible)?;
                                }
                                for operation in operations {
                                    operation(&view)?;
                                }
                                Ok(())
                            })();
                            if let Err(error) = setup {
                                eprintln!("Native view setup: {error}");
                            }
                            state.borrow_mut().view = Some(Rc::new(view));
                        }
                        Err(error) => {
                            state.borrow_mut().error = Some(error.to_string());
                        }
                    }
                }))
            });
            if !CREATING.with(|creating| creating.replace(true)) {
                cx.foreground_executor()
                    .spawn(async move {
                        // A single worker preserves native sibling order even though
                        // each WebView2 constructor runs a nested Windows message loop.
                        loop {
                            while let Some(create) =
                                CREATIONS.with(|queue| queue.borrow_mut().pop_front())
                            {
                                create();
                            }
                            // Constructors put new HWNDs on top and can change
                            // focus while pumping messages. Present chrome only
                            // after every pending sibling has actually been built.
                            let pending = AFTER_CREATIONS
                                .with(|queue| std::mem::take(&mut *queue.borrow_mut()));
                            if pending.is_empty() {
                                break;
                            }
                            for present in pending {
                                present();
                            }
                        }
                        CREATING.with(|creating| creating.set(false));
                    })
                    .detach();
            }
            Ok(Self(state))
        }

        fn call(
            &self,
            operation: impl FnOnce(&wry::WebView) -> wry::Result<()> + 'static,
        ) -> Result<()> {
            let mut state = self.0.borrow_mut();
            if let Some(error) = &state.error {
                return Err(anyhow!("Creating native view: {error}"));
            }
            if let Some(view) = state.view.clone() {
                drop(state);
                return Ok(operation(&view)?);
            }
            state.operations.push(Box::new(operation));
            Ok(())
        }
        fn after_creations(
            &self,
            operation: impl FnOnce(&wry::WebView) -> wry::Result<()> + 'static,
        ) -> Result<()> {
            if !is_creating() {
                return self.call(operation);
            }
            let weak = Rc::downgrade(&self.0);
            AFTER_CREATIONS.with(|queue| {
                queue.borrow_mut().push(Box::new(move || {
                    if let Some(state) = weak.upgrade()
                        && let Err(error) = NativeWebView(state).call(operation)
                    {
                        eprintln!("Presenting native view: {error}");
                    }
                }))
            });
            Ok(())
        }
        pub fn set_bounds(&self, bounds: wry::Rect) -> Result<()> {
            let mut state = self.0.borrow_mut();
            state.bounds = Some(bounds);
            if let Some(view) = state.view.clone() {
                let clip = state.panel_clip.clone();
                drop(state);
                view.set_bounds(bounds)?;
                return clip.apply(&view);
            }
            Ok(())
        }
        pub fn set_panel_corner_radius(&self, radius: f32) -> Result<()> {
            self.set_panel_corner_shape(radius, false)
        }
        pub fn set_panel_corner_shape(&self, radius: f32, bottom_only: bool) -> Result<()> {
            let clip = self.0.borrow().panel_clip.clone();
            clip.radius.set(Some(radius));
            clip.bottom_only.set(bottom_only);
            self.call(move |view| {
                clip.apply(view)
                    .map_err(|error| wry::Error::Io(std::io::Error::other(error.to_string())))
            })
        }
        pub fn set_visible(&self, visible: bool) -> Result<()> {
            let mut state = self.0.borrow_mut();
            state.visible = Some(visible);
            if let Some(view) = state.view.clone() {
                drop(state);
                return Ok(view.set_visible(visible)?);
            }
            Ok(())
        }
        #[cfg(feature = "smoke")]
        pub fn test_panel_clip_state(&self) -> Result<serde_json::Value> {
            use windows::Win32::{
                Foundation::{HWND, RECT},
                Graphics::Gdi::{
                    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, ERROR, EqualRgn,
                    GetWindowRgn, PtInRegion, RGN_OR,
                },
                UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::GetClientRect},
            };
            use wry::WebViewExtWindows;
            let view = self
                .0
                .borrow()
                .view
                .clone()
                .ok_or_else(|| anyhow!("View is not ready"))?;
            unsafe {
                let mut container = HWND::default();
                view.controller().ParentWindow(&mut container)?;
                let mut rect = RECT::default();
                GetClientRect(container, &mut rect)?;
                let scale = f64::from(GetDpiForWindow(container)) / 96.;
                let region = CreateRectRgn(0, 0, 0, 0);
                let clips = GetWindowRgn(container, region).0 != ERROR;
                let corners = [
                    (0, 0),
                    (rect.right - 1, 0),
                    (0, rect.bottom - 1),
                    (rect.right - 1, rect.bottom - 1),
                ]
                .into_iter()
                .enumerate()
                .fold(0, |mask, (i, (x, y))| {
                    mask | if clips && !PtInRegion(region, x, y).as_bool() {
                        1 << i
                    } else {
                        0
                    }
                });
                let center = PtInRegion(region, rect.right / 2, rect.bottom / 2).as_bool();
                let clip = self.0.borrow().panel_clip.clone();
                let radius = clip
                    .radius
                    .get()
                    .unwrap_or(crate::render::PANEL_CORNER_RADIUS);
                let diameter = (f64::from(radius) * 2. * scale).round() as i32;
                let expected =
                    CreateRoundRectRgn(0, 0, rect.right + 1, rect.bottom + 1, diameter, diameter);
                if clip.bottom_only.get() {
                    let top =
                        CreateRectRgn(0, 0, rect.right + 1, (diameter / 2 + 1).min(rect.bottom));
                    CombineRgn(expected, expected, top, RGN_OR);
                    let _ = DeleteObject(top.into());
                }
                let matches_radius = clips && EqualRgn(region, expected).as_bool();
                let _ = DeleteObject(expected.into());
                let _ = DeleteObject(region.into());
                Ok(serde_json::json!({
                    "radius": matches_radius.then_some(radius),
                    "clips": clips && center,
                    "corners": corners,
                    "width": f64::from(rect.right) / scale,
                    "height": f64::from(rect.bottom) / scale,
                }))
            }
        }

        pub fn raise(&self) -> Result<()> {
            self.after_creations(|view| {
                use windows::Win32::{
                    Foundation::HWND,
                    UI::WindowsAndMessaging::{
                        HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
                    },
                };
                use wry::WebViewExtWindows;
                unsafe {
                    let mut parent = HWND::default();
                    view.controller().ParentWindow(&mut parent)?;
                    SetWindowPos(
                        parent,
                        Some(HWND_TOP),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    )?;
                }
                Ok(())
            })
        }
        pub fn focus(&self) -> Result<()> {
            // MoveFocus can bounce through the parent HWND and emit blur/focus
            // events even when this controller already owns keyboard focus.
            if !is_creating() && self.has_focus() {
                return Ok(());
            }
            self.after_creations(|view| view.focus())
        }
        pub fn has_focus(&self) -> bool {
            use windows::Win32::{
                Foundation::HWND,
                UI::{Input::KeyboardAndMouse::GetFocus, WindowsAndMessaging::IsChild},
            };
            use wry::WebViewExtWindows;
            let Some(view) = self.0.borrow().view.clone() else {
                return false;
            };
            unsafe {
                let mut container = HWND::default();
                if view.controller().ParentWindow(&mut container).is_err() {
                    return false;
                }
                let focused = GetFocus();
                focused == container || IsChild(container, focused).as_bool()
            }
        }
        #[cfg(feature = "smoke")]
        pub fn test_frontmost(&self) -> Result<bool> {
            use windows::Win32::{
                Foundation::{HWND, POINT, RECT},
                Graphics::Gdi::MapWindowPoints,
                UI::WindowsAndMessaging::{
                    CWP_SKIPINVISIBLE, ChildWindowFromPointEx, GetClientRect, GetParent,
                },
            };
            use wry::WebViewExtWindows;
            let view = self
                .0
                .borrow()
                .view
                .clone()
                .ok_or_else(|| anyhow!("View is not ready"))?;
            unsafe {
                let mut container = HWND::default();
                view.controller().ParentWindow(&mut container)?;
                let mut rect = RECT::default();
                GetClientRect(container, &mut rect)?;
                let parent = GetParent(container)?;
                let mut center = [POINT {
                    x: rect.right / 2,
                    y: rect.bottom / 2,
                }];
                MapWindowPoints(Some(container), Some(parent), &mut center);
                Ok(ChildWindowFromPointEx(parent, center[0], CWP_SKIPINVISIBLE) == container)
            }
        }
        pub fn focus_parent(&self) -> Result<()> {
            // SetFocus returns the previous HWND, which can be null on success.
            // Wry's windows-rs wrapper reports this as an error with HRESULT 0.
            self.call(|view| match view.focus_parent() {
                Err(wry::Error::WebView2Error(webview2_com::Error::WindowsError(error)))
                    if error.code().0 == 0 =>
                {
                    Ok(())
                }
                result => result,
            })
        }
        pub fn setup_downloads(
            &self,
            manager: Rc<RefCell<crate::downloads::DownloadManager>>,
        ) -> Result<()> {
            self.call(move |view| {
                crate::downloads::install(view, manager)
                    .map_err(|error| wry::Error::Io(std::io::Error::other(error.to_string())))
            })
        }
        pub fn setup_protection(
            &self,
            protection: Rc<RefCell<crate::protection::Protection>>,
            script: String,
            url: String,
            initial: bool,
            reload: bool,
        ) -> Result<()> {
            let id = self.0.borrow().protection_script.clone();
            if initial {
                id.borrow_mut().begin_setup(url);
            }
            self.call(move |view| {
                if initial {
                    crate::protection_native::windows::install(view, protection)
                        .map_err(|e| wry::Error::Io(std::io::Error::other(e.to_string())))?;
                }
                crate::protection_native::windows::script(view, &script, id, None, reload)
                    .map_err(|e| wry::Error::Io(std::io::Error::other(e.to_string())))
            })
        }
        /// Install after deferred WebView2 construction, before queued navigation.
        pub fn observe_visits(&self, callback: impl Fn(Option<String>) + 'static) -> Result<()> {
            self.call(move |view| {
                use webview2_com::{
                    NavigationCompletedEventHandler, NavigationStartingEventHandler, take_pwstr,
                };
                use wry::WebViewExtWindows;
                let callback = Rc::new(callback);
                let starting = callback.clone();
                let completed = Cell::new(None);
                unsafe {
                    let webview = view
                        .controller()
                        .CoreWebView2()
                        .map_err(webview2_com::Error::from)?;
                    webview
                        .add_NavigationStarting(
                            &NavigationStartingEventHandler::create(Box::new(move |_, _| {
                                starting(None);
                                Ok(())
                            })),
                            &mut 0,
                        )
                        .map_err(webview2_com::Error::from)?;
                    webview
                        .add_NavigationCompleted(
                            &NavigationCompletedEventHandler::create(Box::new(
                                move |webview, args| {
                                    if let (Some(webview), Some(args)) = (webview, args) {
                                        let mut success = windows::core::BOOL::default();
                                        let mut id = 0;
                                        args.IsSuccess(&mut success)?;
                                        args.NavigationId(&mut id)?;
                                        if success.as_bool()
                                            && completed.replace(Some(id)) != Some(id)
                                        {
                                            let mut source = windows::core::PWSTR::null();
                                            webview.Source(&mut source)?;
                                            callback(Some(take_pwstr(source)));
                                        }
                                    }
                                    Ok(())
                                },
                            )),
                            &mut 0,
                        )
                        .map_err(webview2_com::Error::from)?;
                }
                Ok(())
            })
        }
        pub fn zoom(&self, factor: f64) -> Result<()> {
            self.call(move |view| view.zoom(factor))
        }
        pub fn install_page_shortcuts(
            &self,
            callback: impl Fn(&'static str) + 'static,
        ) -> Result<()> {
            self.call(move |view| {
                use webview2_com::{
                    AcceleratorKeyPressedEventHandler, Microsoft::Web::WebView2::Win32::*,
                };
                use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
                use wry::WebViewExtWindows;
                unsafe {
                    view.controller()
                        .add_AcceleratorKeyPressed(
                            &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                                let Some(args) = args else {
                                    return Ok(());
                                };
                                let mut kind = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
                                let mut key = 0;
                                args.KeyEventKind(&mut kind)?;
                                args.VirtualKey(&mut key)?;
                                if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN {
                                    return Ok(());
                                }
                                let down = |vk| GetKeyState(vk) < 0;
                                let text = match key {
                                    0xBB | 0x6B => "=",
                                    0xBD | 0x6D => "-",
                                    0x30 | 0x60 => "0",
                                    0x46 => "f",
                                    _ => "",
                                };
                                if let Some(operation) = crate::page_tools::shortcut(
                                    text,
                                    down(0x11),
                                    down(0x10),
                                    down(0x12) || down(0x5B) || down(0x5C),
                                ) {
                                    args.SetHandled(true)?;
                                    callback(match operation {
                                        crate::page_tools::Operation::ZoomIn => "zoom-in",
                                        crate::page_tools::Operation::ZoomOut => "zoom-out",
                                        crate::page_tools::Operation::ZoomReset => "zoom-reset",
                                        crate::page_tools::Operation::Find => "find-page",
                                    });
                                }
                                Ok(())
                            })),
                            &mut 0,
                        )
                        .map_err(webview2_com::Error::from)?;
                }
                Ok(())
            })
        }
        pub fn reload(&self) -> Result<()> {
            self.call(|view| view.reload())
        }
        pub fn load_url(&self, url: &str) -> Result<()> {
            let url = url.to_owned();
            if self
                .0
                .borrow()
                .protection_script
                .borrow_mut()
                .defer_navigation(url.clone())
            {
                return Ok(());
            }
            self.call(move |view| view.load_url(&url))
        }
        pub fn load_html(&self, html: &str) -> Result<()> {
            let html = html.to_owned();
            if self
                .0
                .borrow()
                .protection_script
                .borrow_mut()
                .defer_navigation("about:blank".into())
            {
                return Ok(());
            }
            self.call(move |view| view.load_html(&html))
        }
        pub fn evaluate_script(&self, script: &str) -> Result<()> {
            let script = script.to_owned();
            self.call(move |view| view.evaluate_script(&script))
        }
        pub fn evaluate_script_with_callback(
            &self,
            script: &str,
            callback: impl Fn(String) + Send + 'static,
        ) -> Result<()> {
            let script = script.to_owned();
            self.call(move |view| view.evaluate_script_with_callback(&script, callback))
        }
        pub fn url(&self) -> Result<String> {
            let state = self.0.borrow();
            Ok(match &state.view {
                Some(view) => view.url()?,
                None => "about:blank".into(),
            })
        }
        pub fn prepare_capture(&self) -> Result<Option<CaptureVisibility>> {
            let state = self.0.borrow();
            if state.visible == Some(true) {
                return Ok(None);
            }
            let view = state
                .view
                .clone()
                .ok_or_else(|| anyhow!("Native view is still being created"))?;
            drop(state);
            let original = view.bounds()?;
            let guard = CaptureVisibility {
                state: Rc::downgrade(&self.0),
                view: view.clone(),
                original,
            };
            // Hidden WebView2 controllers do not complete CapturePreview. Render
            // outside the parent's clipping area without covering active pages.
            view.set_bounds(wry::Rect {
                position: wry::dpi::PhysicalPosition::new(-32000, -32000).into(),
                size: original.size,
            })?;
            view.set_visible(true)?;
            Ok(Some(guard))
        }
        pub fn webview(
            &self,
        ) -> Result<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2> {
            use wry::WebViewExtWindows;
            let state = self.0.borrow();
            Ok(state
                .view
                .as_ref()
                .ok_or_else(|| anyhow!("Native view is still being created"))?
                .webview())
        }
    }
}

/// Reorder an existing child without destroying its document, history or focus state.
pub fn raise(view: &NativeWebView) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWindowOrderingMode;
        use wry::WebViewExtMacOS;
        let child = view.webview();
        if let Some(parent) = unsafe { child.superview() } {
            parent.addSubview_positioned_relativeTo(&child, NSWindowOrderingMode::Above, None);
        }
    }
    #[cfg(target_os = "windows")]
    view.raise()?;
    Ok(())
}
