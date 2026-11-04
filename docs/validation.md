# Closing empty tab panes

Run on 2026-11-04 in `bro-close-empty-tab-panels` on macOS.

- `cargo test --workspace --all-features --locked`: 18 tests passed. Added
  coverage checks active/inactive tab selection, nested and root split collapse,
  focus transfer, invalid/cross-space closes, and fresh blank replacement in the
  final pane even when another space exists.
- Formatting, whitespace checks and Clippy with all targets/features passed.
- Native WKWebView smoke passed through the GPUI `CloseTab` action used by
  Cmd+W. It checks remaining-tab selection, nested/root pane removal, focus
  transfer, disposal of closed native views, expanded surviving page dimensions,
  and a freshly loaded start page in the final pane.
- An isolated ad hoc signed desktop build received actual Cmd+T/Cmd+W input.
  Closing a nested pane removed its native page and expanded the sibling; closing
  down to one pane expanded it across the window; closing the final tab loaded a
  fresh empty start page. The test app was quit afterward.
- Full foreground input acceptance remains limited: tab chrome sometimes stayed
  stale until window zoom/restore, and automation focus/shortcut delivery was
  inconsistent. In one check, editing a right-page field through accessibility
  did not make it the model's focused pane; Cmd+W closed the left pane. This run
  therefore does not establish reliable pointer/keyboard focus routing in every
  foreground case. No unrelated focus/redraw workaround was added. Windows
  desktop input was not tested.

# Tab edge split validation

Run on 2026-11-04 on macOS in `bro-tab-edge-splits`.

- All 15 workspace unit tests, formatting, whitespace checks and all-target,
  all-feature Clippy passed. Added core cases cover normalized corner distances,
  vertical tie preference, center/outside rejection, all four edge orders,
  same-pane multi-tab splitting, sole-tab rejection, nested source collapse,
  focus/identity retention and invalid/cross-space atomic rejection.
- Native WKWebView smoke passed, including all four split directions through
  the app's drag handlers, same-pane and nested cross-pane movement, empty-source
  collapse, and retained DOM identity, form state, URL and back/forward history.
  It dispatches the real Escape action during each split preview and verifies
  that the later mouse release has no effect. Existing tab-strip docking passes.
- An isolated, ad hoc signed app was exercised with actual desktop input. A
  typed value `Retained through edge splits` survived left, right, top and bottom
  drops. Cross-pane moves collapsed their empty sources. A center drop and a
  sole-tab self-edge drop left the layout unchanged. Adding a second tab then
  dragging it to its own content edge created a nested split with the value
  retained and the moved page focused. These drag outcomes appeared without
  zoom/restore.
- Held smoke screenshots confirmed a translucent right-half and bottom-half
  placeholder above live page pixels, with the thumbnail above the placeholder.
  The desktop adapter performs each OS drag atomically; it cannot pause a held
  OS drag or inject Escape midway. Held-preview visuals and Escape routing were
  therefore checked with the native smoke, separately from desktop drops.
- The existing initial/occluded compositor limitation was encountered in the
  first diagnostic held run; zooming exposed the current chrome. A subsequent
  held run displayed the overlays directly. This change does not claim a general
  fix for the baseline foreground/occlusion behavior.
- Windows native overlay layering, DPI and desktop interaction remain untested
  on this Mac.

# System appearance validation

Run on 2026-11-04 on macOS in `bro-tab-drag-preview`.

- Native smoke verifies startup theme equals OS window appearance, then applies
  window-local AppKit dark/light/dark overrides. Real appearance notifications
  update the component theme and shell/divider palettes; clearing the override
  returns to OS appearance. No system preference is changed by the test.
- Desktop screenshots confirmed readable light and dark tab bars in the same
  running app. The extended `--smoke-hold-theme` visual run later failed the
  existing URL-cancellation fixture after inspection; the regular unattended
  smoke passed. This visual mode is diagnostic, not a clean full-suite result.
- Formatting, 13 unit tests, and all-target/all-feature Clippy passed.
- Windows native appearance notifications and high-contrast modes remain
  unverified on this Mac.

# Dropdown over live pages validation

Run on 2026-11-04 on macOS in `bro-tab-drag-preview`.

- Native smoke now verifies that all four active page surfaces remain visible
  while a dropdown is open and after it closes. The remaining native smoke
  checks, 13 unit tests, formatting and Clippy passed.
- Desktop clicks selected the horizontal split action, then opened a dropdown
  above both native pages. Visual inspection confirmed both pages and the input
  `Keep visible behind dropdown` remained visible behind the menu.
- Escape and clicking outside the menu dismissed its native surface. Page form
  state remained intact. Initial app presentation still needed a zoom/restore in
  this desktop automation session; opening and dismissing the menu afterward did
  not require it.
- Windows dropdown interaction and native layering remain untested on this Mac.

# Live tab drag and split resize validation

Run on 2026-11-04 on macOS in `bro-tab-drag-preview`.

- Workspace unit tests, formatting, whitespace checks and Clippy with all targets
  and features passed (13 unit tests).
- Native WKWebView smoke passed. Added checks keep interactions active across
  rendered frames, verify page visibility and changed native viewport dimensions
  during resize, decode a captured thumbnail with the dragged title, and dispose
  the preview on cancellation. A lazy inactive tab also loads/captures while the
  original active pages remain visible. Existing threshold, invalid drop,
  cancellation, insertion, document identity, form and history checks still pass.
- An ad hoc signed app with an isolated bundle ID was exercised through desktop
  input. A divider drag changed both page widths without zoom/restore. A tab drag
  into another pane immediately displayed the merged full-width pane and retained
  the input value `Retained through native drag`. Another drag released over page
  content kept both panes, tabs and that value intact.
- Held native smoke states were inspected visually: the destination strip showed
  a tab-shaped insertion placeholder, and the correctly titled Example Domain
  thumbnail appeared above native page pixels. The desktop adapter completes
  each OS drag atomically, so it cannot pause a real held OS drag for a screenshot;
  intermediate visual inspection used `--smoke-hold-preview` instead.
- The existing unattended desktop compositor issue still affected initial menus
  and some held-smoke screenshots: zoom/restore was needed to inspect native
  pixels. The drag/resize-specific AppKit layer invalidation fixed the tested
  desktop input outcomes without requiring zoom/restore after those operations.
  This does not establish that every foreground/occlusion scenario is resolved.
- The Windows CapturePreview/stream adapter type-checked in isolation for
  `x86_64-pc-windows-msvc`. Full desktop cross-compilation on this Mac stopped in
  `ring` because the Windows C headers/toolchain are unavailable. No full Windows
  build, native overlay layering, DPI, input or interaction pass is claimed.

The reports below describe earlier milestones; their hide-during-drag behavior
is superseded by this change. Other outstanding prototype checks remain applicable.

# Tab docking validation

Run on 2026-11-04 on macOS in `bro-tab-panel-docking`.

- `cargo test --workspace --all-features --locked`: 13 tests passed, including
  insertion at every destination slot, identity/metadata retention, activation,
  nested source collapse, invalid/cross-space atomic rejection, and insertion
  midpoint calculations.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`,
  formatting, and whitespace checks passed.
- `cargo run -p axolotl --locked --features smoke -- --smoke-test`: passed on
  actual WKWebViews. The added checks exercise the app's drag threshold, valid
  target, outside/content release and cancellation handlers; then verify retained
  document identity, input value, URL, back/forward history, destination activation,
  insertion order and expansion after removing the source pane. The HTTPS history
  fixture verifies back/forward before docking as well as after it.
- `scripts/package-macos.sh`: native debug build and ad hoc signing passed.

Actual desktop input was exercised using an isolated packaged app with its own
bundle ID. Mouse clicks created split panels, and OS drags moved tabs to the end,
before an existing destination tab, and between two destination tabs. Screenshots
confirmed the moved tab active in the requested slot, retained input text
(`Keep this while docking` and `Inserted middle`), and a full-width remaining
panel after the source emptied. A drag released over page content retained both
panels and their tab stacks.

The desktop session repeatedly showed stale GPUI chrome until the native window
was zoomed/restored. Native surfaces hid on interaction and the updated layout
and page state appeared after that resize. This behavior also affects existing
menus, as noted in the earlier report below. Thus the OS drag outcomes were
verified, but smooth foreground redraw and the insertion marker during a held
OS drag remain unverified. The automation API performs a complete drag in one
call and cannot pause it for a screenshot. No forced frame-drawing workaround
was added to normal app execution. Windows interaction remains untested.

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
