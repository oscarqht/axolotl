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
        static CREATING: Cell<bool> = const { Cell::new(false) };
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
    struct State {
        view: Option<Rc<wry::WebView>>,
        error: Option<String>,
        bounds: Option<wry::Rect>,
        visible: Option<bool>,
        operations: Vec<Operation>,
        protection_script: Rc<RefCell<crate::protection_native::windows::ScriptState>>,
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
                        while let Some(create) =
                            CREATIONS.with(|queue| queue.borrow_mut().pop_front())
                        {
                            create();
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
        pub fn set_bounds(&self, bounds: wry::Rect) -> Result<()> {
            let mut state = self.0.borrow_mut();
            state.bounds = Some(bounds);
            if let Some(view) = state.view.clone() {
                drop(state);
                return Ok(view.set_bounds(bounds)?);
            }
            Ok(())
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
        pub fn raise(&self) -> Result<()> {
            self.call(|view| {
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
            self.call(|view| view.focus())
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
        #[cfg(feature = "smoke")]
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
