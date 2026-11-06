# Floating panels

Drag a tab chip or the stack handle out into the workspace. A free-space drop
creates an overlay; dragging its header moves it and its bottom grip resizes it.
The last tiled pane leaves a fresh start pane when detached. Native pages keep
their existing tab IDs, documents and back/forward histories throughout moves.

Drop targets are explicit:

- A panel's tab bar merges a tab or complete stack, with an insertion marker.
- A tiled panel's 28-point edge region creates a split, with a green preview.
- The workspace's outer 22-point edge region docks an overlay, with a purple preview.
- Other content drops float, with a blue preview.

Window-edge overlays keep the tiled layout unchanged. Each has a title/favicon
chip; corner space is reserved so chips on adjacent edges remain accessible.
Hover reveals the page, and leaving both chip and panel hides it after a 400 ms
grace period. One unpinned panel reveals per edge. The diamond button pins the
panel open. Address editing, menus, dragging and resizing keep it open. Dragging
one chip onto another on its edge reorders them. Dragging away undocks it.

Floating metadata belongs to the space. The existing version-1 session format
adds an optional `floating` array, defaulting to empty for older sessions. It
stores pane references, preferred geometry, edge, pin state, and chip order.
Snapshot validation requires each pane to appear exactly once in either the
layout or the floating array and rejects invalid geometry. Runtime hover and
front-to-back ordering are transient. Unpinned edge panels restore collapsed.
Geometry is fitted to the current workspace on every layout pass without
throwing away the preferred size for a larger window.

## Verification on macOS

```sh
cargo test --workspace --locked
cargo run -p axolotl --features smoke --locked -- --smoke-test --smoke-floating
python3 scripts/test-floating-session.py target/debug/axolotl
```

The native smoke delivers AppKit events through WebKit pointer capture for tab
and whole-stack detachment, header movement, resizing, all four window edges,
chip ordering, and split/merge return paths. It checks live document identity and
form state, native visibility, pinning, space isolation, and fitting after window
resize. Hover is invoked through the chip's handler; this is a programmatic
native check, not physical mouse automation. The separate-process test verifies
six panels, all edges, order, pin state, geometry and native restored visibility
against the actual SQLite session saver.

Use `--smoke-hold-floating` with the native smoke to hold the final layout for
inspection. The session probe supports `--smoke-hold-floating-session` and
`--smoke-floating-snapshot=/absolute/path.png` for a native view-cache capture.
For a workspace image that includes transparent native controls, use:

```sh
python3 scripts/capture-floating-panels.py target/debug/axolotl /tmp/floating.png
```

This requires Pillow. It captures native views and combines transparent surfaces
at their recorded runtime bounds; it excludes the GPUI spaces header. It is useful
when desktop/window capture is unavailable. It is a native view capture, not an
OS desktop screenshot.

Validated in this branch: 108 Rust tests passed and one existing test ignored.
The broader JavaScript checks pass 17/18; the unchanged input-bridge Escape test
expects one message and receives two. Strict Clippy reports two existing
`collapsible_if` warnings in the pinned-tab mouse handlers in `render.rs`.
Clippy passes with only that lint allowed. Windows native behavior has not been
run in this macOS environment.
