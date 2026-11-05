//! Native WebKit pointer capture and queued release regression (macOS).
use crate::{
    app::BrowserApp,
    smoke::{bar_eval, eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use objc2_app_kit::NSEventType as Mouse;
use workspace_core::{Axis, PaneId, Rect, SplitEdge};

async fn mouse(
    view: &WeakEntity<BrowserApp>,
    pane: PaneId,
    kind: Mouse,
    position: Point<Pixels>,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    view.update_in(cx, |app, _, _| {
        app.bars[&pane].test_mouse(
            kind,
            f32::from(position.x).into(),
            f32::from(position.y).into(),
        )
    })??;
    Ok(())
}

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    for cross in [false, true] {
        for edge in [
            SplitEdge::Left,
            SplitEdge::Right,
            SplitEdge::Top,
            SplitEdge::Bottom,
        ] {
            for paused in [false, true] {
                let (pane, moved, destination, host) =
                    view.update_in(cx, |app, window, cx| {
                        app.model.add_space("Native docking test", "about:blank");
                        let pane = app.model.focused_pane().id;
                        let moved = app.active_tab();
                        app.model.add_tab("about:blank");
                        let destination = if cross {
                            app.model.split_focused(Axis::Horizontal, "about:blank")
                        } else {
                            pane
                        };
                        app.host.ensure(moved, "about:blank", window, cx)?;
                        cx.notify();
                        Ok::<_, anyhow::Error>((pane, moved, destination, app.host.clone()))
                    })??;
                wait_title(&host, moved, "New tab · Axolotl", cx).await?;
                for _ in 0..4 {
                    pause(cx).await;
                }
                eval(&host, moved, "window.dockIdentity={}; document.querySelector('input').value='retained drag'; true", cx).await?;
                bar_eval(view, pane, "pinned=true; expand(true); true", cx).await?;
                for _ in 0..3 {
                    pause(cx).await;
                }
                // sendEvent/mouseDragged do not update the OS's global pressed
                // buttons. Supply that state to the trusted WebKit pointer events
                // only in this smoke fixture. No DOM events are dispatched here.
                bar_eval(view, pane, r#"window.dragTrace=[];
                    for(const type of ['pointerdown','pointermove','pointerup','pointercancel','lostpointercapture'])
                        window.addEventListener(type,e=>{
                            if(type==='pointerdown'||type==='pointermove') Object.defineProperty(e,'buttons',{value:1});
                            dragTrace.push([type,e.isTrusted]);
                        },true); true"#, cx).await?;
                let chip = bar_eval(view, pane, "(()=>{const r=document.querySelector('.chip').getBoundingClientRect();return [r.x+35,r.y+r.height/2]})()", cx).await?;
                let (origin, position) = view.update_in(cx, |app, window, _| {
                    let bar = app.bars[&pane].bounds;
                    let origin = point(
                        px(bar.x + chip[0].as_f64().unwrap() as f32),
                        px(bar.y + chip[1].as_f64().unwrap() as f32),
                    );
                    let viewport = window.viewport_size();
                    let geometry = app.model.active_space().layout.geometry(
                        Rect::new(
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
                        SplitEdge::Left => (0.08, 0.5),
                        SplitEdge::Right => (0.92, 0.5),
                        SplitEdge::Top => (0.3, 0.08),
                        SplitEdge::Bottom => (0.3, 0.92),
                    };
                    (origin, point(px(r.x + r.width * x), px(r.y + r.height * y)))
                })?;
                mouse(view, pane, Mouse::LeftMouseDown, origin, cx).await?;
                pause(cx).await;
                mouse(
                    view,
                    pane,
                    Mouse::LeftMouseDragged,
                    origin + point(px(12.), px(0.)),
                    cx,
                )
                .await?;
                pause(cx).await;
                mouse(view, pane, Mouse::LeftMouseDragged, position, cx).await?;
                if paused {
                    pause(cx).await;
                }
                if paused {
                    view.update_in(cx, |app, window, _| -> Result<()> {
                        ensure!(
                            app.native_drag_from(pane) && app.tab_drag_active(),
                            "native drag not active"
                        );
                        ensure!(
                            app.split_drop_target(position, window)
                                .map(|(p, e, _)| (p, e))
                                == Some((destination, edge)),
                            "wrong preview target"
                        );
                        ensure!(app.split_preview.is_some(), "missing native preview");
                        Ok(())
                    })??;
                }
                mouse(view, pane, Mouse::LeftMouseUp, position, cx).await?;
                // Exercise the race deterministically before the queued WebKit
                // release is polled: both shell completion paths and a free move.
                view.update_in(cx, |app, window, cx| {
                    app.finish_tab_drag(None, window, cx);
                    app.finish_tab_drag(Some(point(px(800.), px(20.))), window, cx);
                    app.update_tab_drag(
                        &MouseMoveEvent {
                            position: point(px(800.), px(20.)),
                            ..Default::default()
                        },
                        window,
                        cx,
                    );
                })?;
                for _ in 0..3 {
                    pause(cx).await;
                }
                let trace = bar_eval(view, pane, "dragTrace", cx).await?;
                ensure!(
                    trace
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|e| e[0] == "pointerup" && e[1] == true),
                    "missing trusted native release: {trace}"
                );
                view.update_in(cx, |app, _, _| -> Result<()> {
                    ensure!(app.tab_drag.is_none(), "release retained drag");
                    ensure!(
                        app.model.active_space().panes.len() == if cross { 3 } else { 2 },
                        "{edge:?} cross={cross} paused={paused}: native release did not split"
                    );
                    ensure!(
                        app.active_tab() == moved && app.model.focused_pane().id != destination,
                        "wrong split focus"
                    );
                    Ok(())
                })??;
                ensure!(eval(&host,moved,"!!window.dockIdentity && document.querySelector('input').value==='retained drag'",cx).await?==true,"native document replaced");
                eprintln!("AXOLOTL_DOCKING passed edge={edge:?} cross={cross} paused={paused}");
            }
        }
    }
    // Global cancellation still applies to a native-owned drag, and a late
    // native release must not resurrect it. Exercise the real Escape action.
    let before = view.update_in(cx, |app, window, cx| {
        let pane = app.model.focused_pane().id;
        let tab = app.active_tab();
        let before = app.model.to_json();
        let origin = point(px(30.), px(100.));
        app.arm_tab_drag(pane, tab, origin);
        app.tab_drag.as_mut().unwrap().input = crate::app::DragInput::NativeBar;
        app.update_native_tab_drag(pane, origin + point(px(20.), px(20.)), window, cx);
        (before, pane, origin)
    })?;
    cx.update(|window, cx| window.dispatch_action(Box::new(gpui_component::input::Escape), cx))?;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| -> Result<()> {
        ensure!(
            app.tab_drag.is_none() && app.split_preview.is_none() && app.drag_preview.is_none(),
            "Escape retained native drag"
        );
        app.finish_native_tab_drag(before.1, Some(before.2), window, cx);
        ensure!(
            app.model.to_json() == before.0,
            "late native release changed cancelled layout"
        );
        Ok(())
    })??;
    Ok(vec![
        "16 native pointer-capture split drops: four edges, same/cross pane, quick/paused release; competing shell events ignored; native DOM retained",
    ])
}
