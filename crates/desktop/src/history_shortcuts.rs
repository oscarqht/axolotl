//! History shortcuts work with native page and shell focus.
use crate::app::BrowserApp;
use gpui::Context;

#[cfg(any(target_os = "macos", test))]
pub fn shortcut(key: &str, command: bool, other_modifiers: bool) -> Option<bool> {
    if !command || other_modifiers {
        return None;
    }
    match key {
        "[" => Some(false),
        "]" => Some(true),
        _ => None,
    }
}

impl BrowserApp {
    pub(crate) fn install_history_shortcuts(cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        for forward in [false, true] {
            let view = view.clone();
            let handler = move |cx: &mut gpui::App| {
                let view = view.clone();
                cx.defer(move |cx| {
                    let _ = view.update(cx, |app, cx| {
                        app.navigate_history(forward);
                        cx.notify();
                    });
                });
            };
            if forward {
                gpui::App::on_action(cx, move |_: &crate::Forward, cx| handler(cx));
            } else {
                gpui::App::on_action(cx, move |_: &crate::Back, cx| handler(cx));
            }
        }
    }

    pub(crate) fn navigate_history(&mut self, forward: bool) {
        if self.settings.is_some() {
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
        // Native focus can move before pane-focus IPC is processed.
        let tab = self.host.focused_page().unwrap_or(fallback);
        let result = if forward {
            self.host.forward(tab)
        } else {
            self.host.back(tab)
        };
        self.report(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_command_brackets_navigate_history() {
        for (key, forward) in [("[", false), ("]", true)] {
            assert_eq!(shortcut(key, true, false), Some(forward));
            assert_eq!(shortcut(key, false, false), None);
            assert_eq!(shortcut(key, true, true), None);
            assert_eq!(shortcut(key, false, true), None);
        }
        for key in ["", "{", "}", "left", "right", "a"] {
            assert_eq!(shortcut(key, true, false), None);
        }
    }
}
