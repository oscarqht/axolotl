# Settings

Open **Settings** using the gear at the right of the spaces header,
**Cmd+,** on macOS, **Ctrl+,** on Windows, or **Axolotl → Settings…** in the
macOS app menu. The shortcut is also handled while native pages or floating
panel controls have keyboard focus.

The **Search engine** dropdown offers **Google** (the default), **DuckDuckGo**,
and **Bing**. Changes apply immediately to future address-bar searches; there
is no Save/Cancel step. Existing pages are not reloaded, and explicit URLs,
hostnames, localhost addresses, and `about:blank` retain their normal behavior.

Settings opens in a centered, theme-matched modal with three tabs: **General**
(search engine, homepage, default browser), **Privacy** (ad and tracker blocking,
browsing history), and **Tabs & media** (automatic sleep).
Use Left/Right arrows or Home/End on the tab bar to switch categories. Only the
selected category participates in keyboard navigation. The modal focuses the
search dropdown on opening, keeps Tab/Shift+Tab
inside it, and dismisses with Escape or its labeled close button. Dismissal
restores focus to the invoking header control or page. Background app commands
are blocked while the modal is open.

## Browsing history

Both address inputs suggest up to eight unique recent URLs when the typed text
matches a stored title or URL (case-insensitive literal substring). Each result
shows its title and URL. Use Up/Down and Enter or click a result to open it in the
current tab. Escape dismisses suggestions; Enter with no selected result keeps
normal URL/search submission.

History is shared across spaces and stored locally in `app.sqlite3` with the
other durable app state. Completed HTTP/HTTPS main-frame visits are recorded, including
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

Preferences, workspace snapshots, window geometry, sleep metadata, protection
preferences, and browsing history share `app.sqlite3` in the app's data directory:

- macOS: `~/Library/Application Support/Axolotl/`
- Windows: `%APPDATA%\Axolotl\`
- `AXOLOTL_DATA_DIR` overrides the directory for isolated development runs.

The persisted `search_engine` identifiers are `google`, `duckduckgo`, and `bing`.
Missing fields and unknown preference values fall back to defaults. Invalid legacy
files or corrupt database records are retained and persistence is disabled for the
run, with an error in the browser status. A failed background save keeps the current
choice in memory, reports an error, and retains the batch for retry and recovery.
Smoke runs neither read nor write the user's session or settings. See
[app persistence](persistence.md) for migration, batching, and recovery details.

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
peer connections, connecting/open WebSocket and EventSource connections, and
trusted form edits. Submitting a form does not clear edit protection: a prevented,
asynchronous, or failed submission may still contain unsaved work. Cross-origin or uninspectable frames
are conservatively exempt. Custom editors and page overrides may escape these
best-effort checks; manual sleep remains available.

On macOS 14 and newer, loaded content views disable WebKit's independent inactive
scheduling. Keep Alive therefore prevents Axolotl unloading while WebKit is also
allowed to keep background work running. Ordinary idle tabs can still be unloaded
by Axolotl's automatic sleep. Retaining live webapps may use more energy/memory.
This does not keep the computer awake or guarantee network connectivity during
system sleep; webapps still need their own reconnection handling after wake.

For a page that remains stuck after Refresh, the tab context menu offers
**Recover Tab…**. Its native warning explains that recreation loses unsaved
changes and Back/Forward history. Cancel preserves the current page; Recreate
Tab opens its saved URL with the same cookies and persistent site data. Recovery
is manual and unavailable during active downloads or a linked sign-in popup.
See [webapp reliability investigation and validation](webapp-stability.md).

Sleep captures a small PNG before dropping the native page. A faded snapshot
(or a title placeholder if capture fails) stays in the pane until clicked.
Selecting/focusing the tab recreates it at its saved URL; unsaved live state
may be lost. Returning to a space wakes only its visible tabs. Sleeping tab
titles show 💤 and exactly 50% text opacity. Hover previews use the cached
images without creating or waking page views; space previews include sleeping
visible panes.

Sleep/Keep Alive metadata lives in `app.sqlite3`, with local `sleep/<tab-id>.png`
caches outside the database. Waking or closing tabs removes their
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

## Default browser and external links

**Default browser** reports the OS selection for both HTTP and HTTPS. A partial
selection is shown when Axolotl owns only one protocol. **Make default** starts
the OS-approved selection flow; it never silently overrides the user's choice.
Status refreshes while Settings is open and when the window becomes active.
There is no startup prompt. Errors include instructions for retrying or choosing
Axolotl in the system's default-app settings.
macOS refuses Make default when the running copy is not listed by LaunchServices
(for example, a bundle inside the temporary directory); move it to Applications
and reopen it before retrying. Only the observed OS association is called default.

On macOS, run the packaged `Axolotl.app` from a permanent location (usually
Applications). `scripts/package-macos.sh` declares HTTP/HTTPS URL types and
registers the bundle with LaunchServices. A bare `cargo run` executable cannot
register as a browser. Make default uses `NSWorkspace`'s asynchronous default
application request on macOS 12 and later; older versions provide manual System
Preferences instructions. Keep a stable bundle identifier when distributing
updates (`AXOLOTL_BUNDLE_ID` remains supported).

On Windows, keep the executable in a permanent location and run it once.
Axolotl registers its ProgID, quoted URL command, browser client, HTTP/HTTPS
capabilities, and RegisteredApplications entry under **HKEY_CURRENT_USER**.
No administrator rights or installer are required. Make default repairs these
entries and opens `ms-settings:defaultapps?registeredAppUser=Axolotl`.
Choose **Set default**, or assign both HTTP and HTTPS, then return to Axolotl.
The effective association is queried through `QueryCurrentDefault`; registration
alone does not count as being the default. No `UserChoice` values are written.
Moving the executable requires running it again to update its registered path.

OS-opened HTTP/HTTPS links and absolute HTTP/HTTPS command-line arguments open
as new tabs in the focused panel of the active space. Multiple links retain
request order and the last tab becomes active. Existing tabs, other panels,
spaces, pins, and restored sessions are preserved. Incoming links dismiss app
popovers and Settings, then activate the browser window. Files, search terms,
and executable/custom schemes are ignored at this external entry point.

Each data directory has one process owner. Additional executable launches pass
URLs to that owner over an authenticated loopback connection and exit after
acknowledgement. Retried deliveries carry the same request ID and open once;
separate user actions on the same URL still open separate tabs. A file lock
prevents concurrent session writers and is released after exit or a crash.
macOS Apple events are queued before launch finishes and consumed after session
restoration. An app using a different `AXOLOTL_DATA_DIR` has an isolated instance.

### Native acceptance checks

```sh
cargo test --workspace --locked
cargo build -p axolotl --features smoke --locked
python3 scripts/test-external-urls.py target/debug/axolotl
cargo run -p axolotl --features smoke --locked -- --smoke-test --smoke-settings
```

The process check uses a temporary profile with two spaces and a split panel.
It verifies cold/running executable launches, multiple URLs, separate repeated
requests, unsafe scheme rejection, focused-panel routing, foreground activation,
and session preservation on restart. Unit tests also retry an identical delivery
ID and recover an abandoned endpoint. Passing a packaged **smoke-build** `.app`
to the Python check additionally exercises cold and running macOS Apple events.
This includes a forced second bundle launch, which forwards its startup Apple
events through a temporary windowless event loop rather than restoring a second
session. Empty secondary executable launches exit after the startup event grace
period. The native check also minimizes the owner before sending running URLs.
The Settings smoke check exercises status delivery, modal focus, and the bare
macOS executable's actionable Make default error without changing OS defaults.

On each native OS, also verify the user-consent flow manually: select Make
default, cancel and retry, complete the OS selection, return to Settings, and
confirm both protocols. Change either association in system settings and check
that Axolotl reports the partial state. Test a link clicked in another app with
Axolotl stopped, running, and minimized. Windows checks require a Windows desktop
with WebView2 and the Windows SDK to build; cross-compilation alone does not
verify registry registration, Default apps UI, or foreground behavior.

API references: [Apple NSWorkspace default application request](https://developer.apple.com/documentation/appkit/nsworkspace/setdefaultapplication(at:toopenurlswithscheme:completion:)),
[Windows Default Programs registration](https://learn.microsoft.com/en-us/windows/win32/shell/default-programs),
and [launching Windows Default Apps settings](https://learn.microsoft.com/en-us/windows/uwp/launch-resume/launch-default-apps-settings).
