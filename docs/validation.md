# Prototype validation

Run on 2026-11-04 on macOS 27.0.1, Apple M1 Pro, using Rust 1.91.1,
GPUI 0.2.2, gpui-component 0.5.1, and Wry 0.55.1. Windows hardware was
not available.

## Automated results

- `cargo test --workspace --all-features --locked`: **9 tests passed**.
  Seven core tests cover model/layout invariants, geometry, navigation policy,
  and a deterministic 1,500-command sequence. Two host tests cover URL schemes
  and the authenticated input bridge.
- Strict Clippy for the workspace, every target and feature: passed.
- Rust formatting and whitespace checks: passed.
- Native macOS debug build: passed. A locally signed `.app` can be generated
  with `scripts/package-macos.sh`.
- The opt-in native smoke test passed on actual WKWebView instances. It checked:
  page DOM availability; separate pane document state; HTTPS page content;
  native back/forward history; nested horizontal/vertical layout; four nonempty
  grid browser viewports; native view hiding/restoration while the GPUI menu is
  open; split-ratio relayout; hidden-space state retention; tab creation and
  closed-view disposal.

The smoke test calls the normal Rust app commands and actual browser host. It
reads native engine results through asynchronous JavaScript evaluation. It
waits for native page-load completion because Wry's macOS implementation does
not retain a result callback for evaluation submitted before the initial load.
It explicitly draws GPUI frames so an occluded test window can lay out its
native children. Its elapsed time includes deliberate waits and HTTPS network
work; it is **not** a startup or frame-rate benchmark.

## Desktop interaction limits

The macOS computer-use adapter repeatedly failed to attach with
`cgWindowNotFound`, even though native window metadata showed a visible,
onscreen Axolotl window. A read-only accessibility probe showed the same unusual
AX window identity behavior for Chrome, so this did not establish an
Axolotl-specific accessibility defect. No application accessibility workaround
was added on that evidence.

The native `focus` call succeeded, but the smoke window did not receive OS
activation (`document.hasFocus()` was false). Consequently **keyboard focus,
real text entry, clipboard delivery, pointer dragging, and visual menu layering
have not completed end-to-end acceptance**. Menu visibility transitions and
layout geometry passed programmatically; those results do not substitute for
these manual checks. The app has been built and exercised, but complete desktop
interaction acceptance remains outstanding.

## Manual acceptance checklist

Run these on macOS and Windows in a foreground GUI session:

1. Enter two different HTTPS URLs into side-by-side panes. Follow links, then
   use back/forward/reload in each pane. Confirm the other page is unaffected.
2. Click a text field on each page, type, select/copy/paste, and use Cmd/Ctrl+L
   to return to the address field. Confirm the address belongs to the clicked
   pane. Test these actions inside a cross-origin iframe as well.
3. Add two tabs per pane, switch between them, switch spaces, and return.
   Confirm document state, page scroll position, and histories remain separate.
4. Split horizontally, then split one pane vertically. Drag each divider in
   both directions. Release outside the window, then re-enter and verify resize
   mode ends. Resize the window and check that page bounds follow the layout.
5. Create a grid, open **···**, and confirm the entire menu is visible. Select a
   command, then **Done**; pages must return with their prior document state.
   Open the start page's native select menu and a page context menu.
6. Maximize and restore a pane. Close tabs and panes, including the final tab;
   confirm the app keeps a valid focused target. Test many tabs/spaces and small
   windows. Click every pane's new-tab button to verify it targets that pane.
7. On Windows, verify WebView2 runtime discovery, 100%/150%/200% DPI, a second
   monitor, IME input, focus after Alt+Tab, and WebView2 child-window rendering.
8. Close the window while pages load, relaunch, and confirm clean shutdown. The
   prototype intentionally starts a fresh workspace; saved layouts are absent.

## Performance and engine findings

- GPUI draws the shell, while each visible page occupies a native engine view.
  Runtime validation demonstrated four nonempty native page viewports together.
- Native pages cover GPUI pixels. During menus and divider dragging, hiding the
  native surfaces avoids that composition conflict. The tradeoff is placeholder
  content during resizing, rather than a live page preview.
- Views are created lazily and released on tab closure. Hidden views retain
  documents and may continue consuming CPU/memory. There is no suspension policy.
- The shell polls browser events every 40 ms and the focused native URL about
  every 480 ms to observe same-document navigation. Idle-energy and latency
  impact have not been benchmarked.
- Release startup, resize frame time, total browser-process memory, idle energy,
  and Windows behavior remain unmeasured. No performance comparison with Electron
  or other browsers is claimed.
- This backend supplies no Chrome extension runtime. A future CEF/Chromium or
  other engine adapter requires a separate feasibility prototype.
