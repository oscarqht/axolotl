//! Locally bundled user-supplied SVG icons.
use gpui::{prelude::*, *};
use std::borrow::Cow;

const ICONS: &[(&str, &[u8])] = &[
    (
        "dots-grid3x3",
        include_bytes!("../assets/icons/dots-grid3x3.svg"),
    ),
    (
        "arrow-down",
        include_bytes!("../assets/icons/arrow-down.svg"),
    ),
    ("download", include_bytes!("../assets/icons/download.svg")),
    (
        "shield-loading",
        include_bytes!("../assets/icons/shield-loading.svg"),
    ),
    ("arrow-up", include_bytes!("../assets/icons/arrow-up.svg")),
    (
        "arrow-up-right",
        include_bytes!("../assets/icons/arrow-up-right.svg"),
    ),
    (
        "nav-arrow-down-solid",
        include_bytes!("../assets/icons/nav-arrow-down-solid.svg"),
    ),
    (
        "arrow-down-tag",
        include_bytes!("../assets/icons/arrow-down-tag.svg"),
    ),
    ("pin", include_bytes!("../assets/icons/pin.svg")),
    ("pin-slash", include_bytes!("../assets/icons/pin-slash.svg")),
    ("check", include_bytes!("../assets/icons/check.svg")),
    (
        "arrow-left",
        include_bytes!("../assets/icons/arrow-left.svg"),
    ),
    (
        "arrow-right",
        include_bytes!("../assets/icons/arrow-right.svg"),
    ),
    ("settings", include_bytes!("../assets/icons/settings.svg")),
    (
        "more-horiz",
        include_bytes!("../assets/icons/more-horiz.svg"),
    ),
    (
        "emoji-talking-happy",
        include_bytes!("../assets/icons/emoji-talking-happy.svg"),
    ),
    (
        "bright-star",
        include_bytes!("../assets/icons/bright-star.svg"),
    ),
    ("minus", include_bytes!("../assets/icons/minus.svg")),
    ("half-moon", include_bytes!("../assets/icons/half-moon.svg")),
    ("plus", include_bytes!("../assets/icons/plus.svg")),
    (
        "shield-check",
        include_bytes!("../assets/icons/shield-check.svg"),
    ),
    (
        "shield-xmark",
        include_bytes!("../assets/icons/shield-xmark.svg"),
    ),
    (
        "arrow-up-right-square",
        include_bytes!("../assets/icons/arrow-up-right-square.svg"),
    ),
    ("maximize", include_bytes!("../assets/icons/maximize.svg")),
    ("copy", include_bytes!("../assets/icons/copy.svg")),
    ("xmark", include_bytes!("../assets/icons/xmark.svg")),
];

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| path == format!("icons/{name}.svg"))
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .map(|(name, _)| format!("icons/{name}.svg"))
            .filter(|name| name.starts_with(path))
            .map(Into::into)
            .collect())
    }
}

/// GPUI paints SVG alpha with the inherited text color.
pub fn icon(name: &'static str, size: f32) -> gpui_component::Icon {
    gpui_component::Icon::empty()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_shrink_0()
}

/// Expand only app-owned documents/scripts; website content is never processed.
/// Inline SVGs inherit currentColor and do not need network or filesystem access.
pub fn html(source: &str) -> String {
    let mut result = source.to_owned();
    for (name, bytes) in ICONS {
        let token = format!("__ICON_{name}__");
        if result.contains(&token) {
            let svg = std::str::from_utf8(bytes).expect("bundled SVG is UTF-8")
                .replace(" aria-hidden=\"true\"", "")
                .replace("<svg ", &format!("<svg data-icon=\"{name}\" aria-hidden=\"true\" focusable=\"false\" width=\"18\" height=\"18\" style=\"display:inline-block;vertical-align:middle;flex-shrink:0;pointer-events:none\" "))
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
            include_str!("../assets/start.html"),
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
                !rendered.contains("__ICON_"),
                "Unresolved icon in embedded surface"
            );
            assert!(rendered.contains("currentColor"));
            assert!(rendered.contains("focusable=\"false\""));
        }
    }
}
