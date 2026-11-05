# macOS website compatibility

Axolotl retains WKWebView. Its default macOS user-agent omits Safari's
`Version/… Safari/…` tokens, so some websites classify a current WebKit engine
as an unknown or old browser. This is documented in
[WebKit issue 278284](https://bugs.webkit.org/show_bug.cgi?id=278284).

Page views now use a Safari-compatible native user-agent before the first
request. The Safari marketing version comes from the installed `com.apple.Safari`
application's `CFBundleShortVersionString`, located with public macOS APIs.
The frozen OS/WebKit tokens match Safari, including on Apple Silicon. The
Safari version is cached for the process; restart Axolotl after updating Safari.
If Safari cannot be located or its version is invalid, Axolotl logs a diagnostic
and retains WKWebView's default identity rather than guessing a version.
Windows retains WebView2's native identity.

This corrects identification; it does not add engine APIs or remove providers'
restrictions on embedded browser OAuth. Safari's version is a compatibility
proxy for system WebKit, not a guarantee that every Safari feature is available.

## Validation (2026-11-05)

On macOS 27.0.1, installed Safari 27.0.1 reported the following in both an HTTP
request to a local fixture and `navigator.userAgent`:

```text
Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.0.1 Safari/605.1.15
```

The Axolotl native build reported the same identity after the fix. A/B checks
changed only the user-agent, retaining WKWebView:

| Page | Default WKWebView identity | Safari-compatible identity |
| --- | --- | --- |
| Google homepage | Legacy layout, Advanced search, no button elements | Modern layout, AI Mode, normal buttons; visually inspected in the native app |
| Signed-in Google Calendar week view | Exact “This browser version is no longer supported” warning | Same signed-in week route, calendar controls present, warning absent from rendered page text |
| Google Sign in | User-reported older design | Following the homepage Sign in link reached `/v3/signin/identifier`, with Email or phone, Next, Forgot email and Create account controls |

The Calendar check used the same executable location and existing authenticated
WebKit session for both runs. A temporary executable had a separate session and
redirected to the public Calendar landing page; that result was not used as proof
of signed-in Calendar compatibility. No credentials, MFA or third-party OAuth
consent were submitted. Full fresh sign-in and provider-specific OAuth flows
remain manual checks. Calendar event contents are excluded from this report.

## Regression check

The focused native regression needs a macOS GUI session and installed Safari,
but uses only a loopback HTTP fixture and no Google account or external network:

```sh
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-browser-identity
```

It compares the native HTTP header with JavaScript identity in the first
document, redirected pages and frames, then exercises navigation, back/forward,
reload and another tab. Unit tests cover Safari version formatting and invalid
metadata. The ordinary native smoke suite continues to cover popup handling and
other browser commands.

Validation results: 53 Rust tests, 10 JavaScript checks, the focused native
identity regression, formatting and Clippy (`-D warnings`) passed. The broader
native smoke suite did not pass: the final fixed binary timed out evaluating the
inline address bar's input state; the pre-fix binary failed with `Settings
evaluation timed out`. These are separate from the focused compatibility checks;
this change does not claim a clean full native smoke run.
