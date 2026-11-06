//! Native floating-control and whole-panel integration assertions.
use crate::{
    app::BrowserApp,
    smoke::{Host, eval, pause},
};
use anyhow::{Result, ensure};
use gpui::*;
use workspace_core::{PaneId, TabId};

#[cfg(target_os = "windows")]
pub(crate) async fn check_windows_controls(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (pane, first, second) = view.update_in(cx, |app, window, cx| {
        app.cancel_edit(window, cx);
        let pane = app.model.focused_pane().id;
        let first = app.active_tab();
        let second = app.model.add_tab("about:blank");
        app.model.switch_tab(pane, first);
        cx.notify();
        (pane, first, second)
    })?;
    for _ in 0..4 {
        pause(cx).await;
    }
    view.update_in(cx, |app, _, cx| app.bars[&pane].test_expand(true, cx))?;
    for _ in 0..4 {
        pause(cx).await;
    }
    view.update_in(cx, |app, _, cx| -> Result<()> {
        let bar = &app.bars[&pane];
        ensure!(bar.expanded, "GPUI hover expansion was not delivered");
        let viewport = bar.test_viewport(cx)?;
        ensure!(
            (f32::from(viewport.width) - bar.bounds.width).abs() < 1.
                && (f32::from(viewport.height) - 48.).abs() < 1.,
            "Native client area clips the chip row: {viewport:?}, bounds {:?}",
            bar.bounds
        );
        ensure!(bar.bounds.width > 176., "Native overlay did not expand");
        ensure!(
            bar.test_frontmost()?,
            "Native GPUI overlay is covered by the page"
        );
        bar.test_action("select", second, cx);
        Ok(())
    })??;
    for _ in 0..4 {
        pause(cx).await;
    }
    view.update_in(cx, |app, _, cx| -> Result<()> {
        ensure!(
            app.active_tab() == second,
            "Native selection selected the wrong tab"
        );
        app.bars[&pane].test_action("edit", second, cx);
        Ok(())
    })??;
    for _ in 0..4 {
        pause(cx).await;
    }
    view.update_in(cx, |app, _, cx| -> Result<()> {
        let (editing, value) = app.bars[&pane].test_editor(cx);
        ensure!(
            editing == Some(second) && value == "about:blank",
            "Native address editor was not initialized"
        );
        ensure!(
            app.bars[&pane].has_focus(),
            "Native editor did not receive keyboard focus"
        );
        app.bars[&pane].test_action("cancel", second, cx);
        Ok(())
    })??;
    for _ in 0..3 {
        pause(cx).await;
    }
    view.update_in(cx, |app, window, cx| {
        app.tab_action(
            crate::app::TabTarget::Page(pane, second),
            crate::app::TabAction::Close,
            window,
            cx,
        );
        app.model.switch_tab(pane, first);
        cx.notify();
    })?;
    Ok(vec![
        "native GPUI overlay expands above WebView2, selects tabs, focuses its address editor, cancels editing and closes tabs without JavaScript",
    ])
}

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    host: &Host,
    moved: TabId,
    destination: PaneId,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let mut checks = check_sizing(view, cx).await?;
    let pane = view.update_in(cx, |app, window, cx| {
        app.cancel_edit(window, cx);
        let pane = app.model.focused_pane().id;
        for _ in 0..12 {
            app.model.add_tab("about:blank");
        }
        app.model.switch_tab(pane, moved);
        cx.notify();
        pane
    })?;
    pause(cx).await;
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
                crate::render::panel_content_rect(
                    f32::from(viewport.width),
                    f32::from(viewport.height),
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
            crate::render::panel_content_rect(
                f32::from(viewport.width),
                f32::from(viewport.height),
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

pub(crate) async fn check_sizing(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (pane, tab) = view.update_in(cx, |app, window, cx| {
        app.cancel_edit(window, cx);
        (app.model.focused_pane().id, app.active_tab())
    })?;
    pause(cx).await;
    view.update_in(cx, |app, _, cx| app.bars[&pane].test_expand(true, cx))?;
    pause(cx).await;
    pause(cx).await;
    let width = view.update_in(cx, |app, _, _| app.bars[&pane].bounds.width)?;
    ensure!(width > 176., "GPUI strip failed to expand");
    let extra = view.update_in(cx, |app, _, cx| {
        let id = app.model.add_tab("about:blank");
        app.model.switch_tab(pane, tab);
        cx.notify();
        id
    })?;
    pause(cx).await;
    pause(cx).await;
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.bars[&pane].bounds.width >= width,
            "Additional chip shrank native strip"
        );
        Ok(())
    })??;
    view.update_in(cx, |app, window, cx| {
        app.tab_action(
            crate::app::TabTarget::Page(pane, extra),
            crate::app::TabAction::Close,
            window,
            cx,
        )
    })?;
    Ok(vec!["GPUI strip expands and grows to fit additional tabs"])
}
