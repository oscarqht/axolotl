//! OS-owned default selection. Registration never writes a user's choice.
use std::{
    collections::VecDeque,
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};

type Completion = Receiver<Result<(), String>>;

pub struct Integration {
    pub state: serde_json::Value,
    last_check: Instant,
    pending: Option<Completion>,
    remaining: VecDeque<&'static str>,
    requested_at: Instant,
    message: String,
    error: String,
}

impl Integration {
    pub fn new() -> Self {
        let mut this = Self {
            state: serde_json::Value::Null,
            last_check: Instant::now(),
            pending: None,
            remaining: VecDeque::new(),
            requested_at: Instant::now(),
            message: String::new(),
            error: String::new(),
        };
        this.refresh();
        this
    }

    pub fn refresh(&mut self) {
        self.last_check = Instant::now();
        let (http, https, query_error) = match platform::status() {
            Ok((http, https)) => (Some(http), Some(https), String::new()),
            Err(error) => (None, None, error.to_string()),
        };
        // A later manual selection supersedes an earlier cancellation or error.
        // Keep the waiting message until an in-flight OS request completes.
        if http == Some(true) && https == Some(true) && self.pending.is_none() {
            self.error.clear();
            self.message.clear();
        }
        self.state = serde_json::json!({
            "supported": cfg!(any(target_os="macos", target_os="windows")),
            "http": http, "https": https,
            "isDefault": http.zip(https).map(|(a,b)| a && b),
            "pending": self.pending.is_some(),
            "message": self.message,
            "error": if self.error.is_empty() { &query_error } else { &self.error },
        });
    }

    pub fn make_default(&mut self) {
        if self.pending.is_some() {
            return;
        }
        self.error.clear();
        self.message.clear();
        self.remaining = ["https", "http"].into();
        self.request_next();
        self.refresh();
    }

    fn request_next(&mut self) {
        #[cfg(target_os = "macos")]
        while self.remaining.front().is_some_and(|scheme| {
            platform::status().is_ok_and(|(http, https)| match *scheme {
                "http" => http,
                _ => https,
            })
        }) {
            // macOS may assign both protocols after the first confirmation.
            // Query again after each completion instead of prompting twice.
            self.remaining.pop_front();
        }
        let Some(scheme) = self.remaining.pop_front() else {
            self.message = platform::instructions().into();
            return;
        };
        match platform::request(scheme) {
            Ok(Some(receiver)) => {
                self.pending = Some(receiver);
                self.requested_at = Instant::now();
                self.message = "Complete the macOS confirmation. If cancelled, choose Make default to try again.".into();
            }
            Ok(None) => {
                self.remaining.clear();
                self.message = platform::instructions().into();
            }
            Err(error) => {
                self.remaining.clear();
                self.message.clear();
                self.error = format!("{error:#}");
            }
        }
    }

    pub fn poll(&mut self, visible: bool) {
        if let Some(receiver) = &self.pending {
            let result = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("The OS did not complete the request. Open system default-app settings and select Axolotl, or try again.".into())),
                Err(_) if self.requested_at.elapsed() > Duration::from_secs(120) => Some(Err("The OS confirmation is still open. Complete or dismiss it, then try again.".into())),
                Err(_) => None,
            };
            if let Some(result) = result {
                self.pending = None;
                match result {
                    Ok(()) if !self.remaining.is_empty() => self.request_next(),
                    Ok(()) => self.message = platform::instructions().into(),
                    Err(error) => {
                        self.error = error;
                        self.message.clear();
                        self.remaining.clear();
                    }
                }
                self.refresh();
            }
        }
        if visible && self.last_check.elapsed() >= Duration::from_secs(2) {
            self.refresh();
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Completion;
    use anyhow::{Context, Result, bail};
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSBundle, NSError, NSString, NSURL};

    fn bundle() -> Result<objc2::rc::Retained<NSBundle>> {
        let bundle = NSBundle::mainBundle();
        if bundle.bundleIdentifier().is_none() || !bundle.bundlePath().to_string().ends_with(".app")
        {
            bail!(
                "Run the packaged Axolotl.app from a permanent location such as Applications, then try again. Build it with scripts/package-macos.sh."
            );
        }
        Ok(bundle)
    }

    pub fn status() -> Result<(bool, bool)> {
        let bundle = bundle()?;
        let identifier = bundle
            .bundleIdentifier()
            .context("Missing bundle identifier")?;
        let owns = |scheme: &str| -> Result<bool> {
            let url = NSURL::URLWithString(&NSString::from_str(&format!("{scheme}://example.com")))
                .unwrap();
            let handler = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&url)
                .context("macOS could not resolve the default browser. Check System Settings → Desktop & Dock → Default web browser.")?;
            Ok(NSBundle::bundleWithURL(&handler)
                .and_then(|b| b.bundleIdentifier())
                .is_some_and(|id| id == identifier))
        };
        Ok((owns("http")?, owns("https")?))
    }

    pub fn request(scheme: &str) -> Result<Option<Completion>> {
        let bundle = bundle()?;
        if !objc2::available!(macos = 12.0) {
            bail!("Choose Axolotl in System Preferences → General → Default web browser.");
        }
        // LaunchServices records temporary bundles but excludes them from
        // usable handlers. Setting one can otherwise persist an unusable choice
        // while the API reports success. Require this exact copy to be listed.
        let own_url = bundle.bundleURL().URLByResolvingSymlinksInPath();
        let web_url =
            NSURL::URLWithString(&NSString::from_str(&format!("{scheme}://example.com"))).unwrap();
        let listed = NSWorkspace::sharedWorkspace()
            .URLsForApplicationsToOpenURL(&web_url)
            .iter()
            .any(|candidate| {
                own_url.is_some() && candidate.URLByResolvingSymlinksInPath() == own_url
            });
        if !listed {
            bail!(
                "macOS does not list this copy of Axolotl as a browser. Move Axolotl.app to Applications, reopen it, and try again. Rebuild with scripts/package-macos.sh if needed."
            );
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let callback = block2::RcBlock::new(move |error: *mut NSError| {
            let result = if error.is_null() {
                Ok(())
            } else {
                Err(format!(
                    "macOS did not change the default browser: {}. Try again, or select Axolotl in System Settings → Desktop & Dock → Default web browser.",
                    unsafe { &*error }.localizedDescription()
                ))
            };
            let _ = sender.send(result);
        });
        NSWorkspace::sharedWorkspace()
            .setDefaultApplicationAtURL_toOpenURLsWithScheme_completionHandler(
                &bundle.bundleURL(),
                &NSString::from_str(scheme),
                Some(&callback),
            );
        Ok(Some(receiver))
    }
    pub fn instructions() -> &'static str {
        "Choose Axolotl as the default web browser in System Settings."
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::Completion;
    use anyhow::{Context, Result};
    use windows::{
        Win32::{
            System::Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize,
            },
            UI::Shell::{
                AL_EFFECTIVE, AT_URLPROTOCOL, ApplicationAssociationRegistration,
                IApplicationAssociationRegistration, ShellExecuteW,
            },
            UI::WindowsAndMessaging::SW_SHOWNORMAL,
        },
        core::{PCWSTR, w},
    };
    const PROG_ID: &str = "Axolotl.HTTP";

    pub fn register() -> Result<()> {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let exe = std::env::current_exe()?;
        let exe = exe
            .to_str()
            .context("Move Axolotl to a path that can be represented as Unicode, then try again")?;
        let command = format!("\"{exe}\" --open-url \"%1\"");
        let user = RegKey::predef(HKEY_CURRENT_USER);
        let write = |path: &str, name: &str, value: &str| -> Result<()> {
            user.create_subkey(path)?.0.set_value(name, &value)?;
            Ok(())
        };
        let class = format!("Software\\Classes\\{PROG_ID}");
        write(&class, "", "Axolotl web link")?;
        write(&class, "URL Protocol", "")?;
        write(
            &format!("{class}\\DefaultIcon"),
            "",
            &format!("\"{exe}\",0"),
        )?;
        write(&format!("{class}\\shell\\open\\command"), "", &command)?;
        let client = "Software\\Clients\\StartMenuInternet\\Axolotl";
        write(client, "", "Axolotl")?;
        write(
            &format!("{client}\\DefaultIcon"),
            "",
            &format!("\"{exe}\",0"),
        )?;
        write(
            &format!("{client}\\shell\\open\\command"),
            "",
            &format!("\"{exe}\""),
        )?;
        let capabilities = format!("{client}\\Capabilities");
        write(&capabilities, "ApplicationName", "Axolotl")?;
        write(
            &capabilities,
            "ApplicationDescription",
            "Browse the web in Axolotl spaces and panels",
        )?;
        for scheme in ["http", "https"] {
            write(&format!("{capabilities}\\URLAssociations"), scheme, PROG_ID)?;
        }
        write("Software\\RegisteredApplications", "Axolotl", &capabilities)?;
        unsafe {
            windows::Win32::UI::Shell::SHChangeNotify(
                windows::Win32::UI::Shell::SHCNE_ASSOCCHANGED,
                windows::Win32::UI::Shell::SHCNF_IDLIST,
                None,
                None,
            );
        }
        Ok(())
    }

    pub fn status() -> Result<(bool, bool)> {
        unsafe {
            // GPUI normally owns COM initialization; balance only our successful call.
            let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
            struct ComGuard(bool);
            impl Drop for ComGuard {
                fn drop(&mut self) {
                    if self.0 {
                        unsafe { CoUninitialize() };
                    }
                }
            }
            let _guard = ComGuard(initialized);
            let registration: IApplicationAssociationRegistration = CoCreateInstance(
                &ApplicationAssociationRegistration,
                None,
                CLSCTX_INPROC_SERVER,
            )?;
            let owns = |scheme: PCWSTR| -> Result<bool> {
                let value =
                    registration.QueryCurrentDefault(scheme, AT_URLPROTOCOL, AL_EFFECTIVE)?;
                let result = value.to_string();
                CoTaskMemFree(Some(value.0.cast()));
                Ok(result?.eq_ignore_ascii_case(PROG_ID))
            };
            Ok((owns(w!("http"))?, owns(w!("https"))?))
        }
    }
    pub fn request(_: &str) -> Result<Option<Completion>> {
        register().context("Could not register Axolotl for this Windows user. Keep the executable in a permanent writable location and try again")?;
        unsafe {
            let result = ShellExecuteW(
                None,
                w!("open"),
                w!("ms-settings:defaultapps?registeredAppUser=Axolotl"),
                None,
                None,
                SW_SHOWNORMAL,
            );
            anyhow::ensure!(
                result.0 as isize > 32,
                "Windows could not open Default apps. Open Settings → Apps → Default apps, select Axolotl, and assign HTTP and HTTPS."
            );
        }
        Ok(None)
    }
    pub fn instructions() -> &'static str {
        "In Windows Settings → Apps → Default apps, select Axolotl and choose Set default (or assign both HTTP and HTTPS). Return here to check the result."
    }
}

#[cfg(target_os = "windows")]
pub use platform::register;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use super::Completion;
    pub fn status() -> anyhow::Result<(bool, bool)> {
        anyhow::bail!("Default-browser integration is supported on macOS and Windows.")
    }
    pub fn request(_: &str) -> anyhow::Result<Option<Completion>> {
        anyhow::bail!("Default-browser integration is supported on macOS and Windows.")
    }
    pub fn instructions() -> &'static str {
        ""
    }
}
