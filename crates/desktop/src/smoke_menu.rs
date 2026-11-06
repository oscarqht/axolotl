//! Native assertions for the tab chip context menu.
use crate::{
    app::BrowserApp,
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
                == serde_json::json!([
                    "Back",
                    "Forward",
                    "Refresh",
                    "Recover Tab…",
                    "Set title…",
                    "Sleep Tab",
                    "Keep Alive",
                    "Pin to header",
                    "Close"
                ])
            && menu["dividers"] == 4
            && menu["top"].as_f64().is_some_and(|t| t >= 0.)
            && menu["right"].as_f64() <= menu["viewport"].as_f64(),
        "Tab context menu has wrong structure or is clipped: {menu}"
    );
    #[cfg(target_os = "macos")]
    if let Some(path) = std::env::args().find_map(|arg| {
        arg.strip_prefix("--smoke-menu-snapshot=")
            .map(str::to_owned)
    }) {
        // Show the action enabled on a loaded page in the native proof, while
        // retaining the unloaded-background-target assertions below.
        bar_eval(view, pane, "document.querySelector('.chip.active').dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:60}));true", cx).await?;
        pause(cx).await;
        pause(cx).await;
        ensure!(bar_eval(view, pane, "[...document.querySelectorAll('#menu button')].find(b=>b.textContent==='Recover Tab…').disabled", cx).await? == false, "Loaded page cannot be recovered from its menu");
        let previous_snapshot = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        view.update_in(cx, |app, _, _| {
            app.bars[&pane].native_snapshot(path.clone())
        })?;
        for _ in 0..15 {
            if std::fs::metadata(&path)
                .is_ok_and(|m| m.len() > 0 && m.modified().ok() != previous_snapshot)
            {
                break;
            }
            pause(cx).await;
        }
        ensure!(
            std::fs::metadata(&path)
                .is_ok_and(|m| m.len() > 0 && m.modified().ok() != previous_snapshot),
            "Native tab-menu snapshot missing"
        );
        bar_eval(view, pane, OPEN, cx).await?;
        pause(cx).await;
    }
    // The lazily-created tab has no page history or view to act on.
    ensure!(
        menu["disabled"]
            == serde_json::json!([true, true, true, true, false, false, false, false, false]),
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
    Ok(
        "tab chip context menu: grouped items, targets the right-clicked inactive tab, disabled history items, set/clear fixed title, outside-click dismissal, close",
    )
}
