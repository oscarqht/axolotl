//! Regression checks for recycled native controls and asynchronous suggestions.
use crate::{
    app::{BrowserApp, Command, TabTarget},
    smoke::{bar_eval, pause},
};
use anyhow::{Result, ensure};
use gpui::*;

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (space, pane) = view.update_in(cx, |app, _, _| {
        (app.model.active_space, app.model.focused_pane().id)
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    bar_eval(view, pane, "window.controlIdentity='retained';true", cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.command(Command::NewTab, window, cx)
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    ensure!(
        bar_eval(view, pane, "window.controlIdentity", cx).await? == "retained",
        "new page recreated floating controls"
    );
    let other = view.update_in(cx, |app, window, cx| {
        app.cancel_edit(window, cx);
        let other = app.model.add_space("Recycling test", "about:blank");
        app.select_target(TabTarget::Space(other), window, cx);
        app.model.focused_pane().id
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    ensure!(
        bar_eval(
            view,
            other,
            "({identity:window.controlIdentity,pane:state.pane,expanded,menu,renaming})",
            cx
        )
        .await?
            == serde_json::json!({"identity":"retained","pane":other,"expanded":false,"menu":null,"renaming":null}),
        "recycled control retained old pane state"
    );
    let before = view.update_in(cx, |app, _, _| app.model.to_json())?;
    bar_eval(
        view,
        other,
        &format!("window.ipc.postMessage(JSON.stringify({{type:'new',pane:{pane}}}));true"),
        cx,
    )
    .await?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.model.to_json())? == before,
        "old pane IPC changed the newly assigned pane"
    );
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Space(space), window, cx);
        let tab = app.active_tab();
        app.begin_edit(TabTarget::Page(pane, tab), window, cx);
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    bar_eval(
        view,
        pane,
        "document.querySelector('input').value='unfinished draft';true",
        cx,
    )
    .await?;
    view.update_in(cx, |app, _, cx| {
        app.model
            .split_focused(workspace_core::Axis::Horizontal, "about:blank");
        cx.notify();
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    let editor = bar_eval(view, pane, "({identity:window.controlIdentity,draft:document.querySelector('input')?.value,focused:document.activeElement===document.querySelector('input')})", cx).await?;
    ensure!(
        editor
            == serde_json::json!({"identity":"retained","draft":"unfinished draft","focused":true}),
        "raising controls lost editor or focus: {editor}"
    );

    view.update_in(cx, |app, window, cx| {
        app.cancel_edit(window, cx);
        let target = TabTarget::Space(app.model.active_space);
        app.begin_edit(target, window, cx);
        app.search_history("old result", cx);
        app.cancel_edit(window, cx);
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    ensure!(
        view.update_in(cx, |app, _, _| app.history_matches.is_empty()
            && app.history_popup.is_none()
            && app.editing.is_none())?,
        "cancelled history query resurrected suggestions"
    );
    view.update_in(cx, |app, window, cx| {
        let target = TabTarget::Space(app.model.active_space);
        app.begin_edit(target, window, cx);
        app.search_history("old result", cx);
        app.invalidate_history_suggestions(cx);
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    ensure!(
        view.update_in(cx, |app, _, _| app.history_matches.is_empty()
            && app.history_popup.is_none())?,
        "invalidated history query resurrected suggestions"
    );
    Ok(vec![
        "new pages retain floating-control document; space switching recycles bounded surfaces and clears transient state",
        "queued IPC from a recycled control's old pane is ignored",
        "creating a native sibling preserves address draft, focus and control identity",
        "cancelled and invalidated asynchronous history queries cannot reopen suggestions",
    ])
}
