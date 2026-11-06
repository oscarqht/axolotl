# Heroicons Outline validation

Validated on macOS on 2026-11-06. Official Heroicons v2.2.0 24px Outline
assets are bundled locally with their MIT license in
`crates/desktop/assets/heroicons/`. No runtime icon downloads are required.

The main header uses `cog-8-tooth` for Settings and `arrow-down-tray` for
Downloads, both at 20px. Other app-owned action and status glyphs use the same
Outline set across GPUI and embedded panels. Website favicons, branding,
user-selected emoji and macOS traffic lights are preserved.

GPUI icons use the component wrapper to inherit the surrounding text color.
Embedded icons use `currentColor`, decorative accessibility attributes and
non-interactive SVGs within the existing controls. Static templates are expanded
before runtime data is inserted, so page titles and selected emoji remain text.

## Checks

- Default and smoke-feature builds: passed. The default build emits two existing
  dead-code warnings for `Scalar` and `history_scalar` in `persistence_worker.rs`.
- `cargo test --workspace --locked --features smoke`: 134 passed, 1 ignored.
- `cargo clippy --workspace --all-targets --locked --features smoke -- -D warnings`: passed.
- `cargo fmt --all --check` and `git diff --check`: passed.
- Expanded embedded JavaScript syntax checks: passed.
- Native Downloads: attachment bytes and filenames, collision handling,
  concurrent progress, Cancel hit testing and cleanup, failed transfers, blob,
  pinned-page and preview downloads, terminal history and explicit Open errors.
- Native pinned tabs: favicon retention, live page state, emoji picker,
  dismissal, resize grip, overflow, unpin targeting and session round trip.
- Native space editor: emoji search/select/clear, Save/Enter, Escape and outside
  dismissal, title fallback, inactive-space targeting and viewport clamping.
- Native sleep: moon SVG, title opacity, Keep Alive, hover preview, wake and
  auto-sleep behavior.
- Native page tools: find arrows/close, keyboard navigation, counts/highlights,
  split targeting, zoom, CSP, pinned/preview/floating pages.
- Native protection: shield hit testing, on/off state, site exceptions, global
  toggle, request blocking and cosmetic rules.
- Native floating panels: detach, retained documents, move/resize, four edges,
  auto-hide, pin toggle, reorder and merge.
- Native docking: 16 pointer-capture split-drop cases across all four edges.
- Direct native UI clicks opened Settings and Downloads; Settings close worked.
- Native theme probe verified shell colors and captured light/dark icons. The
  probe was stopped after both screenshots, before the rest of the full suite.

Downloads, the space editor and protection had intermittent failures during
initial runs; isolated reruns passed. Native checks used disposable profiles.
Windows window-control mappings were updated but not executed on Windows.

## Existing full-check failures

Both reproduce unchanged on base revision `0c79d46`:

- `--smoke-settings`: `Close button replacement ignored homepage` after the
  earlier dialog, preference and homepage checks pass.
- `--smoke-bar-sizing`: expects the old 254px single-chip width and a gap at most
  12px, while both base and this branch report 286px and a 40px gap with the
  existing protection button.

These assertions were kept unchanged.

## Screenshots

Unretouched native window captures:

- [Header and Downloads](screenshots/heroicons/header-downloads.png)
- [Settings](screenshots/heroicons/settings.png)
- [Light chrome](screenshots/heroicons/light.png)
- [Dark chrome](screenshots/heroicons/dark.png)
