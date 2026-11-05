# Copy current URL shortcut

Run on 2026-11-05 on macOS in `bro-copy-current-tab-url`.

Cmd+Shift+C is reserved by the existing AppKit local event monitor and dispatched
to an app-level action. It reads the committed native page URL, including query
strings and fragments, rather than the address draft or cached tab metadata.
Copying keeps focus and drafts intact; no committed URL leaves the clipboard
unchanged. Open pinned pages and temporary previews use their own URL; a preview
that is already closing no longer takes precedence over the underlying tab.

- `--smoke-copy-url` passed in 14.267s using real WKWebViews and queued AppKit
  events. It checks document and SPA navigation, switching tabs and returning,
  webpage and chrome focus, GPUI and native address drafts, page shortcut
  interception, pinned pages and previews, and ordinary Cmd+C on selected page
  text. The test restores the previous clipboard. Physical OS keys are not sent.
- All 73 Rust tests passed with all features. Formatting and whitespace checks
  passed. Clippy passed with the existing `collapsible_if` lint allowed; strict
  Clippy still reports two pre-existing pinned-tab drag warnings in `render.rs`.
- JavaScript checks: 17 passed; the existing Escape test still expects modified
  Escape to be ignored, while the unchanged bridge forwards it.
- An additional queued Cmd+C check in a native address field failed after
  switching from the GPUI space address editor. Replaying that complete editor
  sequence with the original HEAD app, shortcut monitor and key bindings also
  failed, while a standalone original-HEAD address-field check passed. This is
  an existing editor-transition issue; this change retains existing Cmd+C/menu
  routing. The passing URL-copy regression covers draft preservation in both
  editors, and verifies ordinary Cmd+C on selected webpage text.

# Temporary preview transitions

Run on 2026-11-05 on macOS in `bro-space-shortcut-link-popup`.

The native page and full-window backdrop use coordinated Core Animation opacity
transitions: 180ms ease-out on opening and 140ms ease-in on natural dismissal
(close button, outside click, Escape or Cmd+W). Both surfaces remain alive until
closing completes. Reopening reverses from the current presentation opacity and
cancels the closing deadline. Promotion clears the page's animation and restores
full opacity before transferring it into a regular tab. Commands that replace
the popup with other UI dispose of it immediately. Reduce Motion skips both the
animation and delayed disposal. Initial surfaces stay hidden until the backdrop
is ready, preventing an unpositioned header flash.

- `--smoke-popup-transition` passed in 7.274s. It measures actual native layer
  presentation opacity on the page and backdrop during both directions, checks
  retained/removed native views around the close deadline, repeated dismissal,
  reopening past an old deadline, promotion during closing, immediate command
  and Settings cleanup, and a test-local Reduce Motion override. No OS preference
  is changed by the test.
- `--smoke-link-popup` passed in 31.588s, including real Option-click, protection
  policy off/on reloads, shared session, split/resized centering, popup navigation,
  promotion/history, dismissal and existing shortcut behavior.
- `cargo test --workspace --all-features --locked`: 72 tests passed using an
  isolated target directory for this checkout.
- Formatting and whitespace checks passed. Strict Clippy remains blocked by the
  same two pre-existing pinned-tab drag `collapsible_if` warnings in `render.rs`.

These checks use real WKWebViews, AppKit input and Core Animation presentation
layers. Windows keeps its existing immediate behavior and was not interaction
tested. The previously documented GPUI chrome redraw limitation remains.

# Option-click after protection integration

Run on 2026-11-05 on macOS in `bro-space-shortcut-link-popup`.

Reproduced the reported Google-result failure on a real WKWebView: a trusted
Option-click (`altKey=true`) on a Google result navigated the source to GitHub
and created no preview. The link path and modifiers were correct. Inspection
showed that protection registration cleared all five initial user scripts.
`WKUserContentController.userScripts` returned a live NSArray whose count became
zero after `removeAllUserScripts`, so the preservation loop restored nothing.
Both `window.ipc` and the browser input bridge were absent on Google.

The fix retains each WKUserScript in an independent Rust Vec before clearing the
controller. It reinstalls the existing IPC/input scripts in order and replaces
only the protection script. This applies at initial view creation and subsequent
protection updates.

- The live `--smoke-google-popup` check passed: native Option-click opened the
  GitHub result in a real preview view and kept the exact Google search URL.
- `--smoke-link-popup` passed in 30.490s. Its new network-independent regression
  toggles protection off/on, waits for actual document reloads, verifies IPC,
  and delivers native Option-clicks while checking retained source URL/state.
  Existing popup navigation, split/resized centering, shared session, promotion,
  history, dismissal and other shortcut checks also passed.
- `cargo test --workspace --all-features --locked`: 66 tests passed using an
  isolated build directory for this checkout.
- `--smoke-protection` passed in 23.759s: native request blocking, cosmetics,
  site exceptions and global disable/re-enable all remained functional.
- Formatting and whitespace checks passed.

Unrelated baseline checks remain failing: the input-bridge JavaScript suite has
10 passing tests and one Shift+Escape expectation failure; the current bridge
forwards modified Escape while the old test expects it to be ignored. Strict
Clippy reports two `collapsible_if` errors in pinned-tab drag handlers in
`render.rs`. Neither the input bridge, that JavaScript test, nor the pinned-tab
handlers changed in this fix. The earlier foreground redraw limitation remains.

# macOS webpage arrow-key routing

Run on 2026-11-05 in `bro-fix-webpage-arrow-keys` on macOS.

- Wry child WKWebViews decline key equivalents, allowing the parent GPUIView
  to interpret navigation events through its input context and app bindings.
  The baseline native regression reproduced Option+Left being consumed before
  the input received a keydown. The screenshot's box-character insertion was
  not reproduced with synthetic NSEvents.
- The existing AppKit local monitor now delivers arrow keydowns directly to
  the focused WebKit responder, then consumes the original event. This avoids
  competing GPUI interpretation while retaining WebKit editing, modifiers,
  repeat state and cancelable DOM events. Other keys and GPUI editors retain
  their existing routing; no page-side editing emulation is installed.
- Passed: 72 native arrow cases across input, textarea and contenteditable,
  including Shift, Option, Command and modifier-plus-Shift combinations. Each
  case checks caret/selection, unchanged text, one trusted arrow keydown and
  no beforeinput insertion. Additional checks cover multiline movement and
  selection, preventDefault, repeat metadata, typing/backspace, history
  navigation outside editors, Cmd+comma, Cmd+L and a GPUI shell editor.
- The fixture uses a local HTTP origin so history navigation obeys the real
  URL policy. It queues NSEvents through NSApplication's event queue and local
  monitors, exercising AppKit and WebKit rather than DOM dispatchEvent.
- Physical OS key injection was attempted with System Events. After retrying
  in an isolated signed test bundle, process activation succeeded, but macOS
  rejected key injection: `osascript is not allowed to send keystrokes` (1002).
  The exact screenshot symptom still needs physical-key confirmation with
  Accessibility access. The optional `scripts/test-macos-keyboard.py` runner
  drives the same fixture with OS key codes (typing assumes a US layout).
- Passed: 49 Rust tests with all features, 10 JavaScript bridge/notification
  tests, formatting, whitespace checks and strict all-target/all-feature Clippy.

# Edit space and emoji icons

Run on 2026-11-05 in `bro-edit-space-emoji` on macOS.

- Space context menu now opens Edit space with a title field, optional icon,
  searchable offline Unicode emoji catalog, Clear, and Save. Save/Enter commits
  title and icon together; Escape/outside click discards the draft. Blank titles
  continue following the focused page. Icons render before the space title and
  clearing removes their spacing.
- Passed: 52 workspace unit tests, 10 JavaScript tests, formatting, whitespace
  checks, and all-target/all-feature strict Clippy.
- Focused native WebKit smoke covers search/select/clear/no results, Save/Enter,
  cancellation after changes, inactive-space targeting without selection or
  page-title changes, and viewport-edge clamping. Native document interactions
  are programmatic; this check does not inject physical OS input.
- `scripts/test-space-session.py` saved through the real native editor, restarted
  the app with an isolated temporary profile, verified the restored title/icon
  in the editor, and checked a legacy session with the icon property removed.
- Repeated focused smoke runs were interrupted by window deactivation, which
  correctly dismisses the popup. One complete popup run and the three-process
  persistence check passed; reliable repeats require retaining foreground focus.
- Windows runtime and OS-specific emoji glyph availability were not checked on
  this Mac.

# Indexed space shortcut validation

Run on 2026-11-05 on macOS in `bro-space-shortcut-link-popup`.

Cmd+1 through Cmd+9 select the corresponding space in the current strip order.
Cmd+9 selects the ninth space, including when a tenth space exists. Unavailable
indices preserve selection, address drafts, Settings and link previews. Valid
selections cancel editing, dismiss overlays and focus the selected native page.

- `cargo test --workspace --all-features --locked`: 54 tests passed, including
  exact numeric indices and rejection of non-Command/extra-modifier input.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`,
  formatting and whitespace checks passed.
- `--smoke-test --smoke-space-shortcuts` passed on actual WKWebViews. AppKit
  queued key events exercise the native shortcut monitor and app-level action
  dispatch. Checks cover page/input/Settings/preview focus, missing indices,
  reorder semantics, the exact ninth space among ten, retained page/form state,
  lazy view creation and native responder ownership after selection.

The smoke test checks that NSWindow's first responder belongs to the selected
WKWebView. This verifies keyboard focus without requiring the shared desktop to
keep the test app foreground. It does not inject physical OS keyboard events.
Indexed selection uses a global action handler because native views can leave
GPUI with no active window or a stale focused chrome node. The handler defers
updates to the owning window, avoiding reentrant native action dispatch.
The existing foreground chrome redraw limitation documented below remains.

# Space shortcut and temporary link preview validation

Run on 2026-11-05 on macOS in `bro-space-shortcut-link-popup`.

- `cargo test --workspace --all-features --locked`: 53 tests passed.
- `node scripts/test-input-bridge.cjs`: 11 tests passed, including trusted
  Option-click, safe destination/nonce checks, Cmd+Shift+T, Escape and unchanged
  Cmd+click/Cmd+T routing.
- Native `--smoke-test --smoke-link-popup` passed on actual WKWebViews in 18.671s.
  Loopback fixtures and AppKit/NSWindow input verify Option-click and
  `target=_blank` suppression, a unique shared session cookie, retained source
  form state, singleton replacement and ordinary navigation inside the preview.
  Geometry is measured across split panes and after a native window resize.
  Native header promotion preserves the live document, form, back/forward history
  and subsequent title callbacks in the originating pane. Outside click, Escape
  and the close button dispose of the temporary page. Native Cmd+click retains
  sibling insertion and source state; native Cmd+Shift+T from the popup uses the
  homepage and retains address-input focus. Popup state stays outside snapshots.
- The header test caught inactive-window first clicks being swallowed. The
  backdrop now accepts first mouse, matching page views; the passing test keeps
  its inactive-window click rather than activating the window first.
- A separate ad hoc signed review app was operated through desktop automation.
  OS Cmd+Shift+T from shell and webpage focus created spaces at the configured
  homepage, and screenshots confirmed the selected address input. Option-click
  displayed the native preview, source page, dim backdrop, title, close button
  and promotion button, centered after resizing.

The existing foreground GPUI chrome redraw problem remains: changes to the space
strip sometimes appeared only after zooming/resizing the window. Desktop
accessibility input also became intermittent (`noWindowsAvailable` and timeouts),
so complete foreground navigation/dismissal acceptance is not claimed from that
adapter. The AppKit regression above passed these native routes, using explicit
frame draws in the existing smoke harness. This does not establish smooth
foreground chrome presentation. Windows interaction remains untested.

# Queued native tab split release

Run on 2026-11-05 in `bro-fix-tab-split-drop` on macOS.

- Root cause: native floating-bar pointer events enter an IPC queue polled every
  40 ms. Immediate GPUI mouse moves and mouse-up/out handlers shared the same
  drag state. A released-button shell move could cancel a valid held preview
  before the queued WebKit `drag-end` arrived, leaving that release with no drag.
  A deterministic native regression failed with `native release did not split`
  before the fix.
- Each drag now records its input owner. Shell move/up/out handlers leave a
  native-owned drag for its originating bar's IPC release or cancellation.
  Native events from other bars cannot finish it. Escape and window deactivation
  continue to cancel either owner. Target geometry and model moves are unchanged.
- `--smoke-test --smoke-docking` exercises trusted AppKit input delivered directly
  to the originating WKWebView, WebKit pointer capture, IPC and preview/drop
  completion. The fixture supplies pressed-button state because AppKit synthesis
  does not change global OS button state, and injects competing shell handlers
  before the native release poll. This is not physical OS mouse injection.
- Passed: all 16 native split releases (four edges, same/cross pane, quick/paused),
  native DOM/form retention, Escape cancellation and inert late native release.
- The full native smoke suite also passed, covering tab reorder/docking, center
  and whole-panel merges, outside-drop cancellation, and retained DOM/form/history.
- Passed: 44 workspace unit tests, nine JavaScript bridge tests, formatting,
  whitespace checks, and strict Clippy across all targets/features.
- Physical OS mouse injection could not be run by the test process because it
  lacks macOS Accessibility permission. Windows runtime checks were not run on
  this Mac; the shared ownership routing applies to both platforms.

# Space tab context menu

Run on 2026-11-05 in `bro-space-tab-context-menu` on macOS.

- Right-clicking a space tab opens a single-item menu, Set title…. A transparent
  native child is created after page and floating-control surfaces so the menu
  and title editor remain above webpage content and panels. Outside pointer-down,
  Escape, window deactivation or submission disposes the overlay. Menu/editor
  geometry is clamped to the window, including after a resize.
- Removed the superseded GPUI space-menu path that invoked URL editing, including
  duplicate state and handlers left by integration.
- Titles belong to spaces, independently of page titles and focused panels, and
  persist in the session snapshot. Blank input restores the focused page's title.
  Older snapshots without the optional space `custom_title` still load.
- Passed: 43 workspace tests, all-target/all-feature strict Clippy, formatting,
  whitespace checks, native debug packaging/signature verification, and the full
  native smoke suite. New smoke assertions cover a single menu item, inactive
  space targeting without selection, focused editing, save/clear without changing
  any page tab custom title or the target page title, edge placement,
  Escape and outside dismissal.
- Actual desktop right-click opened the menu above the Google/SeaTalk split view;
  screenshots confirmed the menu remained above live page pixels, and typing a
  title plus Enter updated the space strip. Clearing restored the original label.
  Windows runtime/layering remains untested on this Mac.

# Page notification badges

Run on 2026-11-05 in `bro-tab-notification-badges` on macOS.

- Page `new Notification(...)` calls reach the tab host even from hidden tabs.
  Unfocused tabs accumulate unread counts; foreground selected tabs do not.
  Selecting a chip, focusing its pane/native page, or reactivating the window
  clears the selected tab. Closed tabs are removed from unread state.
- Chips show red rounded badges with white numbers; the compact bar sums the
  current panel's tabs alongside its active index/count. Space tabs sum unread
  counts across all panels in that space, independently of the selected space.
  Values above 99 display
  `99+`, with the exact count in a tooltip and accessible label.
- The page shim supports badge-only notifications without OS permissions and
  forwards to a native constructor only when native permission is already
  granted. Its permission methods describe in-app badge support. Service-worker
  and push notifications are excluded; counts are not persisted.
- Passed: 42 Rust tests (all features), nine JavaScript bridge/fallback tests,
  formatting, whitespace checks, and strict Clippy for all targets/features.
- `cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-notifications`
  passed in the native WKWebView host. It checks the page API, foreground
  suppression, native focus and chip-selection clearing, combined panel totals,
  expanded/compact badge text and computed colors/geometry, moves and closes,
  space totals across panels, and notification isolation in hidden spaces.
  It uses programmatic native evaluation and chip clicks, without physical OS
  mouse/keyboard input. Startup attempts while the login window owned the
  session could not exercise foreground behavior; the final run did exercise it.
- Windows runtime and actual OS notification delivery were not exercised.

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
  it and restores the live page title. The custom title is stored in the session
  snapshot as an optional `custom_title` per tab (older files load unchanged).
- Added core tests for override/clear, tab moves and snapshot round-trip, a host test
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

## Pinned tab popovers (2026-11-05)

- `cargo test -p workspace-core`: 37 passed, including pin identity, empty-source
  collapse/final-pane replacement, cross-space focused-pane unpinning, order and
  size persistence, legacy snapshots, and invalid saved pin data.
- `cargo test -p axolotl --features smoke`: 16 passed, including bounded header
  overflow and existing bridge, session and preference checks.
- `node scripts/test-input-bridge.cjs`: 7 passed; changed JavaScript parses.
- `cargo check -p axolotl` and smoke-feature build pass on macOS.
- `target/debug/axolotl --smoke-test --smoke-pins` exercises the real native chip
  menu and popup IPC, header-drop completion, real page favicon transfer, live document/form/scroll/history preservation,
  cross-space popup visibility, switching/repeat-click/outside/Escape dismissal,
  resizing, snapshot round trip and focused-pane unpinning. The smoke run uses
  disposable state and does not read or overwrite the user's saved workspace.

The smoke checks are programmatic native integration checks. OS pointer dragging,
header icon reordering, grip dragging, narrow-window visual layout, full process
restart restoration and Windows runtime behavior still require manual validation.
Computer Use listed the isolated review app but repeatedly timed out acquiring
its window, so no successful visual or OS-input verification is claimed here.

### Pinned emoji icons and resize grip follow-up (2026-11-05)

- `cargo test --workspace --features smoke`: 20 desktop and 38 core tests pass.
  Added emoji persistence, clear-to-favicon, and legacy pinned-session coverage.
- The native `--smoke-test --smoke-pins` run passes with the Edit space emoji
  catalog and matching searchable grid. It exercises search/select/save, no
  results, draft cancellation, clearing back to the favicon and snapshot restore.
- Native DOM geometry verifies the grip is at the bottom-right with the correct
  resize cursor. Pointer-handler dispatch verifies positive horizontal movement
  increases width, vertical movement changes height, and the left edge stays fixed.
  This check dispatches fixture pointer events; it does not simulate OS pointer input.
- The popup script passes `node --check`; formatting and whitespace checks pass.


## Native Google OAuth popup linkage (2026-11-05)

Run on macOS in `bro-fix-google-oauth`.

- Reproduced the regression on an archived unchanged `main`: the new native
  `--smoke-oauth` fixture failed because `window.open` returned null. The previous
  native handler denied the request and reopened its URL in an unrelated view.
- `--smoke-oauth` passed on real WKWebViews in 17.527s. It covers native
  WindowProxy/opener linkage, named reuse, empty-URL popup navigation, separate
  origins, message source/origin and targetOrigin checks, blocked cross-origin
  DOM access and file navigation, shared cookies, account-button redirect,
  callback messaging, native close, restored source focus, shell cancellation,
  ordinary noopener links, preview promotion without reload, sleep protection,
  and background closure after pinning the child without stealing focus.
- Existing native `--smoke-link-popup` passed in 34.975s, including protection
  reloads, source preservation, ordinary navigation, promotion, Cmd-click sibling
  insertion and ordinary new-tab editing.
- Live `--smoke-x-oauth` reached X's actual Google button, opened a native Axolotl
  tab at `accounts.google.com/v3/signin/identifier`, and verified non-null
  `window.opener`. One run stopped at the email/phone prompt; the final run
  reached `/v3/signin/accountchooser` in 15.999s with two available accounts and
  a non-null opener. Neither account was selected. Live account selection,
  credentials, MFA/consent and completed X sign-in remain unverified. Earlier probe attempts
  exposed iframe-loading/focus and pre-navigation evaluation races; the probe now
  waits for button loading, activates the native window, and waits for a finished
  provider document before inspection. Later unattended runs also intermittently
  failed to open the embedded button's popup, so the live probe is not a reliable
  unattended acceptance check; the successful identifier-page observation is
  partial verification only. The final probe waited for the source document and
  native activation before measuring/clicking the button, and reached the account
  chooser. No account identifiers, credentials or OAuth query values were recorded.
- All 73 Rust tests and five new favicon JavaScript regressions passed. The
  favicon tests first reproduced three failures on the previous bridge.
- Formatting and whitespace checks passed. Strict Clippy reports two existing
  `collapsible_if` findings in unchanged `render.rs`; Clippy passes with that
  lint allowed. Combined input/favicons JavaScript tests pass 15/16: the existing
  Shift+Escape expectation in `scripts/test-input-bridge.cjs:161` fails identically
  on unchanged HEAD and was left outside this fix.

The checks use programmatic native AppKit input, not physical OS mouse/keyboard
input. WebKit retains security decisions; the fix does not synthesize opener
objects, relay OAuth messages in JavaScript, fetch favicons with native privilege,
or bypass CSP, CORS, same-origin checks or provider restrictions.
