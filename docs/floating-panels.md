# Floating panels

Drag a tab chip or stack handle within the tiled layout to split or merge.
Broad panel edge regions create splits (green preview); the center merges tabs.
Only a drop in the workspace's outer 22-point edge region detaches a new overlay
(purple preview). Existing floating panels can still be freely moved using their
header, or returned to a tiled split using its 28-point edge target. Tab bars
remain merge destinations. The last tiled pane leaves a fresh start pane when
detached. Moving keeps native tab IDs, documents and navigation histories.

Floating pages meet the header with square top corners. Their outer bottom
corners follow the rounded panel outline. A two-point frame inset protects every
border from the native webpage. Native floating pages use those window bounds
directly, avoiding independent canvas rounding that can cover the right border.
GPUI fallback content uses the same bottom
corner mask. A small resize triangle overlays the page's bottom-right corner;
there is no resize footer. Moving back to a tiled pane restores its usual mask.

Each docked panel has a title/favicon chip anchored at its saved docking position.
Left and right chips are vertical; top and bottom chips are horizontal. If chips
overlap, the nearest available offset is used, with inward lanes for overflow.
Hover reveals the page beside that chip. The panel follows the resolved chip
anchor, shifting only as needed to stay in the visible workspace. The chip stays
outside the page so its header controls remain accessible.

Leaving both chip and panel hides it after a 400 ms grace period. One unpinned
panel reveals per edge. The diamond button pins it open. Address editing, menus,
dragging and resizing keep it open. Dragging a chip onto another on its edge
swaps their anchors and reorders them. Dragging away undocks it.

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
cargo +1.99.1 test --workspace --locked
cargo +1.99.1 run -p axolotl --features smoke --locked -- --smoke-test --smoke-floating
python3 scripts/test-floating-session.py target/debug/axolotl
```

The native smoke delivers AppKit events through WebKit pointer capture for tab
and whole-stack detachment, header movement, resizing, all four window edges,
chip ordering, and split/merge return paths. It checks live document identity and
form state, native visibility, pinning, space isolation, and fitting after window
resize. Hover is invoked through the chip's handler; leave checks hold the fixture pointer
outside because synthesized NSEvents do not move the OS cursor. This is a programmatic
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

To verify outer-corner transparency against a contrasting live background:

```sh
python3 scripts/capture-floating-panels.py target/debug/axolotl /tmp/floating.png --contrast-corners
```

This also saves `/tmp/floating-corners.png` with unretouched enlarged corner crops.
The regression checks the native frame's alpha outside each curve, then compares
all four combined corners with the captured backdrop (allowing one 8-bit level
for native color-profile dithering). The frame uses both HTML and native rounded
clips, with no rectangular viewport-clipped shadow.

This requires Pillow. It captures native views and combines transparent surfaces
at their recorded runtime bounds; it excludes the GPUI spaces header. It is useful
when desktop/window capture is unavailable. It is a native view capture, not an
OS desktop screenshot.

Validation for the current behavior: 121 Rust tests passed and one existing test
is ignored. The native check covers split-first content drops, edge-only
creation, anchored chip positions, docked resize anchors, square header joins, rounded outer bottom masks and
the overlay resize triangle. Separate native processes verify SQLite restart
restoration. These checks used the installed Rust 1.99.1 toolchain.
Windows native behavior has not been run in this macOS environment.
