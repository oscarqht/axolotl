# Heroicons Outline

Official 24px Outline SVGs from [Heroicons](https://heroicons.com/), pinned to
[v2.2.0](https://github.com/tailwindlabs/heroicons/tree/v2.2.0/optimized/24/outline).
Original assets are unmodified and distributed under the included MIT license.

`src/icons.rs` embeds these assets for GPUI and expands `__HEROICON_name__`
tokens in app-owned HTML and JavaScript into decorative inline SVGs. Use a
single-quoted JavaScript string or a template literal around tokens because SVG
attributes use double quotes. Icons use `currentColor`, a 24px viewBox and the
original 1.5px Outline stroke. Buttons retain their existing accessible labels,
focus handling and click targets. The start page uses inline copies of
`arrow-up-right.svg` because it can also be loaded directly by the browser host.

Website favicons, user-selected emoji, branding and OS-owned window controls are
outside this asset set. Windows app-drawn window buttons use Heroicons.
