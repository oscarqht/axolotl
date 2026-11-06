# Horizontal scrolling validation

Run on macOS on 2026-11-06, branch `bro-restore-horizontal-scrolling`.

The shipped page bridge now leaves horizontal wheel/trackpad and touch input
with scrollable content in the gesture direction. It examines the composed
ancestor path and document scrolling element, including shadow DOM and RTL
negative offsets. Hidden overflow does not count as user-scrollable content.
A scrolling gesture stays with the page through its boundary; a fresh boundary
gesture can navigate history. Wheel gestures reset after 300 ms idle, matching
the native history accumulator. Vertical and floating-bar handlers are unchanged.

Results: 46 JavaScript tests passed; 133 Rust tests passed with one existing
manual benchmark ignored; strict Clippy, formatting and whitespace checks passed.
The native acceptance flow passed, including floating-bar tab switching in both
directions. See [native.log](native.log) for the native result.

Validation commands (use an isolated `CARGO_TARGET_DIR` when other worktrees are
building):

```sh
node --test scripts/test-*.cjs
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
cargo build -p axolotl --features smoke --locked
AXOLOTL_SCROLL_SNAPSHOT="$PWD/docs/horizontal-scroll-evidence/native.png" \
  "$CARGO_TARGET_DIR/debug/axolotl" --smoke-test --smoke-horizontal-scroll
```

The native smoke uses a deterministic localhost fixture and isolated smoke
profile. CoreGraphics constructs wheel events; AppKit delivers them directly to
the actual app's WKWebView responder. This bypasses OS permission-dependent
posting. The test measures real scroll offsets in both directions for root,
nested, shadow DOM and RTL scrollers, verifies vertical scrolling, and navigates
back/forward via the authenticated bridge at the root's directional boundaries.
It checks page listeners receive trusted scrolling events, and floating-bar wheel
input still switches tabs in both directions without changing page history.

[native.png](native.png) is an unedited WKWebView snapshot after native input.
Touch directions, small initial movement, boundary ownership, cancellation,
normalization, trust checks and wheel momentum are covered by the shipped-bridge
JavaScript tests. Physical trackpad momentum, physical touchscreen input and
Windows runtime behavior have not been exercised on this Mac.
