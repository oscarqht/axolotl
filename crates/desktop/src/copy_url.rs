//! Copy the live page URL without changing focus or an unfinished address edit.
use crate::app::BrowserApp;
use gpui::{ClipboardItem, Context};

impl BrowserApp {
    pub(crate) fn install_copy_url(cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        // WebKit focus can leave GPUI's chrome focus node unmounted, so listen
        // at app level just like the native space shortcuts.
        gpui::App::on_action(cx, move |_: &crate::CopyUrl, cx| {
            let view = view.clone();
            cx.defer(move |cx| {
                let _ = view.update(cx, |app, cx| {
                    let tab = if app
                        .link_popup
                        .as_ref()
                        .is_some_and(|popup| popup.closing_until.is_none())
                    {
                        crate::link_popup::POPUP_TAB
                    } else {
                        app.pins.open.unwrap_or_else(|| app.active_tab())
                    };
                    // Read WebKit directly: model metadata may lag a navigation,
                    // and the address input can contain an unsubmitted draft.
                    // With no committed document, leave the clipboard untouched.
                    if let Ok(url) = app.host.current_url(tab) {
                        cx.write_to_clipboard(ClipboardItem::new_string(url));
                        if let Err(error) = app.host.show_copy_url_toast(tab) {
                            eprintln!("URL copy toast: {error:#}");
                        }
                    }
                });
            });
        });
    }
}
