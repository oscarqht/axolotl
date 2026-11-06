//! Native reload routing is independent of the page JavaScript event loop.
use crate::app::BrowserApp;
use gpui::Context;

#[cfg(any(target_os = "macos", test))]
pub(crate) fn shortcut(key: &str, command: bool, other_modifiers: bool) -> bool {
    command && !other_modifiers && key.eq_ignore_ascii_case("r")
}

impl BrowserApp {
    pub(crate) fn install_reload_shortcut(cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        gpui::App::on_action(cx, move |_: &crate::Reload, cx| {
            let view = view.clone();
            cx.defer(move |cx| {
                let _ = view.update(cx, |app, _| app.reload_page());
            });
        });
    }

    pub(crate) fn reload_page(&mut self) {
        if self.settings.is_some() || self.recovery_prompt {
            return;
        }
        let fallback = if self
            .link_popup
            .as_ref()
            .is_some_and(|popup| popup.closing_until.is_none())
        {
            crate::link_popup::POPUP_TAB
        } else {
            self.pins.open.unwrap_or_else(|| self.active_tab())
        };
        // Native focus can precede the workspace's queued page-focus message.
        let tab = self.host.focused_page().unwrap_or(fallback);
        self.report(self.host.reload(tab));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unmodified_command_r_reloads() {
        assert!(shortcut("r", true, false));
        assert!(shortcut("R", true, false));
        assert!(!shortcut("r", false, false));
        assert!(!shortcut("r", true, true));
        for key in ["", "reload", "q", "f"] {
            assert!(!shortcut(key, true, false));
        }
    }
}
