# Tab chip context menu

Run on 2026-11-05 in `bro-tab-chip-context-menu` on macOS.

- Right-clicking a floating-bar chip opens a menu with three groups separated
  by lines (no titles): Back / Forward / Refresh, Set title…, and Close. It acts
  on the right-clicked tab even when that tab is inactive, and does not select it.
  The 48px native surface grows upward while the menu is open, so it is not
  clipped; outside click, Escape, window blur or any action dismisses it.
- Back/Forward are enabled from a page probe (`navigation.canGoBack/canGoForward`,
  falling back to `history.length`); unreadable results, or tabs with no native
  view yet, disable them. Refresh is disabled without a native view.
- Set title… edits inline on the chip. The result is `Tab::custom_title`, which
  overrides the page title in chips, drag previews and the compact space strip
  while the page title keeps updating underneath. Saving an empty value clears
  it and restores the live page title. There is no session persistence in this
  app yet, so the custom title lives in the workspace model only.
- Added core tests for override/clear and for surviving tab moves, a host test
  for history-probe parsing, and a native smoke check covering structure,
  targeting an inactive tab, disabled items, set/clear title, dismissal and close.
- Passed: `cargo check`/`cargo test --workspace --all-features --locked` and the
  full native smoke run. Native OS right-click delivery and Windows were not
  exercised; the menu is opened by dispatching a `contextmenu` event.

# macOS website passkey integration

Run on 2026-11-05 in `bro-macos-passkey-support` on macOS 27.0.1.

- Added browser permission management through AuthenticationServices. WKWebView
  continues to own website WebAuthn requests, origin checks and provider UI.
  OS version and the running task's entitlement are checked before manager use.
  Unknown states fail closed; a nonblocking 60-second timeout releases pending
  state. App-level cleanup does not depend on the requesting window surviving.
- `cargo test --workspace --all-features --locked`: 30 passed (9 desktop,
  21 core), including two permission-state/gating regressions.
- Passed: six JavaScript bridge tests, workspace formatting, whitespace checks,
  strict Clippy with all targets/features, native debug build, ad-hoc packaging
  and `codesign --verify --strict`. Signing preflight rejected missing files,
  malformed entitlements and entitlement inputs combined with ad-hoc signing.
- Opened this worktree's packaged app through native computer use, clicked
  Axolotl → Enable Passkeys…, and observed the missing-entitlement alert.
- `cargo run -p axolotl --locked --features smoke -- --smoke-test`: passed.
  This is a programmatic native WKWebView test; it reported OS activation
  unavailable and does not synthesize OS mouse/keyboard input.
- No local provisioning profiles were present. Apple-approved signing, actual
  passkey registration/assertion, provider availability, cancellation/denial,
  permission persistence and window closure during a system permission request
  remain interactive checks. Older macOS and Windows were not exercised.
  See [setup and manual acceptance checks](passkeys-macos.md). The local ad-hoc
  bundle is not evidence of successful website passkey authentication.

# Plus-button address autofocus

Run on 2026-11-05 in `bro-floating-panel-tab-chips` on macOS.

- Clicking plus now uses the same new-tab command as Cmd/Ctrl+T: the new tab is
  selected, its inline address editor opens, and the address is focused/selected
  for immediate replacement. Hovering plus still does not expand the chips.
- Full native smoke passed, including a plus-click IPC check that verifies the
  new tab's editing target, actual native DOM input focus and full address
  selection. Strict Clippy passed and the macOS app was rebuilt. OS input and
  Windows runtime checks remain unverified.

# Translucent idle bar

Run on 2026-11-05 in `bro-floating-panel-tab-chips` on macOS.

- The floating bar has an 80% alpha background in light and dark themes. Overall
  opacity is 20% at rest and 100% on hover, with a 160ms fade. Existing reduced-
  motion settings disable the fade. Hover opacity is independent of chip expansion.
- Focused native sizing/transition/overflow checks and whitespace checks passed;
  the macOS app bundle was rebuilt. Foreground OS hover and Windows remain unverified.

# Cmd+Q and sibling link routing

Run on 2026-11-05 in `bro-fix-quit-and-cmd-click` on macOS arm64,
Rust 1.91.1 and Xcode 27.2.

- Traced GPUI's quit action to asynchronous AppKit termination and GPUI shutdown.
  There is no application relaunch/reopen handler in this repository. Wry's
  child WKWebView has special key-equivalent handling; Cmd+Q is now reserved by
  an application-local native key monitor, consumed once, and dispatched
  asynchronously to GPUI's existing graceful quit path. Other keys/modifiers
  and the native Quit menu retain their existing behavior.
- The reported empty-panel relaunch was not reliably reproduced on the baseline.
  An initial mouse-driven comparison was discarded after discovering it changed
  the foreground application. This change hardens native quit routing; it does
  not establish the root cause of an external or intermittent relaunch.
- Wry's navigation callback exposes only the URL, not click modifiers. Existing
  popup handling remains an append-to-source-pane operation. A trusted click
  bridge now cancels default navigation/popup creation and queues a separate,
  nonce-authenticated, URL-filtered sibling event. The model inserts immediately
  after the source and ignores stale or inactive-space sources.
- Passed: formatting, whitespace checks, 28 Rust workspace unit tests (also with
  all features), six dependency-free JavaScript bridge tests, workspace
  all-target/all-feature checking, native debug builds and strict Clippy.
- The existing full native smoke suite passed. The focused input runner passed
  for ordinary and `target="_blank"` Cmd+click (one adjacent tab, source document
  retained), ordinary popup links (existing append behavior), ordinary navigation
  (no new tab), and OS Cmd+Q with native web content focused (exit status 0, no
  relaunch observed during the following three seconds). Clicks use native
  AppKit hit testing, not physical OS mouse input. An earlier process-targeted
  CoreGraphics driver did not deliver mouse events; it was replaced, not counted
  as a passing click test.
- Tested the development executable, not a packaged/distributed app. Physical
  mouse input, the original intermittent relaunch, and Windows runtime behavior
  remain unverified. The original general foreground acceptance checklist below
  is not superseded by these focused checks.

Reproduce with `node --test scripts/test-input-bridge.cjs`, the Rust checks in
README, and `python3 scripts/test-macos-input.py /absolute/path/to/axolotl` after
building with `--features smoke`. The input runner requires a GUI session and
System Events Accessibility/Automation access.

# Circular floating controls

Run on 2026-11-05 in `bro-floating-panel-tab-chips` on macOS.

- Handle and plus are equal 28px circular buttons with zero padding and centered
  inline SVG icons, eliminating font-baseline offsets. Compact count insets are
  symmetric. Native content sizing accounts for the handle's extra 4px.
- Focused native sizing/transition/overflow checks, formatting, whitespace checks,
  and strict Clippy passed. The macOS app bundle was rebuilt. Foreground OS input
  and Windows validation remain unverified.

# Continuous chip hover expansion

Run on 2026-11-05 in `bro-floating-panel-tab-chips` on macOS.

- Reproduced a transient overflow state in the native CSS timeline: a single
  fitting chip gained arrows/fades while the pill was still narrow. Removing
  those arrows near the end started a second 180ms layout transition.
- Overflow now uses the final expanded viewport capacity, measured fixed
  controls/padding/gaps, and chip flex bases. Editing chips respect their final
  maximum width. Fitting content never gains temporary arrows or fades; true
  overflow still uses the existing scrolling controls and edge masks.
- A native regression samples 0/40/80/120/180/240ms without finishing child-arrow
  animations with the parent. It failed before the fix and passes afterward.
  Focused native sizing/overflow checks and the full native smoke pass, along
  with all 24 unit tests and strict Clippy. The macOS bundle was rebuilt. These
  deterministic engine checks do not synthesize OS hover input; foreground and
  Windows validation remain open.

# Content-fitting expanded bar

Run on 2026-11-05 in `bro-floating-panel-tab-chips` on macOS.

- Expanded bars now fit the handle, chips and plus button. Panel width with 16px
  visible edge margins is the maximum; it is no longer the default expanded size.
  A single chip uses a 250px pill, two use 426px, and additional chips grow the
  bar until it reaches the panel limit and shows overflow controls/fades.
- The native surface holds the previous width until shrink animations finish,
  including chip removal and exiting address editing. The CSS endpoint is
  separate from that temporarily held surface width.
- All 24 unit tests and strict Clippy passed. The focused native sizing check
  (`--smoke-test --smoke-bar-sizing`) passed for one/two chips, removal, and many
  chips capped to the panel with fades. The full native suite also passed on
  the final run. The focused check runs independently of the existing
  drag-preview check, which has intermittently failed with
  `Missing native drag card` in the full suite. Foreground OS input and Windows
  runtime behavior remain unverified. The local macOS bundle was rebuilt.

# Chip-area expansion triggers

Run on 2026-11-05 in `bro-floating-panel-tab-chips` on macOS.

- Hover/focus expansion is limited to the count and chip region. Handle and plus
  interactions do not initiate expansion or keep the bar open. Handle dragging
  also suppresses count hover. Rebuilt expanded controls schedule guarded collapse
  so plus-created native pages cannot leave the replacement bar stuck open.
- Plus creates a blank tab without address editing. Native-bar, page and GPUI
  Cmd/Ctrl+T still use the existing address-editing behavior.
- All 24 unit tests and strict Clippy passed. Native WKWebView smoke passed on
  rerun, including negative hover/focus checks for handle/plus, count hover,
  plus creation without editing, and the prior animation/docking/history checks.
  Earlier runs reported `Missing native drag card` in an existing preview check;
  the cancellation cause was not isolated. OS input and Windows remain unverified.

# Floating bar animation and width

Run on 2026-11-05 in `bro-floating-panel-tab-chips` on macOS.

- Added a 240ms eased width transition with count/chip crossfades. Expanded bars
  fill their panel with 16px visible edge margins, without the previous 960px cap.
  Overflow uses native CSS masks at both scroll edges; each fade clears when its
  corresponding first/last chip is fully reachable. Existing arrows remain.
- Expansion starts after the host acknowledges wide native bounds, using a pixel
  endpoint to avoid changing the interpolation basis during native resizing.
  Collapse keeps the native surface wide until completion. An epoch-guarded
  fallback finishes paused CSS timelines before shrinking an occluded view.
  Reduced-motion preferences disable transitions. Editor focus and active-chip
  reveal do not depend on frame callbacks that WebKit may pause when occluded.
- All 24 unit tests, native WKWebView smoke, strict Clippy, formatting, JavaScript
  syntax and whitespace checks passed. The local macOS app bundle was rebuilt.
  Native smoke checks full-panel bounds and CSS interpolation, collapse without
  native clipping, masks at the start/middle/end, editor focus and tab selection.
  Because this test window is occluded, interpolation assertions explicitly seek
  the native CSS width timeline to 70ms; these are not OS-frame or pointer tests.
- Foreground hover animation and Windows runtime behavior remain unverified.

# Floating panel tab chips

Run on 2026-11-05 in `bro-floating-panel-tab-chips` on macOS.

- Space tabs now have close buttons and a trailing create button; the final space
  is protected. Panel controls use small transparent native child surfaces above
  live pages, with compact counts, expandable chips, address inputs and overflow.
  Split menus were removed; chip and whole-panel dragging create splits.
- All 24 unit tests passed, including same-panel insertion slots, whole-stack
  merging/repositioning on every edge, invalid-operation atomicity, wraparound,
  and one-switch-per-trackpad-gesture thresholds. Strict Clippy, formatting and
  whitespace checks passed.
- The programmatic WKWebView smoke run passed. It checks live pages alongside
  floating bars; focused new-tab address editing and Enter/Escape; compact count,
  expansion and single-row overflow; tab docking, all four edge splits and
  cancellation; whole-panel edge moves and center merging; retained tab order,
  native DOM identity, form values and back/forward history.
- OS pointer/keyboard/trackpad/touch acceptance remains unverified. Computer use
  could launch the local app bundle but could not acquire its window, returning
  `cgWindowNotFound`; the native smoke also reported OS activation unavailable.
  The smoke invokes production handlers and native DOM/IPC, and does not
  synthesize OS input. Windows compilation/runtime and physical touch gestures
  were not available on this Mac. Existing Windows native-view creation support
  is retained; cross-platform CI and hands-on interaction checks remain needed.

# Hidden native tab scrollbars

Run on 2026-11-05 in `bro-hide-tab-scrollbars` on macOS arm64 with Rust
1.91.1 and Xcode 27.2.

- Shared page-view initialization injects scrollbar-only CSS for each document
  and subframe. Standard `scrollbar-width` and WebKit/Chromium scrollbar pseudo
  elements hide both axes, without changing overflow, dimensions, or input
  handling. New nested elements inherit the stylesheet's rules automatically.
- Constructed stylesheets avoid inline-style CSP restrictions on current engines;
  older engines fall back to a style element installed when the document root
  exists. That fallback is subject to the website's inline-style CSP. Document
  styles do not pierce encapsulated shadow roots or native engine controls.
  Website-drawn scrollbar widgets remain outside scope.
- All 19 unit tests passed. Formatting, whitespace checks, JavaScript syntax
  checks, `cargo check --workspace --all-targets --all-features --locked`, and
  strict Clippy with all targets/features passed. The smoke-enabled executable
  built and the full native WKWebView smoke test passed.
- New native smoke assertions check root and dynamically inserted nested
  scrollbars, both-axis programmatic scrolling, unchanged nested overflow, and
  absence of nested scrollbar gutters. They run in the initial tab, a new split
  tab, after HTTPS navigation, and after reload into a new document. Existing
  navigation/history, tab/split, docking, and retained-state checks also passed.
- An additional JavaScript harness checked preservation of existing adopted
  sheets and both immediate/deferred style-element fallback paths.
- Windows WebView2 was not built or run on this macOS host. Subframe injection,
  restrictive-CSP pages, and older-engine fallback were not exercised in a native
  engine. OS wheel, trackpad, and keyboard scrolling and visual appearance while
  scrolling remain manual checks; the native smoke test does not synthesize OS
  input, and reported OS activation unavailable.

# Windows spaces header controls

Run on 2026-11-04 on Windows x64.

- Reproduced the spaces header failing to move the window with actual mouse
  input. GPUI's focusable shell prevented the default caption mouse-down.
  Moved focus tracking into the content container so native caption operations
  can proceed, while shell actions remain in the focus dispatch path.
- Actual desktop mouse input verified dragging the empty spaces header,
  double-click maximize, and the new minimize, maximize/restore, and close
  buttons. The maximize glyph switched to restore while maximized; minimize
  was confirmed by the desktop API reporting a minimized window, and close
  removed the app window.
- Windows build, all 19 unit tests, formatting, whitespace checks, Clippy with
  all targets/features, and the programmatic WebView2 smoke test passed.
  The smoke test covers inline input/action routing, native page placement,
  nested splits, resizing, tab docking, and retained document state.
- macOS was not rerun on this Windows host. Multiple monitors and DPI changes
  remain manual checks.

# Windows support validation

Run on 2026-11-04 on Windows x64 with Rust 1.93.0 (MSVC), Visual Studio
2022 C++ tools, Windows SDK 10.0.26100.0, and WebView2 Evergreen 156.0.4314.58.

- Added `scripts/build-windows.ps1` and `scripts/run-windows.ps1` with debug,
  release, and native smoke options. Shared setup discovers Visual Studio and
  `fxc.exe`; it also ran successfully under Windows PowerShell 5.1.
- Native Windows compilation, 19 unit tests, formatting, whitespace checks, and
  Clippy with all targets/features passed. Windows CI now builds the executable
  through the helper script. Remote CI was not run during this local check.
- Fixed WebView2 construction reentering GPUI through its nested Windows message
  loop. Windows child views are now created by a serial foreground worker outside
  GPUI updates; pending geometry, visibility, and commands are applied afterward.
- App-supplied HTML may appear as a `data:` navigation in WebView2. Start pages
  and overlays allow their exact encoded payload; arbitrary data URLs remain
  denied. A regression test checks the inline HTML navigation boundary.
- Handled Wry's `SetFocus` success reported as HRESULT 0. Hidden lazy-tab
  thumbnails temporarily render outside the parent clipping area and restore
  the latest requested visibility and geometry when capture completes.
- Two consecutive final programmatic native smoke runs passed on actual WebView2 documents,
  including inline URL editing, navigation/history, menus over visible pages,
  nested splits, tab closing, active and lazy-tab thumbnails, docking, all four
  split edges, Escape cancellation, and retained DOM/form/history state.
- These checks do not simulate OS mouse/keyboard input. Foreground acceptance,
  multiple monitors, DPI changes, appearance notifications, high contrast, and
  release performance remain manual checks. macOS was not rerun on this Windows host.

Earlier platform reports follow.

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
