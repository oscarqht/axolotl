//! Native layer clipping through panel and window relayout, without network access.
use crate::{
    app::BrowserApp,
    smoke::{eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use workspace_core::{Axis, SplitEdge};

async fn check(
    view: &WeakEntity<BrowserApp>,
    stage: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    pause(cx).await;
    view.update_in(cx, |app, window, _| -> Result<()> {
        let viewport = window.viewport_size();
        let geometry = app.model.active_space().layout.geometry(
            crate::render::panel_content_rect(
                f32::from(viewport.width),
                f32::from(viewport.height),
            ),
            6.,
            100.,
        );
        let left = geometry
            .panes
            .iter()
            .map(|pane| pane.bounds.x)
            .fold(f32::INFINITY, f32::min);
        let top = geometry
            .panes
            .iter()
            .map(|pane| pane.bounds.y)
            .fold(f32::INFINITY, f32::min);
        let right = geometry
            .panes
            .iter()
            .map(|pane| pane.bounds.x + pane.bounds.width)
            .fold(f32::NEG_INFINITY, f32::max);
        let bottom = geometry
            .panes
            .iter()
            .map(|pane| pane.bounds.y + pane.bounds.height)
            .fold(f32::NEG_INFINITY, f32::max);
        ensure!(
            (left - 6.).abs() < 1.
                && (top - crate::render::TOP - 6.).abs() < 1.
                && (f32::from(viewport.width) - right - 6.).abs() < 1.
                && (f32::from(viewport.height) - bottom - 6.).abs() < 1.,
            "{stage}: panel layout lost its six-point outer gutter"
        );
        for pane in geometry.panes {
            let tab = app.model.pane(pane.id).unwrap().active_tab;
            let state = app.host.panel_clip_state(tab)?;
            ensure!(
                state["radius"] == f64::from(crate::render::PANEL_CORNER_RADIUS)
                    && state["clips"] == true
                    && state["corners"] == 15,
                "{stage}: panel {tab} lost four-corner clipping: {state}"
            );
            ensure!(
                (state["width"].as_f64().unwrap() - f64::from(pane.bounds.width)).abs() < 1.
                    && (state["height"].as_f64().unwrap() - f64::from(pane.bounds.height)).abs()
                        < 1.,
                "{stage}: native bounds differ from panel: {state}"
            );
            ensure!(
                app.host.is_visible(tab) && app.bars.contains_key(&pane.id),
                "{stage}: page or floating controls missing"
            );
        }
        Ok(())
    })??;
    println!("AXOLOTL_CORNERS {stage}");
    if std::env::args().any(|arg| arg == "--smoke-hold-corners") {
        for _ in 0..50 {
            pause(cx).await;
        }
    }
    Ok(())
}

async fn fixtures(view: &WeakEntity<BrowserApp>, cx: &mut AsyncWindowContext) -> Result<()> {
    let (host, tabs) = view.update_in(cx, |app, _, _| {
        (
            app.host.clone(),
            app.model
                .active_space()
                .panes
                .iter()
                .map(|p| p.active_tab)
                .collect::<Vec<_>>(),
        )
    })?;
    for (i, tab) in tabs.into_iter().enumerate() {
        wait_title(&host, tab, "New tab · Axolotl", cx).await?;
        eval(&host, tab, &format!(r#"(()=>{{
            document.documentElement.style.cssText='background:{};height:100%';
            document.body.style.cssText='margin:0;background:transparent;min-height:100vh;display:grid;place-items:center;color:#123;font:32px system-ui';
            document.body.textContent='Panel {} — all four corners';
            window.cornerIdentity={{}}; return true;
        }})()"#, ["#74d9d0", "#ffbd80", "#c2b4ff"][i % 3], i + 1), cx).await?;
    }
    Ok(())
}

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    fixtures(view, cx).await?;
    check(view, "single", cx).await?;
    let (first, second) = view.update_in(cx, |app, _, cx| {
        let first = app.model.focused_pane().id;
        let second = app.model.split_focused(Axis::Horizontal, "about:blank");
        app.model.split_focused(Axis::Vertical, "about:blank");
        cx.notify();
        (first, second)
    })?;
    pause(cx).await;
    fixtures(view, cx).await?;
    check(view, "split", cx).await?;
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
        app.model.resize_split(geometry.dividers[0].id, 0.62);
        cx.notify();
    })?;
    check(view, "divider-resized", cx).await?;
    view.update_in(cx, |_, window, _| window.resize(size(px(1000.), px(700.))))?;
    check(view, "window-resized", cx).await?;
    view.update_in(cx, |app, _, cx| -> Result<()> {
        ensure!(
            app.model.move_pane(first, second, Some(SplitEdge::Bottom)),
            "Panel rearrangement failed"
        );
        cx.notify();
        Ok(())
    })??;
    check(view, "rearranged", cx).await?;
    let (host, tabs) = view.update_in(cx, |app, _, _| {
        (
            app.host.clone(),
            app.model
                .active_space()
                .panes
                .iter()
                .map(|p| p.active_tab)
                .collect::<Vec<_>>(),
        )
    })?;
    for tab in tabs {
        ensure!(
            eval(&host, tab, "!!window.cornerIdentity", cx).await? == true,
            "Relayout replaced a native document"
        );
    }
    Ok(vec![
        "four native clipped corners and six-point outer gutter in single and nested split panels; divider/window resize and panel rearrangement preserve bounds, live documents and floating controls",
    ])
}
