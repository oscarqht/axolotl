//! Native assertions for the tab chip context menu.
use crate::{
    app::{BrowserApp, Command, TabAction, TabTarget},
    smoke::{bar_eval, pause},
};
use anyhow::{Result, ensure};
use gpui::*;
use workspace_core::PaneId;

const OPEN: &str = "(()=>{const c=document.querySelector('.chip:not(.active)');c.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:60}));return Number(c.dataset.id)})()";
const SNAPSHOT: &str = "(()=>{const m=document.querySelector('#menu'),r=m.getBoundingClientRect();return {hidden:m.hidden,items:[...m.querySelectorAll('button')].map(b=>b.textContent),disabled:[...m.querySelectorAll('button')].map(b=>b.disabled),dividers:m.querySelectorAll('hr').length,top:r.top,right:r.right,viewport:innerWidth,height:innerHeight}})()";

async fn press(
    view: &WeakEntity<BrowserApp>,
    pane: PaneId,
    label: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    bar_eval(view,pane,&format!("[...document.querySelectorAll('#menu button')].find(b=>b.textContent==={label:?}).click(),true"),cx).await?;
    pause(cx).await;
    Ok(())
}

async fn set_title(
    view: &WeakEntity<BrowserApp>,
    pane: PaneId,
    value: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    bar_eval(view,pane,&format!("(()=>{{const i=document.querySelector('.chip.renaming input');i.value={value:?};i.dispatchEvent(new KeyboardEvent('keydown',{{key:'Enter',bubbles:true}}));return true}})()"),cx).await?;
    pause(cx).await;
    Ok(())
}

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    pane: PaneId,
    cx: &mut AsyncWindowContext,
) -> Result<&'static str> {
    // A second, never-activated tab: the menu must act on it, not the active tab.
    let (active, other) = view.update_in(cx, |app, _, _| {
        let active = app.model.focused_pane().active_tab;
        let other = app.model.add_tab("about:blank");
        app.model.switch_tab(pane, active);
        (active, other)
    })?;
    bar_eval(view,pane,"document.querySelector('#count').dispatchEvent(new PointerEvent('pointerenter',{pointerType:'mouse'}));true",cx).await?;
    pause(cx).await;
    let opened = bar_eval(view, pane, OPEN, cx).await?;
    ensure!(
        opened == other,
        "Context menu targeted {opened}, not {other}"
    );
    pause(cx).await;
    pause(cx).await;
    let menu = bar_eval(view, pane, SNAPSHOT, cx).await?;
    ensure!(
        menu["hidden"] == false
            && menu["items"]
                == serde_json::json!(["Back", "Forward", "Refresh", "Set title…", "Close"])
            && menu["dividers"] == 2
            && menu["top"].as_f64().is_some_and(|t| t >= 0.)
            && menu["right"].as_f64() <= menu["viewport"].as_f64(),
        "Tab context menu has wrong structure or is clipped: {menu}"
    );
    // The lazily-created tab has no page history or view to act on.
    ensure!(
        menu["disabled"] == serde_json::json!([true, true, true, false, false]),
        "Back/Forward/Refresh must be disabled without history: {menu}"
    );
    view.update_in(cx, |app, _, _| {
        ensure!(
            app.bars[&pane].menu_open && app.model.focused_pane().active_tab == active,
            "Opening the menu must not activate the tab"
        );
        Ok(())
    })??;

    // Outside click dismisses and the surface returns to its normal height.
    bar_eval(
        view,
        pane,
        "document.body.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));true",
        cx,
    )
    .await?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| !app.bars[&pane].menu_open
            && app.bars[&pane].bounds.height <= 48.)?,
        "Outside click did not dismiss the menu"
    );

    // Set a fixed title; page title updates must not replace it.
    bar_eval(view, pane, OPEN, cx).await?;
    pause(cx).await;
    pause(cx).await;
    press(view, pane, "Set title…", cx).await?;
    set_title(view, pane, "  Fixed  ", cx).await?;
    view.update_in(cx, |app, _, _| {
        app.model.tab_mut(other).unwrap().title = "Page".into();
        ensure!(
            app.model.tab(other).unwrap().display_title() == "Fixed"
                && app.model.focused_pane().active_tab == active
                && !app.bars[&pane].menu_open,
            "Set title did not apply to the right-clicked tab"
        );
        Ok(())
    })??;
    pause(cx).await;
    let label = bar_eval(
        view,
        pane,
        &format!("document.querySelector('.chip[data-id=\"{other}\"] .label')?.textContent"),
        cx,
    )
    .await?;
    ensure!(label == "Fixed", "Chip label ignored custom title: {label}");

    // An empty title clears it, returning to the page's title.
    bar_eval(view, pane, OPEN, cx).await?;
    pause(cx).await;
    pause(cx).await;
    press(view, pane, "Set title…", cx).await?;
    set_title(view, pane, "", cx).await?;
    pause(cx).await;
    let label = bar_eval(
        view,
        pane,
        &format!("document.querySelector('.chip[data-id=\"{other}\"] .label')?.textContent"),
        cx,
    )
    .await?;
    ensure!(
        label == "Page",
        "Clearing the title did not restore it: {label}"
    );

    // Close acts on the right-clicked tab, leaving the active one selected.
    bar_eval(view, pane, OPEN, cx).await?;
    pause(cx).await;
    pause(cx).await;
    press(view, pane, "Close", cx).await?;
    view.update_in(cx, |app, _, _| {
        ensure!(
            app.model.tab(other).is_none()
                && app.model.focused_pane().active_tab == active
                && !app.bars[&pane].menu_open,
            "Close did not remove only the right-clicked tab"
        );
        Ok(())
    })??;
    bar_eval(
        view,
        pane,
        "pinned=false;document.activeElement?.blur();expand(false);true",
        cx,
    )
    .await?;
    pause(cx).await;
    space_menu(view, cx).await?;
    Ok(
        "tab chip context menu: grouped items, space tab menu, targets the right-clicked inactive tab, disabled history items, set/clear fixed title, outside-click dismissal, close",
    )
}

/// Right-click menu on a space tab: one "Set title…" item acting on the clicked space.
async fn space_menu(view: &WeakEntity<BrowserApp>, cx: &mut AsyncWindowContext) -> Result<()> {
    let (active, other) = view.update_in(cx, |app, window, cx| {
        let active = app.model.active_space;
        app.command(Command::NewSpace, window, cx);
        let other = app.model.active_space;
        app.select_target(TabTarget::Space(active), window, cx);
        (active, other)
    })?;
    ensure!(active != other, "Second space was not created");
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        app.open_space_menu(other, point(px(100.), px(30.)), window, cx);
        ensure!(
            app.space_menu.map(|m| m.0) == Some(other) && app.model.active_space == active,
            "Space menu must target the right-clicked space without activating it"
        );
        Ok(())
    })??;
    pause(cx).await;
    cx.update(|window, cx| window.dispatch_action(Box::new(gpui_component::input::Escape), cx))?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.space_menu.is_none())?,
        "Escape did not dismiss the space menu"
    );
    view.update_in(cx, |app, window, cx| {
        app.open_space_menu(other, point(px(100.), px(30.)), window, cx);
        app.close_space_menu(cx);
        ensure!(app.space_menu.is_none(), "Outside click did not dismiss");
        app.open_space_menu(other, point(px(100.), px(30.)), window, cx);
        // "Set title…" runs the same flow as double-click.
        app.begin_edit(TabTarget::Space(other), window, cx);
        ensure!(
            app.space_menu.is_none() && app.editing == Some(TabTarget::Space(other)),
            "Set title did not start renaming the right-clicked space"
        );
        app.cancel_edit(window, cx);
        app.tab_action(TabTarget::Space(other), TabAction::Close, window, cx);
        ensure!(
            app.model.active_space == active && app.model.spaces.len() == 1,
            "Cleanup of the extra space left state behind"
        );
        Ok(())
    })??;
    Ok(())
}
