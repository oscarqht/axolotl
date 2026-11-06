# Axolotl

A Rust + GPUI desktop browser integration prototype. A **space** contains a
recursive split layout; each **pane** has its own tab stack. Real pages run in
Wry native child views: WKWebView on macOS and WebView2 on Windows.

## Run

Install a recent stable Rust toolchain (development used Rust 1.91.1).

### macOS

Full Xcode and its command-line tools are required. The default development
configuration uses GPUI runtime shaders, so the separate Metal compiler download
is unnecessary. An unsigned development run is:

```sh
cargo run -p axolotl --locked
```

Build a local app bundle with an ad hoc signature, then open it:

```sh
scripts/package-macos.sh
open target/debug/Axolotl.app
```

For a single build-and-launch command, use:

```sh
scripts/package-macos.sh && open target/debug/Axolotl.app
```

Use `cargo run` on its own for a terminal development run. In
`cargo run -p axolotl --locked && open target/debug/Axolotl.app`, the `open`
command runs after you quit the Cargo-launched app successfully, launching the
app again. Cargo also does not refresh the executable inside an existing `.app`;
the packaging script rebuilds and copies that executable before signing it.

Use `scripts/package-macos.sh release` for an optimized build. This is a local
development bundle, without distribution signing, notarization, or an installer.

Website passkeys use WKWebView and the macOS credential provider. Choose
**Axolotl → Enable Passkeys…** to request browser access on macOS 13.3 or later.
This requires Apple-approved signing and provisioning; the default ad-hoc build
reports the missing entitlement. See [macOS passkey setup and testing](docs/passkeys-macos.md).

Website pages use a Safari-compatible identity based on the installed Safari
version to avoid unsupported-browser warnings and legacy site layouts. See
[macOS browser compatibility and validation](docs/browser-compatibility-macos.md).
Background webapp handling, Keep Alive, reload and explicit tab recovery are
covered in [webapp reliability](docs/webapp-stability.md).

On macOS, switching away from a tab or Axolotl space with a playing video
requests native WebKit Picture in Picture. Returning keeps PiP open; dismissing
it suppresses reopening until the tab is shown and hidden again. See
[native auto-PiP behavior and validation](docs/native-auto-pip.md).

### Windows (x64)

Install Rust stable with the MSVC toolchain, Visual Studio Build Tools with
**Desktop development with C++** and a Windows SDK, plus the
[WebView2 Evergreen Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/).
Build and run from PowerShell (the scripts also work outside a Visual Studio
developer terminal):

```powershell
.\scripts\build-windows.ps1
.\scripts\run-windows.ps1

# Optimized build and run
.\scripts\build-windows.ps1 -Profile release
.\scripts\run-windows.ps1 -Profile release

# Native integration check (opens a window and exits)
.\scripts\run-windows.ps1 -SmokeTest
```

The run script builds automatically when needed. Executables are written to
`target\debug\axolotl.exe` or `target\release\axolotl.exe` with Cargo's default
target directory. To launch an existing build directly, use
`.\target\debug\axolotl.exe`.

If PowerShell blocks local scripts, invoke them with a process-only override:
`powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-windows.ps1`
(use `run-windows.ps1` to run).

The scripts import the x64 Visual Studio developer environment and locate the
Windows SDK shader compiler (`fxc.exe`). To override shader compiler discovery,
set `GPUI_FXC_PATH` to its absolute path. Alternatively, run
`cargo build -p axolotl --locked` and `cargo run -p axolotl --locked` directly
from a configured Visual Studio developer terminal. The app sets
`GPUI_DISABLE_DIRECT_COMPOSITION=true` before initializing GPUI so WebView2 can
render its child windows. WebView2 Evergreen must also be installed on any Windows
machine running the executable. See the validation report below for tested scope.

## Use

- The native titlebar and separate navigation toolbar are hidden. On macOS,
  native traffic lights sit before the first space tab. On Windows, minimize,
  maximize/restore, and close buttons sit at the right of the spaces header.
  Drag the empty area to the right of the space tabs to move the window;
  double-click it to maximize/restore on Windows. Resize from the window edges.
- Space tabs show the focused page title, with an in-tab **×** and one trailing
  **+** or Cmd/Ctrl+Shift+T to create and activate a space at the configured
  homepage, with its address input focused and selected. Closing the final space is disabled. Double-click a
  space title to edit its focused page address. Drag a space title to reorder
  spaces; the insertion line shows the drop position. Hold near either edge of
  the strip to scroll through overflow. Drop inside the strip to save the new
  order across restarts. Escape or dropping outside cancels and retains the
  active space.
- Right-click a space tab and choose **Edit space** to set a fixed title and an
  optional emoji icon. Click the icon button to search emoji by name or clear the
  icon. **Save** or Enter saves both fields; Escape or clicking outside discards
  changes. A blank title follows the focused page title. Titles and icons survive
  restarts. Editing an inactive space keeps the active space selected.
- Each panel has a bottom-centered floating bar above its live page: a panel
  drag handle, active index/count (for example **2/5**), and **+**. Hover or focus
  the count/chip area, or tap the count, to expand it into favicon/title chips.
  Hovering the handle or **+** does not expand it. Each chip has
  a close button; double-click the label to edit its address. Enter navigates and
  Escape cancels. Clicking **+** or pressing Cmd/Ctrl+T creates a new tab and
  immediately focuses its address input, with the current address selected. Long stacks remain
  in one horizontal row, with overflow arrows and automatic active-chip reveal.
  The bar animates between compact and expanded states, fitting the handle,
  chips and add button. Its maximum is the panel width with 16px edge margins. Overflow fades indicate hidden chips on either side
  and clear at the corresponding scroll end. Reduced-motion settings are respected.
- Page JavaScript `new Notification(...)` adds an unread count while its tab
  is unfocused, including hidden tabs and spaces. Red badges with white numbers
  appear on tab chips; the collapsed bar shows the panel sum beside its tab
  index/count. Each space tab also shows the sum across all its panels, including
  hidden tabs. Selecting or focusing a tab clears its count. Counts are in memory
  and disappear on restart. Badges display `99+` above 99, with the exact count
  in the tooltip and accessible label.
  The page API supports badges without a system notification prompt; its
  `permission`/`requestPermission()` report permission for in-app badges. Native
  delivery is forwarded only when the engine already has permission and supports
  the constructor. Service-worker and push notifications are not included.
- Drag a chip to move one tab, or the handle to move the complete stack. Drop on
  a panel's bar or center to merge, or in its broad edge region to create a split.
  Only workspace-edge drops detach a new floating overlay. An existing overlay
  can be freely moved by its header or returned to a tiled split at a thin edge
  target. Green previews mean split, purple means dock, blue means move a float.
- Floating panels have a draggable title header and a resize triangle over the
  page's bottom-right corner. Their pages have square inner corners and no resize
  footer. Dock at any window edge without changing the tiled layout. Chips stay
  at the docking position, with offsets to avoid overlaps. Left/right chips are
  vertical. Hover reveals a fitted panel beside the chip; leaving hides it after
  a short grace period. **◇ / ◆** toggles keeping it open. Menus, address editing,
  resizing and dragging keep it open. One unpinned panel reveals per edge.
  Drag a chip onto another chip on that edge to reorder; drag away to float.
- Floating panels belong to their space. Tabs, preferred positions and sizes,
  edge anchors, chip order and pin state restore after restarting. Unpinned
  panels restore collapsed. Window resizing keeps panels and chips reachable.
  Moving retains native page state and back/forward history. Detaching the last
  tiled panel leaves a fresh start panel behind. Escape, loss of window activation,
  or releasing outside the workspace cancels a panel move.
- See [floating panel behavior and validation](docs/floating-panels.md) for persistence
  and native test details.
- Deliberate horizontal swipes across page content navigate that page's web
  history: swipe right to go back and left to go forward. Trackpad direction
  follows the system scrolling settings. At either end of history, swiping does
  nothing; it never switches tabs. Pages and nested areas scroll horizontally
  when they have room in the gesture direction. A fresh swipe at a scroll
  boundary navigates history only when no ancestor can scroll further that way.
  Scrolling touch drags and trackpad momentum stay with the page until the
  gesture ends (300 ms idle for wheel input). Vertical scrolling remains normal.
  History navigation fires once per gesture. Floating-bar swipes still select the next tab on a
  left swipe and the previous tab on a right swipe, wrapping at both ends;
  dragging a chip/handle takes priority over touch swiping.
- Closing the final tab in a panel removes that panel when other panels remain;
  the final panel instead receives a fresh start tab. Drag the thin dividers to
  resize panels while their content stays visible.
- Enter a URL, hostname, localhost address, or search phrase. `about:blank`
  opens the built-in start page. Page links requesting a new window become tabs
  in the originating pane. Cmd+click (Ctrl+click on Windows) opens one tab
  immediately after the source tab in the same pane, without navigating the source.

Option+click a web link on macOS to open a temporary preview centered across the
whole app window, including split layouts. The preview shares the browsing session;
its links navigate inside it, and another Option+click replaces its page. Close it
with **×**, Escape, or a click outside. **Open in new tab** moves the live page,
including its history and form state, into the originating pane. On macOS the page
and backdrop fade in together over 180ms and fade out over 140ms on dismissal.
Reduce Motion disables the transitions. Previews are not restored after restarting.

Open **Settings** with the header gear or Cmd/Ctrl+, to choose the search engine
and homepage URL. The homepage is used for new tabs, new spaces, replacement tabs
when the final tab is closed, and startup when there is no saved session. Existing
and restored tabs are unchanged. Use an HTTP/HTTPS URL; leave the field blank or
enter `about:blank` to use the built-in start page. Press Enter or leave the field
to apply. Preferences are saved separately from the workspace session.

Settings also shows whether Axolotl is the default browser for HTTP and HTTPS.
**Make default** requests the macOS confirmation or opens Windows Default apps;
there is no startup prompt. External web links open in new tabs in the focused
panel of the active space and bring the existing app window forward. macOS needs
the packaged `.app`; Windows registers the current user's executable on launch.
See [default-browser setup and checks](docs/settings.md#default-browser-and-external-links).

| Shortcut | Action |
| --- | --- |
| Cmd/Ctrl+Q | Quit the app (Cmd+Q also works while native web content has focus) |
| Cmd/Ctrl+L | Edit the focused page URL inline |
| Cmd+Shift+C (macOS) | Copy the current page's full URL as plain text |
| Cmd/Ctrl+T | New tab in the focused pane |
| Cmd/Ctrl+Shift+T | New space using the homepage; focus its address input |
| Cmd+1 … Cmd+9 (macOS) | Select the corresponding space in its current strip order |
| Option+click (macOS) | Open a link in a temporary window-centered preview |
| Cmd/Ctrl+W | Close the focused tab |
| Cmd/Ctrl+R | Reload the focused tab |
| Cmd/Ctrl+Plus or Equals / Minus | Zoom the focused page in / out (saved per hostname) |
| Cmd/Ctrl+0 | Reset website zoom to 100% |
| Cmd/Ctrl+F | Find in the focused page; Enter / Shift+Enter for next / previous, Escape to close |
| Cmd+[ / Cmd+] (macOS) | Back / forward in the focused page, including page inputs and chrome focus |
| Alt+Left / Alt+Right | Back / forward (when delivered to the shell) |
| F6 | Cycle pane focus when delivered to the GPUI shell |

Page zoom is shared across schemes and ports on the same hostname, but not subdomains,
and survives app restarts. Find is case-insensitive plain text with match highlighting
and a current/total count. Both tools also target open pins and link previews.
See [page zoom and find](docs/page-tools.md) for details and verification.

Right-click a space and choose **Pin Space** to keep it at the left of the header,
after the window controls. Pinned spaces show their emoji or title initial in a
square tab, with the full title on hover and no close button. **Unpin Space**
returns it to the front of the regular tabs. Drag to reorder within either group;
regular spaces scroll independently. Pin status and both groups' order survive
restarts. Cmd+number follows the combined pinned-then-regular order.

Cmd+number uses the current space order after dragging spaces. Cmd+9 selects
the ninth space; numbers without a corresponding space leave the current view unchanged.

History shortcuts target the native focused page, or the open preview/pin or
focused pane when chrome has focus. Cmd+[ and Cmd+] are reserved before webpage
handlers on macOS, navigate once per key press, and do nothing in Settings or at
the corresponding history boundary. Reload acts on the focused pane. The native macOS Edit menu
supplies standard copy/paste actions to both web content and the inline URL input.
Cmd+Shift+C works with webpage or browser chrome focus and copies the live page
URL, including query strings and fragments, without changing an unfinished
address edit. Open pinned pages and temporary previews copy their own URL.
A small black **URL copied** toast appears at that page's top-right for 1.6 seconds.
It does not take focus or block clicks; repeated copies refresh it.

Window size and position, zoom/maximized state, and macOS full-screen state are
saved in `app.sqlite3` alongside the browsing session. Normal bounds survive
zoom and full-screen transitions. Restored geometry is kept on an available display;
on macOS it is clamped to the usable screen area. Missing or invalid state uses the
existing centered 1280×860 startup window. `AXOLOTL_DATA_DIR` also isolates window
state; `--smoke-test` never reads or writes the real profile. See
[app persistence](docs/persistence.md) for automatic legacy import and save recovery.

Built-in ad and tracker protection is enabled by default. Use **Settings → Block
ads and trackers** or the website shield in each panel to change it. Lists update
daily while the browser is open; **Update now** retries immediately. See
[content protection and current YouTube limitations](docs/content-protection.md).

Website `alert()`, `confirm()`, and `prompt()` use native dialogs on macOS and
Windows. See [JavaScript dialog behavior and checks](docs/javascript-dialogs.md).

Web-page downloads save automatically to the user's Downloads folder on macOS and
Windows, using the suggested filename and numbered suffixes for conflicts. The
**Downloads** button between pinned tabs and Settings opens this session's list,
with byte counts, progress, and completed, failed or cancelled status. Unknown
file sizes show an indeterminate progress indicator. Use **Cancel** while a file
is downloading, or **Open file** and **Show in Finder / Explorer** after completion.
Files only open when requested. Download history clears when the app quits;
pause, resume, retry and persistent history are not supported.

Native UI performance measurements and methodology: [performance report](docs/performance.md).

## Checks

Split separators snap within 50 logical pixels of 25%, 33⅓%, 50%, 66⅔%, and
75% of their immediate parent panel. Drag farther away to resize freely; releasing
keeps the current position. Nested splits use their own parent bounds, and existing
pane-size limits take priority over snap stops.

```sh
node --test scripts/test-input-bridge.cjs scripts/test-notifications.cjs scripts/test-protection.cjs scripts/test-history-suggestions.cjs
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

The macOS window-state regression launches and quits real native windows using a
temporary profile. It checks relaunches after moving/resizing, zooming, full-screen,
and zoom then full-screen, plus invalid and off-screen geometry. It needs a GUI
session but no Accessibility permission:

```sh
cargo build -p axolotl --locked --features smoke
python3 scripts/test-window-state.py target/debug/axolotl
```

The macOS quit regression checks Cmd+Q through AppKit's event queue and the
actual **Quit Axolotl** menu target.
Each isolated process must exit successfully, save its complete session, and
restore the same tabs on relaunch. It needs a GUI session but no Accessibility
permission:

```sh
cargo build -p axolotl --locked --features smoke
python3 scripts/test-macos-quit.py target/debug/axolotl
```


An optional native integration run opens a real desktop window, exercises the
normal app commands and browser host, prints one `AXOLOTL_SMOKE` JSON result, and
exits. It requires a GUI session and HTTPS access to example.com:

```sh
cargo run -p axolotl --locked --features smoke -- --smoke-test

# Startup placeholder regression (no network needed)
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-launch-loading
# Add --smoke-hold-launch-loading to hold loading and ready states for visual inspection.

# macOS native HTTP/JavaScript browser identity regression (no network needed)
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-browser-identity

# macOS iframe rendering, native clicks and security regression (no network needed)
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-iframes
# Set AXOLOTL_IFRAME_SNAPSHOT=/tmp/axolotl-iframes.png to save the native page image.
# See docs/iframe-evidence/README.md for the fixture and before/after evidence.

# Focused native check for page notifications and unread badges (no network needed)
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-notifications
# Add --smoke-notifications-snapshot=/tmp/axolotl-notifications on macOS for before/after native tab-bar PNGs.

# Downloads: native attachment/blob clicks, conflicts, concurrent progress, cancellation and failures
# Creates UUID-prefixed fixtures in the real Downloads folder and removes its own files.
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-downloads
# Add --smoke-downloads-actions to exercise real Open/Reveal application launches.
# Add --smoke-downloads-snapshot=/tmp/axolotl-downloads.png on macOS for a native dropdown snapshot.
# Add --smoke-keep-overlays when concurrent GUI tests steal focus (smoke builds only).

# macOS native pointer-capture split release regression (no network needed)
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-docking

# Floating tabs/panels: native capture, resize, four edges, chips, pin and return paths
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-floating
# Hold a live floating layout for screenshot/manual inspection:
# append --smoke-hold-floating
python3 scripts/test-floating-session.py target/debug/axolotl

# Windows/macOS four-corner native clipping through splits, resizing and panel moves
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-corners
# Add --smoke-hold-corners to pause each colored layout for visual inspection.

# macOS separator snapping through queued AppKit mouse events (offline native window)
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-split-snap
# Add --smoke-hold-split-snap to hold the final colored nested layout for a screenshot.

# Focused native check for content width, panel cap, and overflow fades
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-bar-sizing

# Space editor, emoji search/select/clear, save/cancel and session roundtrip
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-space-menu

# Pin/title/icon persistence across separate native processes and legacy sessions
python3 scripts/test-space-session.py target/debug/axolotl

# Indexed macOS space shortcuts from page and app-control focus
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-space-shortcuts

# Current-URL clipboard shortcut, navigation, tab switching and ordinary Cmd+C
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-copy-url

# Native overlay fades, interrupted closing, promotion reset and Reduce Motion
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-popup-transition

# Temporary link preview, promotion, split centering and resize
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-link-popup

# Native Google-style OAuth opener, postMessage, redirect and close regression (offline, macOS)
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-oauth

# Opt-in live X Google-sign-in probe (stops before account choice/credentials)
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-x-oauth

# Opt-in live Google result Option-click regression (network required, macOS)
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-google-popup

# Space order, pinned groups, measured insertion slots, edge scrolling and cancellation
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-space-drag
# Add --smoke-hold-space-pins to inspect compact pinned tabs after regular scrolling.
```

A focused macOS input regression checks native link hit testing and sends an OS
Cmd+Q to the test process after verifying it is foreground:

```sh
cargo build -p axolotl --locked --features smoke
python3 scripts/test-macos-input.py "$(pwd)/target/debug/axolotl"
```

This needs Python 3, a GUI session and System Events Accessibility/Automation
permission. Link clicks are synthesized through AppKit/NSWindow hit testing;
they are not JavaScript `dispatchEvent` calls or physical OS mouse input. The
runner checks sibling order, exactly one new tab, retained source document,
ordinary navigation, existing popup handling, graceful exit and no relaunch
observed for three seconds. Run it without another instance of the same binary.

The macOS keyboard regression queues native NSEvents through NSApplication,
including the local event monitor and AppKit key-equivalent dispatch:

```sh
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-keyboard
```

It checks arrow navigation and Shift/Option/Command selection in inputs,
textareas and contenteditable elements, multiline movement, page cancellation,
repeat metadata, ordinary typing/deletion and app shortcuts. No DOM keyboard
events are synthesized. For OS-driven key presses instead, use the same System
Events permissions and an unlocked desktop:

```sh
python3 scripts/test-macos-keyboard.py "$(pwd)/target/debug/axolotl"
```

The macOS docking check delivers AppKit mouse events to the floating bar's
WKWebView and exercises real WebKit pointer capture, IPC, preview and release.
Its fixture supplies the pressed-button state because AppKit synthesis does not
change the OS's global button state. It injects competing shell handlers before
the queued native release to reproduce the race deterministically. This is not
a physical OS mouse test.

For visual inspection of a held drag state, append `--smoke-hold-preview` after
`--smoke-test`; it holds the captured card and insertion placeholder for 20 seconds.
Use `--smoke-hold-split` to hold each of the four native split previews for 10 seconds.

This checks actual native documents and host operations programmatically. It
does **not** synthesize OS mouse or keyboard input. The smoke test explicitly
draws frames because occluded windows can skip display callbacks. See the
[validation report](docs/validation.md) for results and outstanding manual checks.
Repeatable native action, webpage, resource and battery benchmarks are documented
in [docs/benchmarks.md](docs/benchmarks.md).

A macOS/Windows build-and-unit-test workflow is provided in
[`.github/workflows/check.yml`](.github/workflows/check.yml); defining it does not
mean either remote CI job has been run.

## Scope and limitations

This milestone implements spaces, pane tab stacks, nested/resizable splits,
native browser views, and compact title tabs. The core retains a 2×2 grid
operation for integration testing; it is not exposed as a standalone UI control.
Workspace layouts and
tab metadata are in memory. Website cookies/storage follow the platform engine's
default store; this is not a private-browsing mode or an isolated profile per
space. Hidden pages stay loaded and can keep running; page suspension and memory
budgets remain future work.

Native child views cover GPUI overlays. Floating panel bars and drag previews use
transparent native child surfaces above live pages. Browser permission controls, profiles,
passwords, browser extensions, mobile shells, and layout persistence are outside
this milestone. Chrome extensions are not supported by this Wry backend.

The GUI-independent workspace core can be reused by future mobile shells. See
[architecture and integration findings](docs/architecture.md) for the engine
boundary, platform tradeoffs, and follow-up work.

The bundled emoji catalog contains Unicode Emoji 16.0 fully-qualified sequences
and English names from [Unicode emoji-test.txt](https://unicode.org/Public/emoji/16.0/emoji-test.txt),
under the [Unicode License](crates/desktop/assets/unicode-license.txt). Emoji
appearance and glyph availability depend on the operating system.

### Pinned tab popups

Right-click a page chip and choose **Pin to header**, or drag a page chip into the
main header. Pinned pages move into circular favicon buttons immediately left of
Downloads and stay available across spaces. Click a button to open its page; click
it again, click outside the popup, or press Escape to dismiss it. Opening another
pin switches the popup. The existing page stays alive, retaining forms, scrolling
and navigation history.

Drag header icons to reorder them. Right-click an icon and choose **Unpin to
focused pane** to return the page to the active space. Extra pins appear under
**•••**; right-click an overflow entry to unpin it. Drag the popup's bottom-right
corner to resize it. Pins, their order and preferred sizes restore on restart;
popups start at 480 × 640 and fit within the window.

Right-click a pinned tab and choose **Edit icon…** to pick an emoji using the
same catalog and searchable grid as **Edit space**. Save applies it to the header
and overflow menu. Clear and Save returns to the page favicon; Escape discards
the draft. Emoji choices are restored with the pinned tabs.
