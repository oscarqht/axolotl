//! Platform-native network enforcement; no JavaScript fetch shim for network lists.
#[cfg(target_os = "macos")]
mod platform {
    use crate::protection::{Policy, SCRIPT_MARKER};
    use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
    use objc2_foundation::{NSError, NSString};
    use objc2_web_kit::{
        WKContentRuleList, WKContentRuleListStore, WKUserScript, WKUserScriptInjectionTime,
    };
    use std::{cell::RefCell, rc::Rc};
    use wry::WebViewExtMacOS;
    type Compilation = Rc<RefCell<Option<Result<Retained<WKContentRuleList>, String>>>>;
    pub struct RuleStore {
        result: Compilation,
    }
    impl RuleStore {
        pub fn new(network: &str, policy: &Policy) -> Self {
            let result: Compilation = Rc::new(RefCell::new(None));
            let Some(mtm) = MainThreadMarker::new() else {
                return Self { result };
            };
            let mut rules: Vec<serde_json::Value> =
                serde_json::from_str(network).expect("parsed rules");
            if !policy.enabled {
                rules = vec![
                    serde_json::json!({"trigger":{"url-filter":".*"},"action":{"type":"ignore-previous-rules"}}),
                ];
            } else if !policy.exceptions.is_empty() {
                // WebKit's domain trigger is evaluated against the main
                // document, so it also bypasses third-party subresources.
                rules.push(serde_json::json!({"trigger":{"url-filter":".*","if-domain":policy.exceptions},"action":{"type":"ignore-previous-rules"}}));
            }
            let json = serde_json::to_string(&rules).unwrap();
            let started = std::time::Instant::now();
            eprintln!("Protection: compiling {} WebKit rules", rules.len());
            let target = result.clone();
            let callback =
                block2::RcBlock::new(move |list: *mut WKContentRuleList, error: *mut NSError| {
                    let value = if !list.is_null() {
                        Ok(unsafe { Retained::retain(list) }.unwrap())
                    } else {
                        Err(if error.is_null() {
                            "WebKit compilation failed".into()
                        } else {
                            unsafe { (&*error).localizedDescription() }.to_string()
                        })
                    };
                    eprintln!(
                        "Protection: WebKit compilation completed in {:?}: {}",
                        started.elapsed(),
                        if value.is_ok() { "ready" } else { "failed" }
                    );
                    *target.borrow_mut() = Some(value);
                });
            unsafe {
                if let Some(store) = WKContentRuleListStore::defaultStore(mtm) {
                    // A stable identifier replaces the compiled cache, rather than
                    // accumulating a file for every automatic update or site switch.
                    store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(Some(&NSString::from_str("axolotl-protection-v1")),Some(&NSString::from_str(&json)),Some(&callback));
                } else {
                    *result.borrow_mut() = Some(Err("WebKit rule store unavailable".into()));
                }
            }
            Self { result }
        }
        pub fn ready(&self) -> bool {
            matches!(*self.result.borrow(), Some(Ok(_)))
        }
        pub fn error(&self) -> Option<String> {
            self.result
                .borrow()
                .as_ref()
                .and_then(|r| r.as_ref().err())
                .cloned()
        }
        pub fn apply(&self, view: &wry::WebView, script: &str) -> anyhow::Result<()> {
            let result = self.result.borrow();
            let list = result
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Native filters still compiling"))?
                .as_ref()
                .map_err(|s| anyhow::anyhow!("{s}"))?;
            unsafe {
                let controller = view.webview().configuration().userContentController();
                controller.removeAllContentRuleLists();
                controller.addContentRuleList(list);
                // Preserve Wry's IPC and all other initialization scripts.
                let scripts = controller.userScripts();
                controller.removeAllUserScripts();
                for old in scripts.iter() {
                    if !old.source().to_string().starts_with(SCRIPT_MARKER) {
                        controller.addUserScript(&old);
                    }
                }
                let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
                    WKUserScript::alloc(MainThreadMarker::new().unwrap()),
                    &NSString::from_str(script),
                    WKUserScriptInjectionTime::AtDocumentStart,
                    false,
                );
                controller.addUserScript(&script);
            }
            Ok(())
        }
    }
}
#[cfg(target_os = "macos")]
pub use platform::RuleStore;

#[cfg(not(target_os = "macos"))]
pub struct RuleStore;
#[cfg(not(target_os = "macos"))]
impl RuleStore {
    pub fn new(_: &str, _: &crate::protection::Policy) -> Self {
        Self
    }
    pub fn ready(&self) -> bool {
        true
    }
    pub fn error(&self) -> Option<String> {
        None
    }
}

#[cfg(target_os = "windows")]
pub mod windows {
    use std::{cell::RefCell, rc::Rc};
    use webview2_com::{
        AddScriptToExecuteOnDocumentCreatedCompletedHandler, Microsoft::Web::WebView2::Win32::*,
        NavigationStartingEventHandler, WebResourceRequestedEventHandler, take_pwstr,
    };
    use windows::core::{HSTRING, Interface, PWSTR};
    use wry::WebViewExtWindows;
    pub fn install(
        view: &wry::WebView,
        protection: Rc<RefCell<crate::protection::Protection>>,
    ) -> anyhow::Result<()> {
        let webview = view.webview();
        let env = view.environment();
        unsafe {
            // Include worker/service-worker requests where the runtime supports it.
            if let Ok(v) = webview.cast::<ICoreWebView2_22>() {
                v.AddWebResourceRequestedFilterWithRequestSourceKinds(
                    &HSTRING::from("*"),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                )?;
            } else {
                webview.AddWebResourceRequestedFilter(
                    &HSTRING::from("*"),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                )?;
            }
            let main_url = Rc::new(RefCell::new(String::new()));
            let pending_url = main_url.clone();
            let mut navigation_token = 0;
            webview.add_NavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let mut uri = PWSTR::null();
                        args.Uri(&mut uri)?;
                        *pending_url.borrow_mut() =
                            take_pwstr(uri).split('#').next().unwrap_or("").to_owned();
                    }
                    Ok(())
                })),
                &mut navigation_token,
            )?;
            let mut token = 0;
            webview.add_WebResourceRequested(
                &WebResourceRequestedEventHandler::create(Box::new(move |sender, args| {
                    let (Some(sender), Some(args)) = (sender, args) else {
                        return Ok(());
                    };
                    let request = args.Request()?;
                    let mut uri = PWSTR::null();
                    request.Uri(&mut uri)?;
                    let uri = take_pwstr(uri);
                    let mut source = PWSTR::null();
                    sender.Source(&mut source)?;
                    let source = take_pwstr(source);
                    let mut resource_context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL;
                    args.ResourceContext(&mut resource_context)?;
                    let kind = match resource_context {
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT if uri == *main_url.borrow() => {
                            "document"
                        }
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => "subdocument",
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => "script",
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => "image",
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => "stylesheet",
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => "font",
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => "media",
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST
                        | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => "xmlhttprequest",
                        _ => "other",
                    };
                    let mut method = PWSTR::null();
                    request.Method(&mut method)?;
                    let method = take_pwstr(method);
                    let p = protection.borrow();
                    // Top-level navigation is never cancelled by a subresource list.
                    if kind != "document"
                        && p.rules
                            .blocks_request(&p.policy, &uri, &source, kind, &method)
                    {
                        let response = env.CreateWebResourceResponse(
                            None,
                            403,
                            &HSTRING::from("Blocked by Axolotl"),
                            &HSTRING::from("Content-Type: text/plain\r\nCache-Control: no-store"),
                        )?;
                        args.SetResponse(&response)?;
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }
        Ok(())
    }
    use crate::protection_script::{Completion, ScriptRegistration};
    #[derive(Default)]
    pub struct ScriptState {
        registration: ScriptRegistration,
        protects_documents: bool,
        in_flight: bool,
        queued: Option<(String, u64)>,
    }
    impl ScriptState {
        pub fn begin_setup(&mut self, url: String) {
            self.protects_documents = true;
            self.registration.defer_navigation(url);
        }
        pub fn defer_navigation(&mut self, url: String) -> bool {
            self.protects_documents && self.registration.defer_navigation(url)
        }
    }
    pub fn script(
        view: &wry::WebView,
        script: &str,
        state: Rc<RefCell<ScriptState>>,
        url: Option<String>,
        reload: bool,
    ) -> anyhow::Result<()> {
        let webview = view.webview();
        let generation = {
            let mut s = state.borrow_mut();
            let generation = s.registration.begin(url, reload);
            if s.in_flight {
                s.queued = Some((script.to_owned(), generation));
                return Ok(());
            }
            s.in_flight = true;
            generation
        };
        register_script(&webview, script, state, generation)
    }
    fn register_script(
        webview: &ICoreWebView2,
        script: &str,
        state: Rc<RefCell<ScriptState>>,
        generation: u64,
    ) -> anyhow::Result<()> {
        let target = webview.clone();
        let callback_state = state.clone();
        let callback = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
            move |error, value: String| {
                if let Err(error) = error {
                    finish_registration(&target, callback_state.clone())?;
                    return Err(error);
                }
                let completion = callback_state
                    .borrow_mut()
                    .registration
                    .complete(generation, value);
                unsafe {
                    match completion {
                        Completion::Stale(id) => {
                            target.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(id))?
                        }
                        Completion::Current { old, url, reload } => {
                            if let Some(old) = old {
                                target
                                    .RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(old))?;
                            }
                            if let Some(url) = url {
                                if url == "about:blank" {
                                    target.NavigateToString(&HSTRING::from(include_str!(
                                        "../assets/start.html"
                                    )))?;
                                } else {
                                    target.Navigate(&HSTRING::from(url))?;
                                }
                            } else if reload {
                                target.Reload()?;
                            }
                        }
                    }
                }
                finish_registration(&target, callback_state)?;
                Ok(())
            },
        ));
        if let Err(error) = unsafe {
            webview.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(script), &callback)
        } {
            state.borrow_mut().in_flight = false;
            return Err(error.into());
        }
        Ok(())
    }
    fn finish_registration(
        target: &ICoreWebView2,
        state: Rc<RefCell<ScriptState>>,
    ) -> windows::core::Result<()> {
        let queued = {
            let mut s = state.borrow_mut();
            s.in_flight = false;
            let queued = s.queued.take();
            if queued.is_some() {
                s.in_flight = true;
            }
            queued
        };
        if let Some((script, generation)) = queued {
            register_script(target, &script, state, generation).map_err(|error| {
                windows::core::Error::new(
                    windows::core::HRESULT(0x80004005u32 as i32),
                    error.to_string(),
                )
            })?;
        }
        Ok(())
    }
}
