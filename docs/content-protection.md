# Built-in content protection

Protection is enabled by default on macOS and Windows. Settings contains a global
**Block ads and trackers** switch and **Update now**. Each panel's floating bar
has a shield for the active website. Its tooltip reports status and hostname;
clicking toggles an exact-hostname exception and reloads that hostname's pages
after native rules are ready. A filled shield means on, a slashed shield means
off, and a dotted indicator means the change is being applied. A disabled global switch disables the site control. The shield
is unavailable on the built-in start page. Exceptions do not automatically apply
to sibling hosts or subdomains.

The global setting and hostname exceptions are saved to `app.sqlite3` in the normal
app-data directory, together with other durable state. Downloaded filter lists
remain in `filter-lists.json` outside the database. Write failures are reported, and the in-memory choice
still applies. The native views remain alive across protection changes, retaining
back/forward history. A policy change reloads only open pages whose effective protection changed,
including hidden pages on the affected hostname; unsent form state on those
pages can be lost. Unrelated websites keep their loaded documents and state. Automatic filter updates replace native
network rules without reloading documents. Updated cosmetics apply on the next
normal navigation or reload.

## Lists and updates

The application bundles complete EasyList and EasyPrivacy snapshots from
2026-11-05, with upstream version headers, attribution and CC BY-SA 3.0 license
in `crates/desktop/assets/filters/`. Downloads use the same HTTPS subscriptions:

- https://easylist.to/easylist/easylist.txt
- https://easylist.to/easylist/easyprivacy.txt

While Axolotl runs, it checks when the last successful update is at least 24
hours old. Failed automatic attempts retry after 24 hours; **Update now** retries
immediately. Downloading and parsing run on a worker thread, with a 45-second
request timeout and an 8 MiB limit per list. Both lists must validate and parse,
and WebKit must compile the candidate successfully, before the combined cache
is saved atomically to `filter-lists.json`. Failed downloads, invalid lists,
native compilation failures and cache write failures retain the working rules.
An invalid/missing cache falls back to the bundled lists. This is an in-process
update check; the browser must be running. Smoke tests disable automatic updates
for deterministic execution; Update now remains available.

## Engines and supported filtering

[adblock-rust](https://github.com/brave/adblock-rust/) parses the network lists.
On macOS its content-blocking converter produces native
[WKContentRuleList](https://developer.apple.com/documentation/webkit/wkcontentrulelist)
rules. The initial page navigation waits for native compilation (several seconds
for the full lists). A main-document domain exception is appended last, so a
site bypass also permits its third-party requests. A disabled global switch uses
an allow-all native list. Platform callbacks enqueue shell events rather than
changing GPUI state reentrantly.

On Windows,
[WebResourceRequested](https://learn.microsoft.com/en-us/microsoft-edge/webview2/how-to/webresourcerequested)
filters intercept requests in WebView2 and return a local 403 for matched ads or
trackers. The adapter distinguishes main-frame and iframe documents, uses each resource's
type and method, and the current main-document URL for policy/party matching. The newest runtime API also covers
worker/service-worker request sources when available; older runtimes fall back
to the original request filter. Document-start script registration completes
before the initial target navigation or policy-change reload.

Both platforms inject plain CSS hiding rules at document start. Positive and
negative hostname scopes and exact selector exceptions are respected. The style
is mounted when the document root appears and restored if removed; CSS handles
new matching DOM elements without repeated scans. Cross-origin frames use
`ancestorOrigins` where supported, falling back to their referrer for policy.

This is a practical subset, not uBlock Origin Lite parity. Unsupported features
include arbitrary downloaded scriptlets, procedural/extended cosmetics, entity
cosmetic scopes, cosmetic action rules, `$generichide` exceptions, redirect
resource replacements, CSP modification and URL-parameter rewriting. WebKit
conversion also omits network rules that cannot be represented in its content
rule syntax, and its first-party definition differs from adblock-rust. Windows
currently uses the main document rather than each subframe's initiator as the
network matching context. Engine coverage therefore differs. Existing service
worker caches and engine-owned traffic are not guaranteed to be intercepted.

## YouTube: acceptance remains open

The bundled YouTube-specific document-start script removes known ad fields from
initial and subsequent player responses, without removing streaming data or
playability status. It also clicks visible skip buttons and advances a finite
ad video only while the player explicitly reports `ad-showing`. It does not
force normal playback, change mute/rate, or seek live/infinite streams. It is
local audited code; filter updates never execute downloaded JavaScript.

YouTube changes delivery and anti-blocking behavior frequently. Script unit
coverage and a no-ad page visit do **not** establish successful live video ad
blocking. Real preroll/midroll ads, uninterrupted content playback, seeking,
playlist/SPA transitions, signed-in/out behavior and both engines must still be
verified. Server-side inserted ads are not reliably distinguishable. The task
must not be considered complete until the required YouTube behavior is proven.

## Validation commands

```sh
node --test scripts/test-input-bridge.cjs scripts/test-notifications.cjs scripts/test-protection.cjs
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-protection
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-settings
# Also exercises real subscription downloads and a refresh without document reload.
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-protection --smoke-update
# Records live YouTube diagnostics; deliberately cannot certify ad-block acceptance.
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-protection --smoke-youtube
```

The native protection fixture counts requests on a loopback HTTP server. It
checks that a list-matched script never reaches that server, ordinary content
remains visible, a maintained cosmetic selector is hidden, and the shipped
shield's IPC permits/re-blocks the request after site toggles. It runs without
external sites. Programmatic native/DOM interactions are
not physical OS input tests. `--smoke-youtube` additionally attempts a native
AppKit play-button click on macOS and records player state.

## Results on 2026-11-05

- macOS: 54 Rust tests and 14 JavaScript tests passed. Formatting, whitespace
  checks and Clippy with warnings denied passed.
- The programmatic native fixture passed real request suppression, ordinary
  content, cosmetic hiding, the shipped per-site shield, global Settings toggle,
  and re-enabling. The real **Update now** download/parse/native-compile path
  passed, and a page marker survived the update without a reload.
- The existing native Settings smoke test passed, including all five keyboard
  focus targets, theme changes, homepage/search preferences and modal dismissal.
- YouTube: after a native AppKit play click, the requested music video advanced
  beyond 21 seconds with readyState 4 and no media error while protection was on.
  The diagnostic count of removed ad entries was zero. The unprotected comparison
  also showed no ad. This establishes working playback for that visit, **not**
  proven ad suppression; required live-ad acceptance remains unmet.
- Windows: the actual native adapter was type-checked against Wry 0.55.1 and
  WebView2 COM 0.38.2 in a focused cross-target harness. It was not run on Windows.
  The full cross-build on this Mac stopped in the `ring` dependency because the
  Windows C SDK (`assert.h`) was unavailable. The harness is not a substitute for
  the full Windows build, native controls, worker/frame filtering and YouTube
  acceptance checks on a Windows machine.
