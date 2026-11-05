# Settings

Open **Settings** using the gear at the right of the spaces header,
**Cmd+,** on macOS, **Ctrl+,** on Windows, or **Axolotl → Settings…** in the
macOS app menu. The shortcut is also handled while native pages or floating
panel controls have keyboard focus.

The **Search engine** dropdown offers **Google** (the default), **DuckDuckGo**,
and **Bing**. Changes apply immediately to future address-bar searches; there
is no Save/Cancel step. Existing pages are not reloaded, and explicit URLs,
hostnames, localhost addresses, and `about:blank` retain their normal behavior.

The theme-matched modal focuses the dropdown on opening, keeps Tab/Shift+Tab
inside it, and dismisses with Escape or its labeled close button. Dismissal
restores focus to the invoking header control or page. Background app commands
are blocked while the modal is open.

## Browsing history

Both address inputs suggest up to eight unique recent URLs when the typed text
matches a stored title or URL (case-insensitive literal substring). Each result
shows its title and URL. Use Up/Down and Enter or click a result to open it in the
current tab. Escape dismisses suggestions; Enter with no selected result keeps
normal URL/search submission.

History is shared across spaces and stored locally in `history.sqlite3` beside
`settings.json`. Completed HTTP/HTTPS main-frame visits are recorded, including
reloads and Back/Forward visits. Internal pages, failed loads, subframes, and
redirect intermediates are excluded. Titles can update after a visit completes.
History persists across app restarts independently of session snapshots.

The default retention is **30 days**. Choose **Never automatically clear** or a
positive whole number of days in Settings. Expired visits are removed at startup,
once an hour while running, and immediately after changing retention. The
`history_retention_days` setting is a positive integer, or `null` for Never.

**Clear browsing history…** opens an inline confirmation. Confirming clears all
stored visits and suggestions; cancelling keeps them. Clearing preserves cookies,
login sessions, saved tabs, live pages, and native Back/Forward navigation.

## Persistence

The choice is written atomically to `settings.json`, separately from
`session.json`, in the app's data directory:

- macOS: `~/Library/Application Support/Axolotl/`
- Windows: `%APPDATA%\Axolotl\`
- `AXOLOTL_DATA_DIR` overrides the directory for isolated development runs.

The persisted `search_engine` identifiers are `google`, `duckduckgo`, and `bing`.
Missing/unreadable files, malformed JSON, missing fields, and unknown or invalid
values fall back to Google. If saving fails, the current choice still applies
in memory and the dialog displays a warning that it may not survive restart.
Smoke runs neither read nor write the user's session or settings.

## Verification

```sh
node --test scripts/test-input-bridge.cjs scripts/test-notifications.cjs
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-settings
```

The focused native check needs a GUI session but no network. It covers the
shipped dialog's labels/options, initial focus, forward/reverse Tab wrapping,
immediate changes, invalid IPC rejection, action dispatch, close/Escape, focus
restoration, background command blocking, and retained workspace/live page
state. On macOS it also checks live light/dark appearance updates. The same
check runs in the full native smoke suite. File round-trip/fallback and provider
URL encoding are covered by unit tests; shortcut authentication and modifiers
are checked against the shipped input bridge.

These programmatic checks do not synthesize OS keyboard/mouse input or verify
screen-reader announcements. Windows native execution and manual accessibility
checks must be performed on a suitable Windows desktop.

The browsing-history native acceptance flow uses a local HTTP fixture and an
isolated in-memory database (disk persistence is covered separately by unit tests):

```sh
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-history
```

It covers redirects/subframes/failed loads, repeat visits, late titles, both
suggestion inputs, keyboard and mouse selection handlers, retention changes and
periodic pruning, confirmed clearing, and retained cookies/page/navigation state.
Append `--smoke-hold-history` to pause at both suggestion surfaces for visual review.

## Tab and space sleep

The tab context menu offers **Sleep Tab** and a checked **Keep Alive** choice
(including pinned tabs). The space context menu offers **Sleep Space**. Keep
Alive protects against automatic sleep and Sleep Space; explicit Sleep Tab
still sleeps that tab once, preserving Keep Alive.

Settings enables automatic sleep by default after **30 whole minutes** hidden.
The enable checkbox and timeout (at least one whole minute) apply immediately.
All visible split panes stay awake. Tabs in inactive spaces are hidden. Turning
off automatic sleep does not wake manual sleepers. Pages are checked before
sleeping for playing media, live capture streams, active Web Audio contexts,
peer connections, and trusted form edits. Cross-origin or uninspectable frames
are conservatively exempt. Custom editors and page overrides may escape these
best-effort checks; manual sleep remains available.

Sleep captures a small PNG before dropping the native page. A faded snapshot
(or a title placeholder if capture fails) stays in the pane until clicked.
Selecting/focusing the tab recreates it at its saved URL; unsaved live state
may be lost. Returning to a space wakes only its visible tabs. Sleeping tab
titles show 💤 and exactly 50% text opacity. Hover previews use the cached
images without creating or waking page views; space previews include sleeping
visible panes.

Sleep/Keep Alive metadata lives in `sleep/state.json`, with local `sleep/<tab-id>.png`
caches alongside the session and settings. Waking or closing tabs removes their
cached images. Sleep state and previews survive restarts; restored visible
sleepers wait for user activation too.

Focused checks (use a GUI desktop for native tests):

```sh
node scripts/test-sleep-activity.cjs
cargo test --workspace --locked
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-sleep
python3 scripts/test-sleep-session.py target/debug/axolotl
```

The sleep smoke test ages hidden timestamps to exercise the automatic deadline
without a 30-minute wait. `--sleep-inspect` can be added to hold a sleeping pane
for an OS click (up to 60 seconds). The restart script supplies a temporary data
directory; do not run `--smoke-sleep-session` directly against a real profile.
macOS native checks do not establish Windows runtime behavior; run them again
on a Windows desktop using WebView2.
