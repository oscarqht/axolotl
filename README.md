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

Use `scripts/package-macos.sh release` for an optimized build. This is a local
development bundle, without distribution signing, notarization, or an installer.

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
  **+** to create a space. Closing the final space is disabled. Double-click a
  space title to edit its focused page address.
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
- Drag a chip to reorder within a panel, dock into another panel's chip row or
  center, or create a split at any edge. Drag the handle to move the complete
  stack: center drops merge panels and edge drops reposition the whole panel.
  Destination previews appear above live pages. Escape, loss of window activation,
  or releasing outside the workspace cancels. Moving retains native page state
  and back/forward history. Splits are created exclusively by dragging.
- Deliberate horizontal trackpad/touch swipes across page content select the next
  tab on a left swipe and the previous tab on a right swipe, wrapping at both ends.
  Horizontal tab switching takes priority over page horizontal scrolling; vertical
  scrolling remains normal. Trackpad momentum switches once per gesture. On the
  floating bar, dragging a chip/handle takes priority over touch swiping.
- Closing the final tab in a panel removes that panel when other panels remain;
  the final panel instead receives a fresh start tab. Drag the thin dividers to
  resize panels while their content stays visible.
- Enter a URL, hostname, localhost address, or search phrase. `about:blank`
  opens the built-in start page. Page links requesting a new window become tabs
  in the originating pane.

| Shortcut | Action |
| --- | --- |
| Cmd/Ctrl+L | Edit the focused page URL inline |
| Cmd/Ctrl+T | New tab in the focused pane |
| Cmd/Ctrl+W | Close the focused tab |
| Cmd/Ctrl+R | Reload the focused tab |
| Alt+Left / Alt+Right | Back / forward (when delivered to the shell) |
| F6 | Cycle pane focus when delivered to the GPUI shell |

Back, forward, and reload act on the focused pane. The native macOS Edit menu
supplies standard copy/paste actions to both web content and the inline URL input.

## Checks

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

An optional native integration run opens a real desktop window, exercises the
normal app commands and browser host, prints one `AXOLOTL_SMOKE` JSON result, and
exits. It requires a GUI session and HTTPS access to example.com:

```sh
cargo run -p axolotl --locked --features smoke -- --smoke-test

# Focused native check for content width, panel cap, and overflow fades
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-bar-sizing
```

For visual inspection of a held drag state, append `--smoke-hold-preview` after
`--smoke-test`; it holds the captured card and insertion placeholder for 20 seconds.
Use `--smoke-hold-split` to hold each of the four native split previews for 10 seconds.

This checks actual native documents and host operations programmatically. It
does **not** synthesize OS mouse or keyboard input. The smoke test explicitly
draws frames because occluded windows can skip display callbacks. See the
[validation report](docs/validation.md) for results and outstanding manual checks.
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
transparent native child surfaces above live pages. Browser permission controls, downloads (currently denied), profiles,
passwords, browser extensions, mobile shells, and layout persistence are outside
this milestone. Chrome extensions are not supported by this Wry backend.

The GUI-independent workspace core can be reused by future mobile shells. See
[architecture and integration findings](docs/architecture.md) for the engine
boundary, platform tradeoffs, and follow-up work.
