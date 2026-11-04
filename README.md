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
- Tab bars, split borders, inline URL fields, dropdowns and drag cards follow the
  OS light/dark appearance and update while the app is running.
- Space tabs show the title of their focused page; each pane has its own page
  tabs. Double-click either title to edit that page's URL in place. Enter visits,
  Escape cancels, and switching tabs, spaces, panes, or leaving the input cancels
  an unfinished edit. URL redirects and title updates do not replace a draft.
- Each tab's **•** menu creates a sibling, splits horizontally (side by side) or
  vertically (stacked), or closes the tab. Space menus act on that space and its
  focused pane; page menus act on that page and its pane, even when inactive.
- New siblings and split panes open the start page. Closing the last page tab
  removes its pane and collapses the split when other panes remain. The final
  pane is retained with a fresh start page; the final space remains.
- Drag the thin divider between panes to resize; page content stays visible and
  follows the divider. Drag a page title to another pane's tab strip to dock it.
  A captured thumbnail card follows the cursor above page content, and a tab-shaped
  placeholder marks the insertion slot. Drag into the outer third of a pane's
  page content to preview a half-pane split on that edge; release to move the tab
  there. Corners choose the nearest normalized edge (ties prefer top/bottom).
  Splitting within the source pane requires multiple tabs. Empty source panes
  collapse; the moved page keeps its form state and history. Escape, window
  deactivation, a center drop, or an outside drop cancels the move.
- Tab dropdowns appear above live page content without hiding any pane.
  Dismiss menus by clicking outside, pressing Escape, or selecting an action.
  Arrow keys move between menu items; Enter selects the focused item.
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

Native child views cover GPUI overlays. Tab dropdowns and drag previews use
transparent native child surfaces above live pages. Browser permission controls, downloads (currently denied), profiles,
passwords, browser extensions, mobile shells, and layout persistence are outside
this milestone. Chrome extensions are not supported by this Wry backend.

The GUI-independent workspace core can be reused by future mobile shells. See
[architecture and integration findings](docs/architecture.md) for the engine
boundary, platform tradeoffs, and follow-up work.
