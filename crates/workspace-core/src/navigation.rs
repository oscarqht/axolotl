use std::fmt;
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationError(pub String);

impl fmt::Display for NavigationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for NavigationError {}

/// Accept web URLs, localhost addresses, and search terms. Privileged schemes never reach a host.
pub fn normalize_url(input: &str) -> Result<String, NavigationError> {
    let input = input.trim();
    if input == "about:blank" {
        return Ok(input.to_owned());
    }
    if input.is_empty() {
        return Err(NavigationError("Enter an address or search term".into()));
    }
    if input.chars().any(char::is_control) {
        return Err(NavigationError(
            "Addresses cannot contain control characters".into(),
        ));
    }
    if input.starts_with("//") {
        return web_url(&format!("https:{input}"));
    }
    let first = input.split('/').next().unwrap_or(input);
    let local = first == "localhost"
        || first.starts_with("localhost:")
        || first.starts_with("127.0.0.1")
        || first.starts_with("[::1]");
    if local && !input.chars().any(char::is_whitespace) {
        return web_url(&format!("http://{input}"));
    }
    // A colon before a slash denotes an explicit scheme, except hostname:port.
    let hostname_port = first.rsplit_once(':').is_some_and(|(host, port)| {
        host.contains('.') && !port.is_empty() && port.chars().all(|c| c.is_ascii_digit())
    });
    if input.contains("://") || (first.contains(':') && !hostname_port) {
        return web_url(input);
    }
    if !input.chars().any(char::is_whitespace) && (first.contains('.') || hostname_port) {
        return web_url(&format!("https://{input}"));
    }
    let mut search = Url::parse("https://duckduckgo.com/").expect("static search URL");
    search.query_pairs_mut().append_pair("q", input);
    Ok(search.into())
}

fn web_url(input: &str) -> Result<String, NavigationError> {
    let url = Url::parse(input).map_err(|e| NavigationError(format!("Invalid address: {e}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(NavigationError(
            "Only HTTP, HTTPS, and about:blank are supported".into(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(NavigationError(
            "Addresses containing credentials are not supported".into(),
        ));
    }
    Ok(url.into())
}
