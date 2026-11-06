# Native GPUI tab overlay

The bottom tab strip is rendered by GPUI in a retained popup window per visible pane. It no longer constructs a WebView, loads HTML, evaluates JavaScript, or uses browser IPC. The former floating-bar HTML, JavaScript, history JavaScript and CSS-transition fixture have been removed.

On Windows the overlay is an owned popup of the browser window, allowing its DirectComposition surface to blend over WebView2. Position and size changes run between GPUI updates, allowing its resize callbacks to update the renderer. The popup follows its owner and is raised above asynchronously created WebView2 page children. Its transparent GPUI root omits the normal application background and window decoration; no rounded native region clips the pill. DWM frame extension and a borderless popup style enable pixel alpha over the page. GPUI owns mouse capture; Rust forwards tab actions and drag coordinates to the existing workspace model. Layout measurements supply insertion positions, and only changed measurements wake the owner.

The GPUI strip uses 50% opacity at rest and 85% on hover. Overflowing chip rows blend into the bar with gradients at both ends. The GPUI strip implements compact expansion, horizontal tab scrolling, selection, close/new actions, notification counts, address input, local history suggestions, title editing and the tab action menu. Address drafts remain in a GPUI InputState rather than being rewritten on every model update. Overlay windows are recycled across spaces and closed when no longer retained. The webpage engine and separate floating-panel controls are unchanged.

## Validation on Windows

- `cargo test --workspace --locked`: passed, including containment and sizing regression cases.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: passed.
- `cargo build -p axolotl --locked --features smoke`: passed.
- `axolotl.exe --smoke-test --smoke-windows-controls`: passed; verifies native stacking, expansion, selection, editor initialization/focus, cancellation and close actions.
- `axolotl.exe --smoke-test --smoke-bar-sizing`: passed.
- Real Windows mouse input expanded the strip and dragged a chip onto a page edge, producing two live panes with native strips above both pages.

`--smoke-test --smoke-native-chips-ui` holds an isolated four-tab fixture for manual input checks without opening the saved user session.

## Validation limits

The macOS child-window path has not been built or exercised on this Windows host. Legacy optional smoke suites and benchmarks that inspect tab-strip DOM through `bar_eval` require migration to GPUI probes; the removed renderer deliberately cannot execute those JavaScript probes. The default smoke editor and panel sizing checks now use Rust probes. No numerical before/after performance claim is made: the chip WebView cost is removed, but each native overlay has a GPUI window and renderer whose total resource cost requires a controlled comparison.

The small local gpui-component 0.5.1 patch exposes Root::transparent_overlay() while preserving the Root type required by InputState. See vendor/gpui-component/AXOLOTL-PATCH.md for provenance and scope. Native smoke also checks the full 48px client height, so non-client decoration cannot silently clip the row.
