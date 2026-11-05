use crate::browser::{allowed_url, authenticated_command};

#[test]
fn inline_navigation_allows_only_the_supplied_html() {
    let expected = crate::browser::html_data_url("<h1>Trusted overlay</h1>");
    assert!(crate::browser::inline_navigation("about:blank", &expected));
    assert_eq!(
        crate::browser::inline_navigation(&expected, &expected),
        cfg!(target_os = "windows")
    );
    for url in [
        crate::browser::html_data_url("<script>window.ipc.postMessage('close')</script>"),
        "https://example.com".into(),
        "file:///C:/Windows/win.ini".into(),
        "javascript:alert(1)".into(),
    ] {
        assert!(!crate::browser::inline_navigation(&url, &expected));
    }
    // A permitted internal payload must not make data URLs public navigation.
    assert!(!allowed_url(&expected));
}

#[test]
fn bridge_requires_nonce_and_fixed_command() {
    assert_eq!(
        authenticated_command("secret:address", "secret"),
        Some("address")
    );
    assert_eq!(
        authenticated_command("secret:focus", "secret"),
        Some("focus")
    );
    for message in [
        "address",
        "wrong:address",
        "secret:run-code",
        "secret:reload:extra",
        "secret:",
    ] {
        assert_eq!(authenticated_command(message, "secret"), None);
    }
}

#[test]
fn navigation_never_allows_local_files_or_executable_schemes() {
    for url in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "data:text/html,a",
        "httpsx://test",
        "about:config",
    ] {
        assert!(!allowed_url(url), "allowed {url}");
    }
    for url in [
        "https://example.com",
        "http://localhost:3000",
        "about:blank",
        "HTTPS://example.com",
    ] {
        assert!(allowed_url(url), "denied {url}");
    }
}
