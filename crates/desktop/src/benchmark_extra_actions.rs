//! Reordering, preview creation and whole-panel merge workloads.
use crate::{
    app::BrowserApp,
    benchmark::{Config, Samples, draw, timed},
    benchmark_fixture::Session,
};
use anyhow::{Result, ensure};
use gpui::*;
use workspace_core::Axis;

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
    config: &Config,
    f: &Session,
    samples: &mut Samples,
) -> Result<()> {
    for i in 0..config.warmup + config.iterations {
        let measured = i >= config.warmup;
        timed(
            view,
            cx,
            samples,
            "action.space.reorder",
            measured,
            |app, _, cx| {
                let slot = if i % 2 == 0 {
                    app.model.spaces.len()
                } else {
                    0
                };
                ensure!(
                    app.model.move_space(f.spaces[0], slot),
                    "space reorder failed"
                );
                cx.notify();
                Ok(())
            },
        )
        .await?;
        timed(
            view,
            cx,
            samples,
            "action.tab.reorder",
            measured,
            |app, _, cx| {
                let slot = if i % 2 == 0 {
                    0
                } else {
                    app.model.pane(f.first).unwrap().tabs.len()
                };
                ensure!(
                    app.model.move_tab(f.first, f.tabs[0], f.first, slot),
                    "tab reorder failed"
                );
                cx.notify();
                Ok(())
            },
        )
        .await?;
        view.update_in(cx, |app, window, cx| app.focus_pane(f.first, window, cx))?;
        draw(cx).await?;
        timed(
            view,
            cx,
            samples,
            "action.tab.drag_preview_start",
            measured,
            |app, window, cx| {
                let origin = point(px(30.), px(100.));
                app.arm_tab_drag(f.first, f.tabs[0], origin);
                app.update_tab_drag(
                    &MouseMoveEvent {
                        position: origin + point(px(30.), px(30.)),
                        pressed_button: Some(MouseButton::Left),
                        ..Default::default()
                    },
                    window,
                    cx,
                );
                ensure!(
                    app.drag_preview.is_some() && app.split_preview.is_some(),
                    "drag preview not created"
                );
                Ok(())
            },
        )
        .await?;
        view.update_in(cx, |app, _, cx| app.cancel_tab_drag(cx))?;
        draw(cx).await?;
        let (pane, tabs) = view.update_in(cx, |app, _, cx| {
            app.model.focus_pane(f.first);
            let pane = app.model.split_focused(Axis::Vertical, "about:blank");
            app.model.add_tab("about:blank");
            cx.notify();
            (
                pane,
                app.model
                    .pane(pane)
                    .unwrap()
                    .tabs
                    .iter()
                    .map(|t| t.id)
                    .collect::<Vec<_>>(),
            )
        })?;
        draw(cx).await?;
        timed(
            view,
            cx,
            samples,
            "action.panel.merge",
            measured,
            |app, window, cx| {
                crate::benchmark_actions::release(
                    app, window, cx, pane, tabs[0], f.first, None, true,
                );
                ensure!(
                    app.model.active_space().panes.len() == 2 && app.model.pane(pane).is_none(),
                    "panel merge failed"
                );
                ensure!(
                    tabs.iter().all(|id| app
                        .model
                        .pane(f.first)
                        .unwrap()
                        .tabs
                        .iter()
                        .any(|t| t.id == *id)),
                    "merged tabs were lost"
                );
                Ok(())
            },
        )
        .await?;
        view.update_in(cx, |app, window, cx| {
            for tab in tabs {
                app.model.close_tab(f.first, tab);
            }
            app.focus_pane(f.first, window, cx);
            cx.notify();
        })?;
        draw(cx).await?;
    }
    Ok(())
}
