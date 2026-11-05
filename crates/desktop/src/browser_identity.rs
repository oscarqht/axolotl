//! WKWebView omits Safari's version tokens on macOS. Google and other sites
//! consequently serve a legacy UI or report a current engine as unsupported.
use std::sync::OnceLock;

/// Safari and WKWebView use the system WebKit. Read the installed Safari's
/// marketing version through public APIs; the WebKit build number and macOS
/// version are not Safari versions. Never claim a made-up version on failure.
pub(crate) fn user_agent() -> Option<&'static str> {
    static AGENT: OnceLock<Option<String>> = OnceLock::new();
    AGENT
        .get_or_init(|| {
            let agent = installed_safari_version().and_then(|version| safari_user_agent(&version));
            if agent.is_none() {
                eprintln!(
                    "Axolotl: Safari version unavailable; retaining the native WebKit user-agent"
                );
            }
            agent
        })
        .as_deref()
}

fn installed_safari_version() -> Option<String> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSBundle, NSString};

    let url = NSWorkspace::sharedWorkspace()
        .URLForApplicationWithBundleIdentifier(&NSString::from_str("com.apple.Safari"))?;
    let bundle = NSBundle::bundleWithURL(&url)?;
    let version = bundle
        .objectForInfoDictionaryKey(&NSString::from_str("CFBundleShortVersionString"))?
        .downcast::<NSString>()
        .ok()?;
    Some(version.to_string())
}

fn safari_user_agent(version: &str) -> Option<String> {
    // Reject malformed bundle metadata rather than injecting it into a header.
    let parts: Vec<_> = version.split('.').collect();
    if !(2..=3).contains(&parts.len())
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
        || parts[0].parse::<u32>().ok()? == 0
    {
        return None;
    }
    // Safari deliberately freezes the OS and AppleWebKit/Safari build tokens,
    // including on Apple Silicon. Only Version/ tracks its marketing version.
    Some(format!(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/{version} Safari/605.1.15"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_safari_marketing_version_without_os_or_engine_version_guessing() {
        for version in ["17.6", "18.6.1", "26.0", "27.0.1"] {
            let agent = safari_user_agent(version).unwrap();
            assert!(agent.ends_with(&format!("Version/{version} Safari/605.1.15")));
            assert!(agent.contains("Mac OS X 10_15_7"));
        }
    }

    #[test]
    fn rejects_missing_or_malformed_versions() {
        for version in [
            "",
            "27",
            "0.1",
            "27..1",
            "27.0\r\nInjected: x",
            "27.0 beta",
            "27.0.1.2",
        ] {
            assert!(safari_user_agent(version).is_none(), "{version:?}");
        }
    }
}
