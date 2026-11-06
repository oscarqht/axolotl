# Custom icon replacement

Replaced the 24 bundled Heroicons with the user-supplied SVGs from
`/Users/tangqh/Downloads/icons` on 2026-11-06. Assets live in
`crates/desktop/assets/icons`; native controls and embedded surfaces share
`src/icons.rs`. Start-page links use the same bundled arrow, including Windows
protection-triggered navigation. No runtime icon downloads are needed.

The SVG geometry is preserved except for the requested rotations:
`arrow-down-tag` is rotated 90 degrees for a vertical handle, and
`nav-arrow-down-solid` is rotated -45 degrees toward the bottom-right resize
corner. Fixed black colors become `currentColor`; XML declarations and fixed
dimensions are removed for inline embedding.

## Validation

- Smoke-feature build and workspace tests passed.
- Clippy with all targets, smoke feature, and warnings denied passed.
- Rust formatting, diff whitespace, SVG XML, icon-reference resolution, and
  expanded embedded JavaScript syntax checks passed.
- Native sleep, find/page tools, pinned tabs, protection, and floating-panel
  smoke checks passed using disposable profiles. These exercise native
  views programmatically; OS mouse/keyboard input is not simulated.
- Windows control mappings were inspected; Windows execution was unavailable.

## Native screenshots

Fresh, unretouched macOS window captures from a disposable smoke profile:

- [Light chrome](screenshots/custom-icons/light.png)
- [Dark chrome](screenshots/custom-icons/dark.png)

The theme probe was stopped after the light/dark captures. The start page uses
its existing light palette in both appearances.

## Chips-bar handle follow-up

The chips-bar drag handle now uses the attached `dots-grid3x3.svg`, without
rotation. Its nine dots inherit the current theme color. The floating-panel
drag grip keeps `arrow-down-tag`. The smoke-feature build and embedded-icon
resolution test passed after this update.

- [Updated chips-bar handle](screenshots/custom-icons/chips-bar-handle.png)
