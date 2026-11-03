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

### Windows

Install Rust stable with the MSVC toolchain, Visual Studio Build Tools with
**Desktop development with C++** and a Windows SDK, plus the
[WebView2 Evergreen Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/).
Run from a Visual Studio developer terminal:

```powershell
cargo run -p axolotl --locked
```

GPUI needs the Windows SDK shader compiler (`fxc.exe`). If it cannot find it,
set `GPUI_FXC_PATH` to its absolute path. The app sets
`GPUI_DISABLE_DIRECT_COMPOSITION=true` before initializing GPUI so WebView2 can
render its child windows. Windows build and interaction checks require Windows;
they have not been run on this macOS host.

## Use

- **+** beside the space strip creates another space; select a space to return.
- The address bar follows the focused pane. Enter a URL, hostname, localhost
  address, or search phrase. `about:blank` opens the built-in start page.
- **Split ↔** and **Split ↕** split the focused pane; repeat to nest splits.
- **Grid** arranges four panes in two columns and two rows. Existing tabs survive
  conversion, including when extra panes are consolidated.
- Drag the thin divider between panes to resize. Page surfaces hide during the
  drag, then return at their new sizes.
- Select a pane's header or click its page to focus it. **+** in that header adds
  a tab; select a tab title to switch; **×** closes it.
- **⤢** temporarily maximizes a pane; select it again to restore the layout.
- **···** opens layout controls and a command to close the focused pane. The last
  pane is retained. Closing the last tab replaces it with a fresh start page.

| Shortcut | Action |
| --- | --- |
| Cmd/Ctrl+L | Focus the address bar |
| Cmd/Ctrl+T | New tab in the focused pane |
| Cmd/Ctrl+W | Close the focused tab |
| Cmd/Ctrl+R | Reload the focused tab |
| F6 | Cycle pane focus when delivered to the GPUI shell |

Back, forward, and reload act on the focused pane. The native macOS Edit menu
supplies standard copy/paste actions to both web content and the address input.
Page links requesting a new window become tabs in the originating pane.

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

This checks actual native documents and host operations programmatically. It
does **not** synthesize OS mouse or keyboard input. The smoke test explicitly
draws frames because occluded windows can skip display callbacks. See the
[validation report](docs/validation.md) for results and outstanding manual checks.
A macOS/Windows build-and-unit-test workflow is provided in
[`.github/workflows/check.yml`](.github/workflows/check.yml); defining it does not
mean either remote CI job has been run.

## Scope and limitations

This milestone implements spaces, pane tab stacks, nested/resizable splits, a
2×2 grid, native browser views, and a compact native shell. Workspace layouts and
tab metadata are in memory. Website cookies/storage follow the platform engine's
default store; this is not a private-browsing mode or an isolated profile per
space. Hidden pages stay loaded and can keep running; page suspension and memory
budgets remain future work.

Native child views cover GPUI overlays. The app hides them while its layout menu
is open and restores them afterward; it does not composite a menu over live page
pixels. Browser permission controls, downloads (currently denied), profiles,
passwords, browser extensions, mobile shells, and layout persistence are outside
this milestone. Chrome extensions are not supported by this Wry backend.

The GUI-independent workspace core can be reused by future mobile shells. See
[architecture and integration findings](docs/architecture.md) for the engine
boundary, platform tradeoffs, and follow-up work.
