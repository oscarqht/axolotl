//! Locally bundled Heroicons v2.2.0, 24px Outline (MIT).
use gpui::{prelude::*, *};
use std::borrow::Cow;

const ICONS: &[(&str, &[u8])] = &[
    (
        "arrow-down",
        include_bytes!("../assets/heroicons/arrow-down.svg"),
    ),
    (
        "arrow-down-tray",
        include_bytes!("../assets/heroicons/arrow-down-tray.svg"),
    ),
    (
        "arrow-path",
        include_bytes!("../assets/heroicons/arrow-path.svg"),
    ),
    (
        "arrow-up",
        include_bytes!("../assets/heroicons/arrow-up.svg"),
    ),
    (
        "arrow-up-right",
        include_bytes!("../assets/heroicons/arrow-up-right.svg"),
    ),
    (
        "arrows-pointing-out",
        include_bytes!("../assets/heroicons/arrows-pointing-out.svg"),
    ),
    ("bars-3", include_bytes!("../assets/heroicons/bars-3.svg")),
    (
        "bookmark",
        include_bytes!("../assets/heroicons/bookmark.svg"),
    ),
    (
        "bookmark-slash",
        include_bytes!("../assets/heroicons/bookmark-slash.svg"),
    ),
    ("check", include_bytes!("../assets/heroicons/check.svg")),
    (
        "chevron-left",
        include_bytes!("../assets/heroicons/chevron-left.svg"),
    ),
    (
        "chevron-right",
        include_bytes!("../assets/heroicons/chevron-right.svg"),
    ),
    (
        "cog-8-tooth",
        include_bytes!("../assets/heroicons/cog-8-tooth.svg"),
    ),
    (
        "ellipsis-horizontal",
        include_bytes!("../assets/heroicons/ellipsis-horizontal.svg"),
    ),
    (
        "face-smile",
        include_bytes!("../assets/heroicons/face-smile.svg"),
    ),
    (
        "globe-alt",
        include_bytes!("../assets/heroicons/globe-alt.svg"),
    ),
    ("minus", include_bytes!("../assets/heroicons/minus.svg")),
    ("moon", include_bytes!("../assets/heroicons/moon.svg")),
    ("plus", include_bytes!("../assets/heroicons/plus.svg")),
    (
        "shield-check",
        include_bytes!("../assets/heroicons/shield-check.svg"),
    ),
    (
        "shield-exclamation",
        include_bytes!("../assets/heroicons/shield-exclamation.svg"),
    ),
    (
        "square-2-stack",
        include_bytes!("../assets/heroicons/square-2-stack.svg"),
    ),
    ("stop", include_bytes!("../assets/heroicons/stop.svg")),
    ("x-mark", include_bytes!("../assets/heroicons/x-mark.svg")),
];

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| path == format!("heroicons/{name}.svg"))
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .map(|(name, _)| format!("heroicons/{name}.svg"))
            .filter(|name| name.starts_with(path))
            .map(Into::into)
            .collect())
    }
}

/// GPUI paints SVG alpha with the inherited text color.
pub fn icon(name: &'static str, size: f32) -> gpui_component::Icon {
    gpui_component::Icon::empty()
        .path(format!("heroicons/{name}.svg"))
        .size(px(size))
        .flex_shrink_0()
}

/// Expand only app-owned documents/scripts; website content is never processed.
/// Inline SVGs inherit currentColor and do not need network or filesystem access.
pub fn html(source: &str) -> String {
    let mut result = source.to_owned();
    for (name, bytes) in ICONS {
        let token = format!("__HEROICON_{name}__");
        if result.contains(&token) {
            let svg = std::str::from_utf8(bytes).expect("bundled SVG is UTF-8")
                .replace(" aria-hidden=\"true\"", "")
                .replace("<svg ", &format!("<svg data-heroicon=\"{name}\" aria-hidden=\"true\" focusable=\"false\" width=\"18\" height=\"18\" style=\"display:inline-block;vertical-align:middle;flex-shrink:0;pointer-events:none\" "))
                .replace('\n', " ");
            result = result.replace(&token, &svg);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::html;

    #[test]
    fn embedded_surfaces_resolve_every_icon() {
        for source in [
            include_str!("../assets/floating-bar.html"),
            include_str!("../assets/floating-bar.js"),
            include_str!("../assets/floating-panel.html"),
            include_str!("../assets/settings.html"),
            include_str!("../assets/link-popup.html"),
            include_str!("../assets/pinned-popup.html"),
            include_str!("../assets/pinned-resize.html"),
            include_str!("../assets/space-menu.html"),
            include_str!("../assets/page-find.js"),
        ] {
            let rendered = html(source);
            assert!(
                !rendered.contains("__HEROICON_"),
                "Unresolved icon in embedded surface"
            );
            assert!(rendered.contains("stroke-width=\"1.5\""));
            assert!(rendered.contains("focusable=\"false\""));
        }
    }
}
