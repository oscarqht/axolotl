# Compact tab chrome validation

Run on 2026-11-04 on macOS, using the pinned GPUI 0.2.2,
gpui-component 0.5.1, and Wry 0.55.1 dependencies. Windows hardware was
not available.

## Automated results

- `cargo test --workspace --all-features --locked`: **9 tests passed**.
  Seven core tests cover model/layout invariants, geometry, navigation policy,
  and a deterministic 1,500-command sequence. Two host tests cover URL schemes
  and the authenticated input bridge.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`:
  passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- Native macOS debug build and ad hoc signed app packaging: passed.
- The opt-in native smoke test passed on actual WKWebView instances. In addition
  to the original document, history, layout, retention and disposal checks, it
  exercises the handlers used by compact tabs:
  - A space URL input mounts with its full URL selected. Cancelling preserves
    the document and URL.
  - Enter from the page input navigates to example.com and closes editing.
  - Escape routes from the focused input to the shell and cancels editing.
  - Changing panes cancels an unfinished space edit.
  - A page sibling action targets its originating pane even when another pane
    was focused. Page close disposes its native view.
  - A space split action targets that space even when another space was active.
    Sibling creation and close actions preserve the last space.
  - Closing the final page tab creates a fresh start page.
  - Native page surfaces hide for a tab menu and return after dismissal.

Run the smoke test with:

```sh
cargo run -p axolotl --locked --features smoke -- --smoke-test
```

This uses normal app handlers, input action routing and actual browser documents;
it does **not** synthesize OS mouse or keyboard events. Its explicit frame draws
allow native geometry checks while a window is occluded. These checks do not
establish that a GPUI dropdown was presented by the compositor above page pixels.

## Desktop interaction evidence and limits

The packaged app was opened through the macOS computer-use adapter. Screenshots
confirmed the transparent native titlebar, traffic lights before the first space
tab, both compact title strips, the absence of standalone toolbar controls,
updated titles, side-by-side native pages, and the inline URL input. The input's
light-theme text on the dark shell was caught visually and fixed by setting the
component theme to dark; a subsequent screenshot confirmed readable URL text.

The adapter could attach, unlike the earlier prototype run, but the desktop
session did not reliably activate/present the app or deliver successive text,
Escape, and traffic-light actions. A temporary isolated test instance with
explicit frame draws did not resolve the discrepancy between native view updates
and screenshots of the GPUI scene. That diagnostic mode was removed. The smoke
run likewise reported that a native focus call succeeded while
`document.hasFocus()` was false.

Consequently **complete foreground macOS interaction acceptance is outstanding**.
Screenshots of intermediate state and programmatic handler success are not proof
that double-clicking, keyboard editing, native controls, dragging or menu
presentation work end to end under ordinary OS input. This session does not
establish whether those remaining symptoms come solely from automation or also
occur during normal foreground use. Windows interaction remains untested.

## Remaining foreground acceptance

1. Click each native traffic light: close, minimize, fullscreen/restore. Drag
   the empty space-strip area and resize the window from its edges.
2. Double-click titles at both levels, including inactive tabs/spaces. Type a
   URL, Enter to visit, Escape to cancel. Test Cmd/Ctrl+L from page text fields,
   immediately replacing the selected URL, clipboard paste, and IME input.
3. Switch page tabs, spaces and panes while editing. Click native page content,
   including in the same pane. The draft must cancel without stealing page focus.
4. Open every dot menu over real native pages. Verify all four items are visible,
   target the menu's tab/space, and dismiss on action, outside click or Escape.
   Page pixels must return with their state retained.
5. Split horizontally, then vertically in a nested pane. Resize both dividers,
   including releasing outside the window. Confirm page geometry follows the
   window and divider sizes.
6. Close inactive and active tabs/spaces, including the final page tab and final
   space. Create enough siblings to overflow each strip; confirm horizontal
   scrolling and Cmd/Ctrl+L reveal the intended input.
7. On Windows, verify native window controls, WebView2, keyboard focus, DPI and
   monitor changes in a foreground session.

## Retained prototype limitations

Layouts and tab metadata remain in memory. Hidden pages remain loaded and may
consume CPU/memory. Menus and divider dragging temporarily hide native surfaces
because those surfaces otherwise cover GPUI overlays; this is not compositing a
menu over live webpage pixels. The core still supports a grid, but the compact UI
has no standalone grid/maximize controls. Permission controls, downloads, browser
profiles, extensions, saved layouts and performance benchmarking remain outside
this change.
