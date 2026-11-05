//! Shell colors selected from the OS-synchronized component theme.
use gpui::App;
use gpui_component::ActiveTheme;

#[derive(Clone, Copy)]
pub struct ChromeColors {
    pub ink: u32,
    pub surface: u32,
    pub muted: u32,
    pub accent: u32,
    pub active_tab: u32,
    pub border: u32,
    pub hover: u32,
    pub shell: u32,
    pub divider: u32,
    pub page: u32,
}

impl ChromeColors {
    pub fn current(cx: &App) -> Self {
        if cx.theme().is_dark() {
            Self {
                ink: 0xdde5ed,
                surface: 0x181e27,
                muted: 0x8491a2,
                accent: 0x8fddc3,
                active_tab: 0x303b49,
                border: 0x303845,
                hover: 0x465366,
                shell: 0x11161e,
                divider: 0x11161e,
                page: 0x20252d,
            }
        } else {
            Self {
                ink: 0x243240,
                surface: 0xf3f5f7,
                muted: 0x5e6d7b,
                accent: 0x267c65,
                active_tab: 0xffffff,
                border: 0xc9d2dc,
                hover: 0xdae2e9,
                shell: 0xe9edf1,
                divider: 0xe9edf1,
                page: 0xf5f6f8,
            }
        }
    }
}
