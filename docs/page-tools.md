# Page zoom and find

On macOS use Cmd; on Windows use Ctrl. Plus (including Shift+Equals) or Equals
zooms in, Minus zooms out, and 0 resets to 100%. Zoom uses browser-style steps
from 25% to 500%. Only page content is zoomed, not application chrome.

Zoom is saved independently of session/settings snapshots in the `site_zoom`
record of the existing `app.sqlite3` store. No schema migration is needed.
HTTP/HTTPS and different ports share an exact URL hostname; subdomains have
independent settings. Existing engines on that hostname update together without
reloading or replacing form state. Navigation, restored tabs and new native
views receive the saved level. Reset removes that hostname's override. Internal documents without a hostname
keep a temporary per-view level, reset on navigation rather than saved.

Cmd/Ctrl+F opens a small, page-local field. Search is case-insensitive, literal
text (not a regular expression). It counts visible text matches, supports inline
text spanning elements, highlights matches, scrolls to the active match, and
wraps next/previous navigation. Use the arrow controls or Enter/Shift+Enter;
Escape or the close button removes highlights and restores the previous element
focus. Reopening selects the retained query. Queries are not persisted.
Live DOM changes refresh results while the field is open. Native shortcuts are
reserved before webpage handlers; Enter in the field and Escape are consumed
before existing pin/preview dismissal.

Native page focus takes priority over the model's last-focused pane. With chrome
focus, tools target the focused pane or an open pin/preview. Floating panes use
the same native page path. Settings and browser-only surfaces are not targets.

## Boundaries

Find currently searches visible text in the main document, not iframe contents,
shadow-root contents, form-control values, PDF/canvas text, or virtualized content
that is not in the DOM. Older engines without the CSS Highlight API show the
active match using selection rather than highlighting every match. Constructed
stylesheets support modern sites with restrictive inline-style CSPs.

## Verification

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
node --test scripts/test-*.cjs
cargo build -p axolotl --features smoke --locked
# macOS: isolated in-memory profile, queued AppKit events (not physical input)
target/debug/axolotl --smoke-test --smoke-page-tools
```

Unit tests cover hostname isolation, scheme/port sharing, zoom steps/bounds/reset,
SQLite close/reopen persistence, and preserving unrelated settings. The page
bridge tests cover both platform modifiers, synthetic-input rejection and Escape.
The macOS native scenario covers page and chrome focus, split/hostname targeting,
new tabs and navigation, zoom keys/reset, find counts/highlights, literal and
cross-element text, wraparound, Enter/Shift+Enter, strict style CSP, interception
of page key handlers, cleanup, pins, previews and floating pages.

Windows reserves Ctrl shortcuts using WebView2 accelerator callbacks and queues
commands rather than updating the UI reentrantly. Native Windows validation must
run on Windows. Cross-checking from the development Mac is blocked in the `ring`
dependency by missing Windows C SDK headers (`assert.h`), before app type checking.

## Local results (2026-11-06)

- macOS all-target/all-feature check, strict Clippy, smoke build, formatting and
  whitespace checks passed.
- Rust workspace tests: 128 passed; one existing manual benchmark ignored.
- JavaScript regression scripts: 31 passed.
- Native queued-AppKit page-tools and space-shortcuts scenarios passed.
- The broader native keyboard scenario fails at “Cmd+, no longer opens settings”.
  An independently built, unchanged HEAD snapshot fails at the identical
  assertion, confirming the failure predates these changes. No unrelated
  settings/focus behavior was changed.
- Windows cross-check blocked as described above; no Windows runtime or physical
  keyboard/mouse validation was performed.
