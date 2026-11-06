//! Explicit recovery replaces a page only after the user accepts losing its state.
use crate::app::BrowserApp;
use anyhow::{Context as _, Result, ensure};
use gpui::{Context, PromptLevel, Window};
use workspace_core::TabId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecoveryTarget {
    pub(crate) tab: TabId,
    url: String,
}

impl RecoveryTarget {
    fn matches(&self, current: Option<&str>) -> bool {
        current == Some(self.url.as_str())
    }
}

impl BrowserApp {
    fn recovery_target(&self, id: TabId) -> Result<RecoveryTarget> {
        let tab = self.model.tab(id).context("This tab has closed")?;
        ensure!(
            crate::browser::allowed_url(&tab.url),
            "This tab has no recoverable URL"
        );
        ensure!(
            self.host.has_view(id),
            "This tab has no running page to recover"
        );
        ensure!(
            !self.host.downloads().borrow().busy(),
            "Wait for active downloads to finish before recovering a tab"
        );
        #[cfg(target_os = "macos")]
        ensure!(
            !self.native_popup_sources.contains_key(&id)
                && !self
                    .native_popup_sources
                    .values()
                    .any(|source| *source == id),
            "Close this tab's sign-in popup before recovering it"
        );
        Ok(RecoveryTarget {
            tab: id,
            url: tab.url.clone(),
        })
    }

    pub(crate) fn confirm_tab_recovery(
        &mut self,
        id: TabId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.recovery_prompt {
            return;
        }
        let target = match self.recovery_target(id) {
            Ok(target) => target,
            Err(error) => {
                self.report(Err(error));
                cx.notify();
                return;
            }
        };
        self.recovery_prompt = true;
        let response = window.prompt(PromptLevel::Warning, "Recover this tab?",
            Some("Recreate this page from its saved URL if Refresh does not work. Unsaved changes and this tab's Back/Forward history will be lost."),
            &["Cancel", "Recreate Tab"], cx);
        let view = cx.entity().downgrade();
        window
            .spawn(cx, async move |cx| {
                let response = response.await;
                #[cfg(feature = "smoke")]
                eprintln!("Recovery: native warning response {response:?}");
                let accepted = response == Ok(1);
                let _ = view.update_in(cx, |app, window, cx| {
                    app.recovery_prompt = false;
                    if accepted {
                        let result = app.recreate_tab(&target, window, cx);
                        app.report(result);
                    }
                    cx.notify();
                });
            })
            .detach();
    }

    /// Called only after the explicit confirmation, or by isolated native smoke tests.
    pub(crate) fn recreate_tab(
        &mut self,
        target: &RecoveryTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        ensure!(
            target.matches(self.model.tab(target.tab).map(|tab| tab.url.as_str())),
            "The tab changed while recovery was awaiting confirmation; recovery was cancelled"
        );
        // Recheck protections: a download or auth popup can start while the prompt is open.
        self.recovery_target(target.tab)?;
        let id = target.tab;
        #[cfg(feature = "smoke")]
        eprintln!("Recovery: replacing native tab {id}");
        let visible = self.pins.open == Some(id)
            || self
                .model
                .active_space()
                .panes
                .iter()
                .any(|pane| pane.active_tab == id);
        let previous_focus = self.host.focused_page();
        if visible {
            self.host.focus_parent(id)?;
            self.focus.focus(window);
        }
        // Never evaluate JavaScript or ask the unresponsive engine for its URL.
        self.host.unload(id);
        self.wake_tab(id);
        self.history.remove(&id);
        if visible {
            self.pending_page_focus = Some(id);
        }
        self.host.ensure(id, &target.url, window, cx)?;
        // macOS child construction briefly returns focus to GPUI even for a
        // hidden view. Restore the foreground page when recovering a background tab.
        if !visible && let Some(previous) = previous_focus {
            self.host.focus(previous)?;
        }
        self.host.request_redraw();
        cx.notify();
        Ok(())
    }

    #[cfg(all(feature = "smoke", target_os = "macos"))]
    pub(crate) fn test_recovery_target(&self, id: TabId) -> Result<RecoveryTarget> {
        self.recovery_target(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_never_applies_to_closed_or_navigated_tab() {
        let target = RecoveryTarget {
            tab: 1,
            url: "https://example.com/board".into(),
        };
        assert!(target.matches(Some("https://example.com/board")));
        assert!(!target.matches(None));
        assert!(!target.matches(Some("https://example.com/another-board")));
        assert!(!target.matches(Some("https://example.com/board?changed")));
    }
}
