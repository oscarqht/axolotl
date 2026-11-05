//! Select spaces by their current visible order, through one app-level action.
use crate::app::{BrowserApp, TabTarget};
use gpui::{Context, Window};

impl BrowserApp {
    pub(crate) fn install_space_shortcuts(window: &Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        let handle = window.window_handle();
        // Native web views can leave GPUI's focused input unmounted. An app-level
        // listener reaches this window even when no chrome focus node remains.
        gpui::App::on_action(cx, move |action: &crate::SelectSpace, cx| {
            let view = view.clone();
            let index = action.index;
            // Action dispatch may already borrow the window; update next turn.
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = view.update(cx, |app, cx| app.select_space_index(index, window, cx));
                });
            });
        });
    }
    pub(crate) fn select_space_index(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Missing indices leave editors, popups and the selection untouched.
        let Some(space) = self.model.spaces.get(index).map(|space| space.id) else {
            return;
        };
        self.close_link_popup(window, cx);
        self.close_settings(window, cx);
        self.close_space_menu(cx);
        self.cancel_tab_drag(cx);
        self.cancel_space_drag(cx);
        self.dragging = None;
        self.moving_window = false;
        self.cancel_edit(window, cx);
        self.select_target(TabTarget::Space(space), window, cx);
        // Lazy page creation and native control reconstruction may return focus
        // to the shell; focus the selected page after all siblings are prepared.
        self.pending_page_focus = Some(self.active_tab());
        self.host.request_redraw();
        cx.notify();
    }
}
