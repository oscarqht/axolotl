//! WKWebView owns WebAuthn. This module only requests browser-level permission;
//! it never sees relying parties, challenges, credentials, or private keys.
use block2::RcBlock;
use gpui::{App, Global, PromptLevel, actions};
use objc2_authentication_services::{
    ASAuthorizationWebBrowserPublicKeyCredentialManager as Manager,
    ASAuthorizationWebBrowserPublicKeyCredentialManagerAuthorizationState as State,
};
use objc2_core_foundation::{CFBoolean, CFString};
use objc2_security::SecTask;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

actions!(axolotl, [EnablePasskeys]);

#[derive(Default)]
struct PermissionRequest(bool);
impl Global for PermissionRequest {}

#[derive(Debug, PartialEq, Eq)]
enum Access {
    Unsupported,
    MissingEntitlement,
    Authorized,
    Denied,
    Request,
    Unknown,
    TimedOut,
}

fn access(supported: bool, entitled: bool, state: isize) -> Access {
    match (supported, entitled, state) {
        (false, _, _) => Access::Unsupported,
        (_, false, _) => Access::MissingEntitlement,
        (_, _, 0) => Access::Authorized,
        (_, _, 1) => Access::Denied,
        (_, _, 2) => Access::Request,
        _ => Access::Unknown,
    }
}

impl Access {
    fn message(&self) -> &'static str {
        match self {
            Self::Unsupported => "Browser passkey permission requires macOS 13.3 or later.",
            Self::MissingEntitlement => {
                "This build is not signed with the browser passkey entitlement. Use an Apple-approved, provisioned build; unsigned and ad-hoc development builds cannot enable browser passkeys. See docs/passkeys-macos.md."
            }
            Self::Authorized => {
                "Axolotl has permission to use passkeys. Open a website's passkey registration or sign-in flow to continue. macOS and your credential provider handle authentication; permission alone does not guarantee a provider is available."
            }
            Self::Denied => {
                "Passkey access was denied or cancelled. You can change Axolotl's access in System Settings → Passwords → Password Options (or the equivalent Passwords settings on your macOS version). Other website sign-in methods remain available."
            }
            Self::Request => {
                "Passkey permission has not been granted. Choose Enable Passkeys again when ready."
            }
            Self::Unknown => {
                "macOS did not return a recognized passkey permission state. No access was assumed; other website sign-in methods remain available."
            }
            Self::TimedOut => {
                "macOS has not completed the passkey permission request. Axolotl is still usable. Dismiss any open system permission dialog before trying again."
            }
        }
    }
}

fn has_entitlement() -> bool {
    // Inspect our running task, not a file path or a user-supplied preference.
    // Missing, malformed and unreadable entitlements all fail closed. This is a
    // preflight only: macOS still enforces provisioning and user authorization.
    unsafe {
        SecTask::from_self(None)
            .and_then(|task| {
                task.value_for_entitlement(
                    &CFString::from_static_str(
                        "com.apple.developer.web-browser.public-key-credential",
                    ),
                    std::ptr::null_mut(),
                )
            })
            .and_then(|value| value.downcast::<CFBoolean>().ok())
            .is_some_and(|value| value.value())
    }
}

pub fn enable(cx: &mut App) {
    if cx.default_global::<PermissionRequest>().0 {
        return;
    }
    let Some(handle) = cx.active_window() else {
        return;
    };
    cx.default_global::<PermissionRequest>().0 = true;
    cx.spawn(async move |cx| {
        let result = if !objc2::available!(macos = 13.3) {
            Access::Unsupported
        } else if !has_entitlement() {
            Access::MissingEntitlement
        } else {
            // All manager calls are made on GPUI's foreground thread after
            // the OS/entitlement checks. The manager stays alive until the
            // callback completes or our nonblocking UI timeout expires.
            let manager = unsafe { Manager::new() };
            let state = unsafe { manager.authorizationStateForPlatformCredentials() };
            let initial = access(true, true, state.0);
            if initial != Access::Request {
                initial
            } else {
                let (sender, receiver) = mpsc::channel();
                let callback = RcBlock::new(move |state: State| {
                    let _ = sender.send(state.0);
                });
                unsafe { manager.requestAuthorizationForPublicKeyCredentials(&callback) };
                let started = Instant::now();
                loop {
                    match receiver.try_recv() {
                        Ok(state) => break access(true, true, state),
                        Err(mpsc::TryRecvError::Disconnected) => break Access::Unknown,
                        Err(mpsc::TryRecvError::Empty) => {}
                    }
                    if started.elapsed() >= Duration::from_secs(60) {
                        break Access::TimedOut;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                }
            }
        };
        let _ = cx.update(|cx| {
            // Clear app-wide state even when the requesting window closed.
            cx.default_global::<PermissionRequest>().0 = false;
            let _ = handle.update(cx, |_, window, cx| {
                window.prompt(
                    PromptLevel::Info,
                    "Website passkeys",
                    Some(result.message()),
                    &["OK"],
                    cx,
                )
            });
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_and_entitlement_gate_every_state() {
        for state in [-1, 0, 1, 2, 3] {
            assert_eq!(access(false, true, state), Access::Unsupported);
            assert_eq!(access(false, false, state), Access::Unsupported);
            assert_eq!(access(true, false, state), Access::MissingEntitlement);
        }
    }

    #[test]
    fn only_undetermined_access_prompts_and_unknown_states_fail_closed() {
        assert_eq!(access(true, true, 0), Access::Authorized);
        assert_eq!(access(true, true, 1), Access::Denied);
        assert_eq!(access(true, true, 2), Access::Request);
        assert_eq!(access(true, true, -1), Access::Unknown);
        assert_eq!(access(true, true, 3), Access::Unknown);
        for state in [
            Access::Unsupported,
            Access::MissingEntitlement,
            Access::Authorized,
            Access::Denied,
            Access::Request,
            Access::Unknown,
            Access::TimedOut,
        ] {
            assert!(!state.message().is_empty());
        }
    }
}
