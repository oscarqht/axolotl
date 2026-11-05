# Prototype architecture

Axolotl separates workspace state from the native shell and the web engine. The
prototype targets macOS and Windows; it does not implement saved-layout
persistence, mobile shells, or browser extensions.

```text
GPUI BrowserApp / controls / input / split handles
    │ commands                         ▲ events
    ▼                                  │
workspace-core                  BrowserHost → WryHost
spaces → recursive layouts             │
panes → tab stacks                     ├─ macOS: WKWebView
                                       └─ Windows: WebView2
```

## Shared workspace state

`crates/workspace-core` depends on URL parsing but has no GUI or web-engine
dependency. `Workspace` owns spaces and globally unique numeric IDs. A `Space`
owns a layout, panes, and a focused pane. A `Pane` owns its tab stack and active
tab ID. A `Tab` holds an ID, current URL, and page title; it does not own a native
view or history implementation.

Layouts are recursive binary trees. A leaf references a pane; a split owns an
axis, ratio, split ID, and two child layouts. Horizontal means side by side;
vertical means top and bottom. Nested splits allow asymmetric arrangements. The
grid command constructs two vertically split columns. It retains existing panes
up to four, creates missing panes, and merges excess panes' tabs into the first
pane so conversion does not discard pages.

Dragging a floating chip retains its existing tab ID. Same-panel drops reorder;
other chip rows insert, and panel centers append. Moving the last tab collapses
its source leaf. Whole-panel handle drops merge ordered stacks in a center or
reposition the existing panel leaf at an edge. Native views stay keyed by tab ID,
preserving document state and history. Cross-space and outside drops cancel.

Dragging starts after five logical pixels. Page edges offer split targets in
outer thirds; corners use the nearest normalized edge, preferring top/bottom on
ties. A sole-tab panel cannot split itself. A valid edge creates a nested 50/50
split. Escape and window deactivation cancel without layout changes. Page views
remain visible during dragging.
A tab-sized placeholder occupies the destination insertion slot, using the same
fixed widths, gaps and scroll offset as hit testing. The latest drag-event position
is shared by the card and placeholder, including in the native smoke test.

Closing a pane closes its tabs and collapses its parent split. The final pane
and final space are retained. Closing the last tab creates a fresh blank tab.
Commands preserve a valid active space, focused pane, and active tab. Switching
spaces preserves each space's focused pane and each pane's active tab.

`Layout::geometry` converts a layout to pane and divider rectangles. It respects
subtree minimum sizes when space permits. If the window cannot fit the requested
minimum sizes, panes shrink proportionally; their dimensions remain nonnegative.
Dividers include their parent bounds for converting pointer position to a split
ratio. Stored ratios clamp to 0.1–0.9, and nonfinite ratios are rejected.

`normalize_url` accepts HTTP, HTTPS, and `about:blank`; it supplies a scheme for
hostnames and localhost addresses and defaults search terms to Google. The desktop
uses `normalize_url_with_engine` with the app-wide Google/DuckDuckGo/Bing preference
(see [Settings](settings.md)). It
rejects privileged schemes, malformed URLs, control characters, and addresses
containing embedded credentials. Native navigation also has a scheme allowlist.

## Native shell and browser boundary

`crates/desktop/src/main.rs` starts GPUI and initializes `gpui-component` for the
address input. GPUI renders the compact space strip, navigation bar, pane tabs,
split handles, and the spaces header. `BrowserApp` translates those interactions into
workspace operations and native browser calls. The address input follows the
focused pane's active tab.

`BrowserHost` in `browser.rs` defines page creation, placement, visibility,
focus, navigation, history commands, reload, URL reading, page retention, and
event draining. `WryHost` owns one native child webview per created tab, keyed by
tab ID. Views are created lazily when tabs first become visible. Switching tabs
or spaces hides existing views; it does not unload them or suspend their page
JavaScript. Closing tabs releases their host entries. Hidden views can therefore
continue consuming CPU and memory.

Native handles stay on the UI thread. Wry callbacks enqueue events through a
bounded channel, and the GPUI app drains it on a 40 ms timer. Page titles and
navigation events update model metadata. An initialization script forwards page
focus and selected browser shortcuts to the shell. Its IPC surface permits only
ordinary UI actions and exposes no general filesystem or process access.

`BrowserApp` holds `Rc<dyn BrowserHost>`. Canvas geometry callbacks retain that
interface and tab IDs, and call `place`; no Wry native handle appears in the
shell or layout core. A replacement engine supplies the trait implementation
and its platform integration. Destruction and all host calls stay on the UI
thread. Workspace state remains reusable without either desktop dependency.

Links requesting another native window become tabs in the source pane. Native
popup creation is denied. Downloads use native WKDownload / WebView2 operations,
with a session-only manager shared by tabs, pins and previews. A native overlay
above page surfaces shows progress and explicit cancel/open/reveal actions. Files
save to the system Downloads folder through private staging files; exclusive
publication avoids overwriting existing files. Hidden retired page engines stay
alive until active transfers finish, including after their tab or preview closes.
This remains a browser integration prototype without complete browser permission,
profile, password, or session-management implementations.

## Composition and platform differences

Wry hosts system web engines: WKWebView on macOS and WebView2 on Windows. GPUI
draws application chrome independently of the web page. Native views occupy real
child surfaces over the GPUI window, so a GPUI element drawn over their bounds
does not automatically appear above page content.

Floating controls use one small transparent native child surface per panel,
created above page views. `floating_bar.rs` hosts the trusted HTML/JavaScript;
`panel_controls.rs` routes its messages into application commands. The surface
shrinks to a count/handle/plus at rest and expands into a single scrolling chip
row on hover, focus or touch expansion. Native address inputs retain drafts
across title/URL changes. Controls are rebuilt above newly created page views;
page views themselves are retained. Chip pointer capture begins only after the
drag threshold, preserving ordinary clicks and double-clicks.

`docking.rs` shares edge, center and measured chip-midpoint hit testing with
native previews and final drops. Initialization scripts capture trusted wheel
and touch gestures across documents/subframes. Per-panel gesture accumulation
survives tab switches and control rebuilds, preventing repeated switching from
trackpad momentum. Vertical page scrolling is left to the native engine.

During tab dragging and divider resizing, active pages remain visible and the
normal geometry canvas continues updating their bounds. The shell retains the
initial drag's mouse events; the native preview is offset from the cursor and
flips at window edges so it cannot cover the drop position.

The split placeholder is a translucent native Wry child above page surfaces,
covering the selected half of the target's content. Its bounds and drop action
use the same edge hit test. Center drops preview a full-panel merge; chip rows
show insertion markers instead. The preview hides outside valid panels, and is disposed on cancellation/drop. The thumbnail is created after this
surface so it remains visible above the placeholder.

The drag card is a separate native Wry child created after the page surfaces,
placing it above their pixels. It displays an escaped title and a frozen PNG from
WKWebView's snapshot API on macOS or WebView2's CapturePreview on Windows. Lazy
inactive tabs are created and sized without replacing the active page; capture
waits for the native page load. Capture errors leave a title card with an
unavailable message. Each drag owns its callback image slot, preventing a late
snapshot from updating a subsequent drag. Dropping or cancelling disposes the
card without recreating the source page.

On macOS, native child surfaces can coincide with suspended GPUI display-link
presentation. Drag/resize updates invalidate the parent AppKit layer, letting
GPUI's normal displayLayer callback draw and present chrome. This avoids forced
frame drawing in normal app execution.

The Windows entry point sets `GPUI_DISABLE_DIRECT_COMPOSITION=true` before GPUI
starts, selecting the window path suitable for WebView2 child HWNDs. Windows
requires the WebView2 Runtime; compilation alone does not confirm that it is
installed or that child views render correctly. macOS builds need full Xcode,
with command-line tools; GPUI runtime shaders avoid requiring the separate Metal
compiler in this configuration. The CI workflow defines checks for both native build targets
on their respective runners; it does not perform desktop interaction tests.

## Validation and performance

Core tests cover tab identity, closure and focus invariants, nested splits,
grid conversion without tab loss, exact geometry, undersized layouts, URL
validation, and a deterministic command sequence. Native host tests check its
navigation allowlist. Runtime rendering, navigation, text input, focus, tab
switching, split dragging, and floating-control layering need an actual desktop session.
The run-specific validation report belongs in `docs/validation.md` alongside platform and hardware limitations.

No startup, resize frame-time, memory, idle-energy, or navigation benchmark is
claimed by this architecture document. Measure those with release builds on
both platforms before making performance comparisons. Include the browser
renderer processes when measuring memory, distinguish cold and warm startup,
and test increasing counts of visible panes and retained hidden tabs. The 40 ms
event timer and retained hidden webviews are current implementation tradeoffs,
not measured performance benefits.

## Extensions and mobile

The Wry host does not implement Chrome's extension runtime, Chrome Web Store
installation, extension APIs, or extension UI. WebView2 being Chromium-based
does not make Chrome extensions available through this host. If extensions
become a priority, evaluate a different engine integration with real target
extensions and their permission/UI requirements; do not infer compatibility
from the layout or `BrowserHost` interface.

A future mobile application can share the Rust workspace model, commands, URL
normalization, and layout tree. It still needs a mobile shell, native web-engine
adapter, platform lifecycle integration, and touch interactions. Phone layouts
may expose one pane at a time while retaining the shared tree. This prototype
does not establish GPUI mobile support or ship Android/iOS adapters. Persistence
and schema/version migration also remain future work.

## System appearance

The component theme initializes from OS appearance rather than forcing dark mode.
`BrowserApp` observes native window appearance changes, synchronizes the component
theme, and requests shell presentation. `ChromeColors` supplies matching light and
dark colors for space/page tabs, text, focus and insertion indicators, split
dividers, and shell backgrounds. Inline inputs use the same component theme.
Native floating controls and preview cards use `prefers-color-scheme` media queries.
Web pages retain their own appearance rules.

## Pinned tabs

`Workspace::pinned_tabs` is a window-wide ordered collection of tabs and preferred
popup sizes. Pin/unpin operations move the original `Tab`; they never reassign
its ID. Pinning the final tab leaves a blank tab in the final pane, or collapses
an emptied split pane. Unpinning appends to the focused pane of the active space.
Snapshots retain pins and sizes, accept older version-1 files without pins, and
validate IDs across both pane tabs and pins.

`pinned_tabs.rs` owns the transient open-popup/menu/drag state. `prepare_views`
retains pinned native views even while hidden. A transparent native child below
the raised existing page catches outside clicks and draws the popup frame and
bottom-right resize grip. Raising changes sibling order without recreating the
page; only one pinned page is visible at a time. The header remains interactive
above the shield. Popup dimensions clamp to the viewport while keeping the saved
preferred size. Authenticated Escape messages from the page dismiss the popup.

Header circles use small PNG favicons read in the page origin; unavailable icons
fall back to a generic symbol. A bounded strip reserves room for spaces, window
dragging and Settings, with excess pins in a scrollable overflow menu. Pin entries
in that menu also support right-click unpinning. Hidden pages remain live and can
consume resources, as with ordinary inactive tabs. Restart restores URLs, order
and sizes; in-memory DOM and navigation history survive pin/open/close/unpin during
the same app session, rather than being serialized across process restarts.

Pinned tabs also persist an optional emoji override. The pin icon editor uses the
Edit space emoji catalog and search/select/clear UI. The native popup records its
left edge when a resize starts, so dragging its bottom-right grip grows toward the
pointer while preserving that edge and fitting within the viewport.
