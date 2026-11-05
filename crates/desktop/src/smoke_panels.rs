//! Native floating-control and whole-panel integration assertions.
use crate::{
    app::BrowserApp,
    smoke::{Host, bar_eval, eval, pause},
};
use anyhow::{Result, ensure};
use gpui::*;
use workspace_core::{PaneId, TabId};
pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    host: &Host,
    moved: TabId,
    destination: PaneId,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let mut checks = Vec::new();
    // Native bar states, overflow and IPC use the production surface above live pages.
    let pane = view.update_in(cx, |app, window, cx| {
        app.cancel_edit(window, cx);
        let pane = app.model.focused_pane().id;
        for i in 0..12 {
            app.model.add_tab(&format!("https://tabs.test/{i}"));
        }
        // Keep the original live document active; the overflow tabs remain lazy.
        app.model.switch_tab(pane, moved);
        cx.notify();
        pane
    })?;
    pause(cx).await;
    let count = bar_eval(
        view,
        pane,
        "document.querySelector('#count').textContent",
        cx,
    )
    .await?;
    ensure!(
        count.as_str().is_some_and(|s| s.ends_with("/13")),
        "Compact count mismatch: {count}"
    );
    bar_eval(
        view,
        pane,
        "document.querySelector('#count').click(); true",
        cx,
    )
    .await?;
    pause(cx).await;
    let overflow=bar_eval(view,pane,"({expanded:document.querySelector('#bar').classList.contains('expanded'),overflow:document.querySelector('#bar').classList.contains('overflow'),closes:document.querySelectorAll('.close').length,rows:new Set([...document.querySelectorAll('.chip')].map(c=>c.offsetTop)).size})",cx).await?;
    ensure!(
        overflow["expanded"] == true
            && overflow["overflow"] == true
            && overflow["closes"] == 13
            && overflow["rows"] == 1,
        "Native overflow layout failed: {overflow}"
    );
    ensure!(bar_eval(view,pane,"(()=>{const chip=document.querySelector('.chip'),saved=state;window.updateBar({...saved,active:saved.tabs.at(-1).id});const retained=chip===document.querySelector('.chip');window.updateBar(saved);return retained&&[...document.querySelectorAll('img')].every(i=>!i.draggable)})()",cx).await?==true,"Selection replaced chip nodes or enabled native image dragging");
    bar_eval(view,pane,"document.querySelector('.chip').dispatchEvent(new MouseEvent('dblclick',{bubbles:true})); true",cx).await?;
    pause(cx).await;
    ensure!(
        bar_eval(view, pane, "document.activeElement?.tagName==='INPUT'", cx).await? == true,
        "Chip double click did not focus native editor"
    );
    bar_eval(view,pane,"document.querySelector('input').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})); true",cx).await?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.editing.is_none())?,
        "Native Escape did not cancel edit"
    );
    bar_eval(view,pane,"document.querySelector('.active').dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowLeft',bubbles:true})); true",cx).await?;
    pause(cx).await;
    let last = view.update_in(cx, |app, _, _| {
        app.model.pane(pane).unwrap().tabs.last().unwrap().id
    })?;
    ensure!(
        view.update_in(cx, |app, _, _| app.active_tab())? == last,
        "Previous-tab selection did not wrap to last"
    );
    let revealed=bar_eval(view,pane,"(()=>{let t=document.querySelector('#tabs').getBoundingClientRect(),a=document.querySelector('.active').getBoundingClientRect();return a.left>=t.left-1&&a.right<=t.right+1})()",cx).await?;
    ensure!(revealed == true, "Overflow did not reveal active chip");
    bar_eval(view,pane,"document.querySelector('.active').dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true})); true",cx).await?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.active_tab())? == moved,
        "Next-tab selection did not wrap to first"
    );
    checks.push("native compact count, expansion, single-row chip overflow, visible close buttons, double-click editing, Escape IPC, tab wraparound and active-chip reveal");
    // Move the complete panel using the same handle drag state, preserving every tab.
    for edge in [
        workspace_core::SplitEdge::Left,
        workspace_core::SplitEdge::Right,
        workspace_core::SplitEdge::Top,
        workspace_core::SplitEdge::Bottom,
    ] {
        view.update_in(cx, |app, window, cx| {
            let viewport = window.viewport_size();
            let geometry = app.model.active_space().layout.geometry(
                workspace_core::Rect::new(
                    0.,
                    crate::render::TOP,
                    f32::from(viewport.width),
                    f32::from(viewport.height) - crate::render::TOP,
                ),
                6.,
                100.,
            );
            let r = geometry
                .panes
                .iter()
                .find(|p| p.id == destination)
                .unwrap()
                .bounds;
            let (x, y) = match edge {
                workspace_core::SplitEdge::Left => (0.1, 0.5),
                workspace_core::SplitEdge::Right => (0.9, 0.5),
                workspace_core::SplitEdge::Top => (0.5, 0.1),
                workspace_core::SplitEdge::Bottom => (0.5, 0.75),
            };
            let position = point(px(r.x + r.width * x), px(r.y + r.height * y));
            let origin = position - point(px(20.), px(20.));
            let before = app.model.pane(pane).unwrap().clone();
            app.arm_tab_drag(pane, moved, origin);
            app.tab_drag.as_mut().unwrap().whole = true;
            app.update_tab_drag(
                &MouseMoveEvent {
                    position,
                    pressed_button: Some(MouseButton::Left),
                    ..Default::default()
                },
                window,
                cx,
            );
            app.finish_tab_drag(Some(position), window, cx);
            ensure!(
                app.model.pane(pane) == Some(&before),
                "Whole-panel edge move changed stack"
            );
            ensure!(
                app.model.focused_pane().id == pane,
                "Whole-panel edge move lost focus"
            );
            Ok(())
        })??;
        pause(cx).await;
        ensure!(
            eval(
                host,
                moved,
                "!!window.dockIdentity && document.querySelector('input').value==='docked state'",
                cx
            )
            .await?
                == true,
            "Whole-panel move replaced native state"
        );
    }
    let original_order = view.update_in(cx, |app, _, _| {
        app.model
            .pane(pane)
            .unwrap()
            .tabs
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>()
    })?;
    view.update_in(cx, |app, window, cx| {
        let viewport = window.viewport_size();
        let geometry = app.model.active_space().layout.geometry(
            workspace_core::Rect::new(
                0.,
                crate::render::TOP,
                f32::from(viewport.width),
                f32::from(viewport.height) - crate::render::TOP,
            ),
            6.,
            100.,
        );
        let r = geometry
            .panes
            .iter()
            .find(|p| p.id == destination)
            .unwrap()
            .bounds;
        let position = point(px(r.x + r.width / 2.), px(r.y + r.height / 2.));
        app.arm_tab_drag(pane, moved, position - point(px(20.), px(20.)));
        app.tab_drag.as_mut().unwrap().whole = true;
        app.update_tab_drag(
            &MouseMoveEvent {
                position,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
        app.finish_tab_drag(Some(position), window, cx);
        ensure!(
            app.model.pane(pane).is_none(),
            "Whole-panel merge left source behind"
        );
        let tabs = &app.model.pane(destination).unwrap().tabs;
        ensure!(
            tabs.iter()
                .skip(tabs.len() - original_order.len())
                .map(|t| t.id)
                .collect::<Vec<_>>()
                == original_order,
            "Merge changed stack order"
        );
        Ok(())
    })??;
    pause(cx).await;
    ensure!(
        eval(
            host,
            moved,
            "!!window.dockIdentity && document.querySelector('input').value==='docked state'",
            cx
        )
        .await?
            == true,
        "Merge replaced native state"
    );
    host.back(moved)?;
    pause(cx).await;
    ensure!(
        !host.current_url(moved)?.contains("dock=history"),
        "Merge lost back history"
    );
    host.forward(moved)?;
    pause(cx).await;
    ensure!(
        host.current_url(moved)?.contains("dock=history"),
        "Merge lost forward history"
    );
    checks.push("whole-panel handle moves at every edge and center merge retain stack order, active tab, native DOM/form state and history");
    Ok(checks)
}
