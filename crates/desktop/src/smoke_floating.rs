//! Native pointer-capture coverage for detached tabs, panel headers and resize grips.
use crate::{
    app::BrowserApp,
    smoke::{bar_eval, eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use objc2_app_kit::NSEventType as Mouse;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use workspace_core::{PaneId, Rect, SplitEdge, Workspace};

async fn chrome_eval(
    view: &WeakEntity<BrowserApp>,
    pane: PaneId,
    surface: &str,
    script: &str,
    cx: &mut AsyncWindowContext,
) -> Result<serde_json::Value> {
    let value = Arc::new(Mutex::new(None));
    let result = value.clone();
    view.update_in(cx, |app, _, _| {
        app.floating_test_eval(pane, surface, script, move |v| {
            *result.lock().unwrap() = Some(v)
        })
    })??;
    for _ in 0..50 {
        if let Some(v) = value.lock().unwrap().take() {
            return Ok(serde_json::from_str(&v)?);
        }
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    }
    anyhow::bail!("floating chrome evaluation timed out")
}
async fn drag(
    view: &WeakEntity<BrowserApp>,
    pane: PaneId,
    surface: &str,
    start: Point<Pixels>,
    end: Point<Pixels>,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    // NSEvent synthesis doesn't change global pressed-button state.
    let fixture = "for(const t of ['pointerdown','pointermove'])window.addEventListener(t,e=>Object.defineProperty(e,'buttons',{value:1}),true);true";
    if surface == "bar" {
        bar_eval(view, pane, fixture, cx).await?;
    } else {
        chrome_eval(view, pane, surface, fixture, cx).await?;
    }
    for (kind, p) in [
        (Mouse::LeftMouseDown, start),
        (Mouse::LeftMouseDragged, start + point(px(10.), px(0.))),
        (Mouse::LeftMouseDragged, end),
        (Mouse::LeftMouseUp, end),
    ] {
        view.update_in(cx, |app, _, _| {
            if surface == "bar" {
                app.bars[&pane].test_mouse(kind, f32::from(p.x).into(), f32::from(p.y).into())
            } else {
                app.floating_test_mouse(pane, surface, kind, p)
            }
        })??;
        pause(cx).await;
    }
    for _ in 0..3 {
        pause(cx).await;
    }
    Ok(())
}

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    eprintln!("Floating smoke: detach native tab");
    let (source, tab, host) = view.update_in(cx, |app, window, cx| {
        app.floating_test_pointer(point(px(-100.), px(-100.)));
        app.model.add_space("Floating panels", "about:blank");
        let source = app.model.focused_pane().id;
        let tab = app.active_tab();
        app.model.add_tab("about:blank");
        app.host.ensure(tab, "about:blank", window, cx)?;
        cx.notify();
        Ok::<_, anyhow::Error>((source, tab, app.host.clone()))
    })??;
    wait_title(&host, tab, "New tab · Axolotl", cx).await?;
    eval(&host,tab,"window.floatingIdentity={};document.querySelector('input').value='Live page retained';true",cx).await?;
    bar_eval(view, source, "pinned=true;expand(true);true", cx).await?;
    for _ in 0..4 {
        pause(cx).await;
    }
    let chip=bar_eval(view,source,"(()=>{let r=document.querySelector('.chip').getBoundingClientRect();return [r.x+30,r.y+20]})()",cx).await?;
    let start = view.update_in(cx, |app, _, _| {
        let r = app.bars[&source].bounds;
        point(
            px(r.x + chip[0].as_f64().unwrap() as f32),
            px(r.y + chip[1].as_f64().unwrap() as f32),
        )
    })?;
    drag(view, source, "bar", start, point(px(480.), px(280.)), cx).await?;
    let split_pane = view.update_in(cx, |app, _, _| {
        ensure!(
            app.model.active_space().floating.is_empty(),
            "content drop created an overlay instead of a split"
        );
        ensure!(
            app.model.active_space().layout.pane_ids().len() == 2,
            "content drop didn't split"
        );
        Ok::<_, anyhow::Error>(
            app.model
                .active_space()
                .panes
                .iter()
                .find(|p| p.active_tab == tab)
                .unwrap()
                .id,
        )
    })??;
    let (start, edge) = view.update_in(cx, |app, window, _| {
        let r = app.bars[&split_pane].bounds;
        let s = window.viewport_size();
        let a = crate::render::panel_content_rect(s.width.into(), s.height.into());
        (
            point(px(r.x + 15.), px(r.y + 24.)),
            point(px(a.x + 4.), px(280.)),
        )
    })?;
    drag(view, split_pane, "bar", start, edge, cx).await?;
    let pane = view.update_in(cx, |app, _, cx| {
        let pane = app.model.active_space().floating.first().unwrap().pane;
        app.model.pin_floating(pane);
        app.reveal_floating(pane);
        cx.notify();
        pane
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    let clip = host.panel_clip_state(tab)?;
    ensure!(
        clip["radius"] == f64::from(crate::floating_panels::PAGE_RADIUS)
            && clip["corners"] == if clip["flipped"] == true { 12 } else { 3 },
        "floating page must have square top joins and rounded outer bottom corners: {clip}"
    );
    let border=chrome_eval(view,pane,"header","(()=>{const s=getComputedStyle(document.body);return [s.borderLeftWidth,s.borderRightWidth,s.borderBottomWidth,s.borderBottomLeftRadius]})()",cx).await?;
    ensure!(
        border == serde_json::json!(["2px", "2px", "2px", "10px"]),
        "floating outline is incomplete: {border}"
    );
    view.update_in(cx, |app, _, _| {
        let frame = app.floating_test_bounds(pane, "header");
        let content = app.host.panel_clip_state(tab)?;
        let left = content["x"].as_f64().unwrap();
        let right = left + content["width"].as_f64().unwrap();
        ensure!(
            left >= f64::from(frame.x) + 1.5 && right <= f64::from(frame.x + frame.width) - 1.5,
            "native page covers the floating frame border: {content}"
        );
        Ok::<_, anyhow::Error>(())
    })??;
    let grip_style=chrome_eval(view,pane,"grip","(()=>{const r=grip.getBoundingClientRect();return [r.width,r.height,getComputedStyle(grip).clipPath]})()",cx).await?;
    ensure!(
        grip_style[0] == 19.
            && grip_style[1] == 19.
            && grip_style[2].as_str().unwrap().starts_with("polygon"),
        "resize grip is not a corner overlay"
    );
    ensure!(eval(&host,tab,"!!window.floatingIdentity && document.querySelector('input').value==='Live page retained'",cx).await?==true,"detaching replaced live document");
    eprintln!("Floating smoke: move whole panel {pane}");
    // Move a whole floating panel using its native header.
    let r = view.update_in(cx, |app, _, _| app.floating_test_bounds(pane, "header"))?;
    drag(
        view,
        pane,
        "header",
        point(px(r.x + 60.), px(r.y + 15.)),
        point(px(620.), px(360.)),
        cx,
    )
    .await?;
    view.update_in(cx, |app, _, _| {
        let f = app.model.active_space().floating_panel(pane).unwrap();
        ensure!(f.bounds.x > 500., "header didn't move panel");
        Ok::<_, anyhow::Error>(())
    })??;
    eprintln!("Floating smoke: resize panel");
    let (grip, before) = view.update_in(cx, |app, _, _| {
        (
            app.floating_test_bounds(pane, "grip"),
            app.model
                .active_space()
                .floating_panel(pane)
                .unwrap()
                .bounds,
        )
    })?;
    drag(
        view,
        pane,
        "grip",
        point(px(grip.x + 14.), px(grip.y + 14.)),
        point(px(grip.x + 64.), px(grip.y + 54.)),
        cx,
    )
    .await?;
    view.update_in(cx, |app, _, _| {
        let r = app
            .model
            .active_space()
            .floating_panel(pane)
            .unwrap()
            .bounds;
        ensure!(
            r.width > before.width && r.height > before.height,
            "native grip did not resize"
        );
        Ok::<_, anyhow::Error>(())
    })??;
    view.update_in(cx, |app, _, cx| {
        app.model.pin_floating(pane);
        cx.notify();
    })?;
    for edge in [
        SplitEdge::Left,
        SplitEdge::Right,
        SplitEdge::Top,
        SplitEdge::Bottom,
    ] {
        eprintln!("Floating smoke: dock {edge:?}");
        let (r, end) = view.update_in(cx, |app, window, _| {
            let s = window.viewport_size();
            let a = crate::render::panel_content_rect(s.width.into(), s.height.into());
            let p = match edge {
                SplitEdge::Left => (a.x + 4., a.y + a.height / 2.),
                SplitEdge::Right => (a.x + a.width - 4., a.y + a.height / 2.),
                SplitEdge::Top => (a.x + a.width / 2., a.y + 4.),
                SplitEdge::Bottom => (a.x + a.width / 2., a.y + a.height - 4.),
            };
            (
                app.floating_test_bounds(pane, "header"),
                point(px(p.0), px(p.1)),
            )
        })?;
        drag(
            view,
            pane,
            "header",
            point(px(r.x + 50.), px(r.y + 15.)),
            end,
            cx,
        )
        .await?;
        view.update_in(cx, |app, window, _| {
            ensure!(
                app.model.active_space().floating_panel(pane).unwrap().edge == Some(edge),
                "wrong docked edge: expected {edge:?}, actual {:?}, drag {:?}",
                app.model.active_space().floating_panel(pane).unwrap().edge,
                app.tab_drag
            );
            ensure!(
                Workspace::from_json(&app.model.to_json())? == *app.model,
                "floating session did not round trip"
            );
            let s = window.viewport_size();
            let area = crate::render::panel_content_rect(s.width.into(), s.height.into());
            let chip = app.floating_test_bounds(pane, "chip");
            let frame = app.floating_test_bounds(pane, "header");
            let f = app.model.active_space().floating_panel(pane).unwrap();
            let vertical = matches!(edge, SplitEdge::Left | SplitEdge::Right);
            let expected = if vertical {
                f.bounds.y.clamp(area.y, area.y + area.height - chip.height)
            } else {
                f.bounds.x.clamp(area.x, area.x + area.width - chip.width)
            };
            ensure!(
                ((if vertical { chip.y } else { chip.x }) - expected).abs() < 1.,
                "chip lost its docking anchor"
            );
            ensure!(
                frame.x >= area.x
                    && frame.y >= area.y
                    && frame.x + frame.width <= area.x + area.width + 0.1
                    && frame.y + frame.height <= area.y + area.height + 0.1,
                "revealed panel extends outside workspace"
            );
            Ok::<_, anyhow::Error>(())
        })??;
        let chip_size=chrome_eval(view,pane,"chip","(()=>{const r=frame.getBoundingClientRect();return [r.width,r.height,r.x,r.y,innerWidth,innerHeight]})()",cx).await?;
        let vertical = matches!(edge, SplitEdge::Left | SplitEdge::Right);
        ensure!(
            chip_size[0].as_f64().unwrap() == if vertical { 28. } else { 156. }
                && chip_size[1].as_f64().unwrap() == if vertical { 156. } else { 28. }
                && chip_size[2].as_f64().unwrap().abs() < 0.1
                && chip_size[3].as_f64().unwrap().abs() < 0.1,
            "rotated chip is clipped or has incorrect orientation: {chip_size}"
        );
        if edge == SplitEdge::Left {
            let anchor = view.update_in(cx, |app, _, cx| {
                let b = app
                    .model
                    .active_space()
                    .floating_panel(pane)
                    .unwrap()
                    .bounds;
                app.model.pin_floating(pane);
                app.reveal_floating(pane);
                cx.notify();
                b
            })?;
            for _ in 0..3 {
                pause(cx).await;
            }
            let g = view.update_in(cx, |app, _, _| app.floating_test_bounds(pane, "grip"))?;
            drag(
                view,
                pane,
                "grip",
                point(px(g.x + 14.), px(g.y + 14.)),
                point(px(g.x + 26.), px(g.y + 24.)),
                cx,
            )
            .await?;
            view.update_in(cx, |app, _, cx| {
                let b = app
                    .model
                    .active_space()
                    .floating_panel(pane)
                    .unwrap()
                    .bounds;
                ensure!(
                    b.x == anchor.x && b.y == anchor.y,
                    "resizing moved the docking anchor"
                );
                app.model.pin_floating(pane);
                cx.notify();
                Ok::<_, anyhow::Error>(())
            })??;
        }
        // Allow leave-to-hide to expire, then invoke the chip's own hover handler.
        for _ in 0..5 {
            pause(cx).await;
        }
        ensure!(!host.is_visible(tab), "unpinned panel failed to hide");
        chrome_eval(view, pane, "chip", "frame.onpointerenter();true", cx).await?;
        pause(cx).await;
        ensure!(host.is_visible(tab), "chip hover didn't reveal native page");
        chrome_eval(view, pane, "header", "pin.click();true", cx).await?;
        for _ in 0..5 {
            pause(cx).await;
        }
        ensure!(host.is_visible(tab), "pinned panel hid");
        chrome_eval(
            view,
            pane,
            "header",
            "pin.click();frame.onpointerenter();true",
            cx,
        )
        .await?;
        pause(cx).await;
    }
    eprintln!("Floating smoke: reorder chips");
    // Separate chips reorder, then whole-stack split and tab-row merge return paths.
    let other = view.update_in(cx, |app, _, cx| {
        app.model.add_tab("about:blank");
        let tab = app.active_tab();
        let other = app
            .model
            .float_tab(
                pane,
                tab,
                Rect::new(50., 100., 420., 300.),
                Some(SplitEdge::Bottom),
            )
            .unwrap();
        app.model.pin_floating(pane);
        cx.notify();
        other
    })?;
    for edge in [SplitEdge::Left, SplitEdge::Right, SplitEdge::Bottom] {
        view.update_in(cx, |app, _, cx| {
            let a = app
                .model
                .active_space()
                .floating_panel(pane)
                .unwrap()
                .bounds;
            let b = app
                .model
                .active_space()
                .floating_panel(other)
                .unwrap()
                .bounds;
            app.model.float_pane(pane, a, Some(edge));
            app.model.float_pane(other, b, Some(edge));
            app.model.reorder_floating(pane, 0);
            cx.notify();
        })?;
        for _ in 0..4 {
            pause(cx).await;
        }
        let (a, b) = view.update_in(cx, |app, _, _| {
            (
                app.floating_test_bounds(pane, "chip"),
                app.floating_test_bounds(other, "chip"),
            )
        })?;
        let vertical = matches!(edge, SplitEdge::Left | SplitEdge::Right);
        let (dx, dy) = if vertical { (14., 45.) } else { (35., 14.) };
        drag(
            view,
            other,
            "chip",
            point(px(b.x + dx), px(b.y + dy)),
            point(px(a.x + dx), px(a.y + dy)),
            cx,
        )
        .await?;
        view.update_in(cx, |app, _, _| {
            ensure!(
                app.model.active_space().floating[0].pane == other,
                "chip reorder failed at {edge:?}"
            );
            Ok::<_, anyhow::Error>(())
        })??;
    }
    eprintln!("Floating smoke: return split");
    // Return panel into a tiled split via its explicit edge target away from window edges.
    // Build a known two-leaf tiled tree by splitting the existing tiled pane.
    view.update_in(cx, |app, _, cx| {
        let tiled = app.model.active_space().layout.pane_ids()[0];
        app.model.focus_pane(tiled);
        app.model
            .split_focused(workspace_core::Axis::Horizontal, "about:blank");
        cx.notify();
    })?;
    for _ in 0..4 {
        pause(cx).await;
    }
    let (r, end, tiled) = view.update_in(cx, |app, window, _| {
        let s = window.viewport_size();
        let g = app.model.active_space().layout.geometry(
            crate::render::panel_content_rect(s.width.into(), s.height.into()),
            6.,
            100.,
        );
        let target = &g.panes[0];
        (
            app.floating_test_bounds(pane, "header"),
            point(
                px(target.bounds.x + target.bounds.width - 8.),
                px(target.bounds.y + 150.),
            ),
            target.id,
        )
    })?;
    drag(
        view,
        pane,
        "header",
        point(px(r.x + 45.), px(r.y + 15.)),
        end,
        cx,
    )
    .await?;
    view.update_in(cx, |app, _, _| {
        ensure!(
            app.model.active_space().floating_panel(pane).is_none(),
            "split didn't return to tiled layout"
        );
        ensure!(
            app.model.active_space().layout.pane_ids().contains(&pane),
            "split lost pane"
        );
        Ok::<_, anyhow::Error>(())
    })??;
    eprintln!("Floating smoke: merge row");
    ensure!(
        host.panel_clip_state(tab)?["radius"] == f64::from(crate::render::PANEL_CORNER_RADIUS)
            && host.panel_clip_state(tab)?["corners"] == 15,
        "tiled page radius wasn't restored"
    );
    // Merge the complete panel through a native tab-row destination.
    let (start, end) = view.update_in(cx, |app, _, _| {
        let a = app.bars[&pane].bounds;
        let b = app.bars[&tiled].bounds;
        (
            point(px(a.x + 15.), px(a.y + 24.)),
            point(px(b.x + b.width / 2.), px(b.y + 24.)),
        )
    })?;
    drag(view, pane, "bar", start, end, cx).await?;
    view.update_in(cx, |app, _, _| {
        ensure!(
            app.model.pane(pane).is_none(),
            "whole-panel tab-row merge failed"
        );
        ensure!(
            app.model
                .pane(tiled)
                .unwrap()
                .tabs
                .iter()
                .any(|t| t.id == tab),
            "merge lost tab"
        );
        Ok::<_, anyhow::Error>(())
    })??;
    ensure!(eval(&host,tab,"!!window.floatingIdentity && document.querySelector('input').value==='Live page retained'",cx).await?==true,"moves replaced native page");
    // Detach an entire tiled stack, preserving both tabs in the existing pane.
    let (whole, tabs) = view.update_in(cx, |app, _, cx| {
        let p = app
            .model
            .active_space()
            .layout
            .pane_ids()
            .into_iter()
            .find(|id| *id != tiled)
            .unwrap();
        app.model.focus_pane(p);
        app.model.add_tab("about:blank");
        let ids = app
            .model
            .pane(p)
            .unwrap()
            .tabs
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>();
        cx.notify();
        (p, ids)
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    let start = view.update_in(cx, |app, _, _| {
        let r = app.bars[&whole].bounds;
        point(px(r.x + 15.), px(r.y + 24.))
    })?;
    let end = view.update_in(cx, |_, window, _| {
        point(window.viewport_size().width - px(10.), px(350.))
    })?;
    drag(view, whole, "bar", start, end, cx).await?;
    view.update_in(cx, |app, _, _| {
        ensure!(
            app.model.active_space().floating_panel(whole).is_some(),
            "whole tiled stack didn't detach"
        );
        ensure!(
            app.model
                .pane(whole)
                .unwrap()
                .tabs
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>()
                == tabs,
            "whole-stack detachment changed tabs"
        );
        Ok::<_, anyhow::Error>(())
    })??;
    let space = view.update_in(cx, |app, _, cx| {
        let old = app.model.active_space;
        app.model.add_space("Other", "about:blank");
        cx.notify();
        old
    })?;
    pause(cx).await;
    ensure!(!host.is_visible(tab), "floating page crossed spaces");
    view.update_in(cx, |app, _, cx| {
        app.model.active_space = space;
        cx.notify();
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    let original = view.update_in(cx, |_, window, _| window.viewport_size())?;
    view.update_in(cx, |_, window, _| window.resize(size(px(720.), px(520.))))?;
    for _ in 0..3 {
        pause(cx).await;
    }
    view.update_in(cx, |app, window, _| {
        let s = window.viewport_size();
        let a = crate::render::panel_content_rect(s.width.into(), s.height.into());
        for f in &app.model.active_space().floating {
            let r = f.fitted(a);
            ensure!(
                r.x >= a.x
                    && r.y >= a.y
                    && r.x + r.width <= a.x + a.width + 0.1
                    && r.y + r.height <= a.y + a.height + 0.1,
                "window resize stranded floating panel"
            );
        }
        for pane in app.panel_geometry(window).panes {
            let tab = app.model.pane(pane.id).unwrap().active_tab;
            let native = app.host.panel_clip_state(tab)?;
            ensure!(
                app.host.is_visible(tab)
                    && (native["width"].as_f64().unwrap() - f64::from(pane.bounds.width)).abs()
                        < 1.
                    && (native["height"].as_f64().unwrap() - f64::from(pane.bounds.height)).abs()
                        < 1.,
                "native page did not follow resized floating geometry"
            );
        }
        Ok::<_, anyhow::Error>(())
    })??;
    view.update_in(cx, |_, window, _| window.resize(original))?;
    for _ in 0..3 {
        pause(cx).await;
    }
    if std::env::args().any(|a| a == "--smoke-hold-floating") {
        view.update_in(cx, |app, _, cx| {
            app.model
                .float_tab(tiled, tab, Rect::new(330., 150., 570., 430.), None);
            app.model.set_custom_title(tab, "Floating live page");
            cx.notify();
        })?;
        for _ in 0..3 {
            pause(cx).await;
        }
        println!("AXOLOTL_FLOATING_READY");
        for _ in 0..100 {
            pause(cx).await;
        }
    }
    Ok(vec![
        "native tab detachment and retained live document",
        "native panel movement and resize",
        "four window edge overlays, auto-hide, hover and pin IPC",
        "native chip reorder, split and tab-row merge",
        "space visibility and session roundtrip",
    ])
}

pub(crate) fn session_probe(app: &Entity<BrowserApp>, window: &Window, cx: &mut App) {
    let Some(stage) = std::env::args().find_map(|a| {
        a.strip_prefix("--floating-session-probe=")
            .map(str::to_owned)
    }) else {
        return;
    };
    assert!(
        std::env::var_os("AXOLOTL_DATA_DIR").is_some(),
        "Floating session probe requires isolated profile"
    );
    let view = app.downgrade();
    window.spawn(cx,async move|cx|{
        let result:Result<serde_json::Value>=async{
            pause(cx).await;
            pause(cx).await;
            view.update_in(cx,|_,window,cx|{window.activate_window();cx.activate(true);})?;
            if stage=="seed" {
                view.update_in(cx,|app,_,cx|{
                    let tiled=app.model.focused_pane().id;
                    for (i,edge) in [Some(SplitEdge::Left),Some(SplitEdge::Right),Some(SplitEdge::Top),Some(SplitEdge::Bottom),Some(SplitEdge::Left),None].into_iter().enumerate(){
                        app.model.focus_pane(tiled);let tab=app.model.add_tab("about:blank");app.model.set_custom_title(tab,&format!("Restored panel {i}"));
                        let pane=app.model.float_tab(tiled,tab,Rect::new(170.+i as f32*13.,100.+i as f32*11.,350.+i as f32*9.,260.+i as f32*7.),edge).unwrap();
                        if i==1 {app.model.pin_floating(pane);}
                    }
                    let last=app.model.active_space().floating.last().unwrap().pane;
                    app.model.reorder_floating(last,0);
                    let old=app.model.active_space;app.model.add_space("Independent space","about:blank");app.model.active_space=old;
                    cx.notify();
                })?;
            }
            for _ in 0..75 {
                pause(cx).await;
                let ready=view.update_in(cx,|app,_,_|app.model.active_space().floating.iter().all(|f|{
                    let tab=app.model.pane(f.pane).unwrap().active_tab;
                    let expected=f.edge.is_none()||f.pinned;
                    app.host.is_visible(tab)==expected&&(!expected||app.host.is_loaded(tab))
                }))?;
                if ready {break;}
            }
            for _ in 0..6{pause(cx).await;}
            if let Some(path)=std::env::args().find_map(|a|a.strip_prefix("--smoke-floating-snapshot=").map(str::to_owned)) {
                view.update_in(cx,|app,_,_|app.floating_snapshot(&path))??;
                for _ in 0..50 {
                    let manifest:serde_json::Value=serde_json::from_str(&std::fs::read_to_string(format!("{path}.json"))?)?;
                    if manifest["surfaces"].as_array().unwrap().iter().all(|s|std::path::Path::new(s["file"].as_str().unwrap()).is_file()){break;}
                    pause(cx).await;
                }
            }
            if std::env::args().any(|a|a=="--smoke-hold-floating-session") {println!("AXOLOTL_FLOATING_READY");for _ in 0..150{pause(cx).await;}}
            view.update_in(cx,|app,_,_|{
                let space=app.model.active_space();
                ensure!(space.floating.len()==6,"missing restored panels");
                let shown=space.floating.iter().map(|f|{
                    let expected=f.edge.is_none()||f.pinned;
                    let actual=app.host.is_visible(app.model.pane(f.pane).unwrap().active_tab);
                    ensure!(actual==expected,"restored visibility incorrect for {}: expected {}, actual {}, has_view {}, sleeping {}, geometry {:?}",f.pane,expected,actual,app.host.has_view(app.model.pane(f.pane).unwrap().active_tab),app.sleep.is_sleeping(app.model.pane(f.pane).unwrap().active_tab),app.model.active_space().floating);
                    Ok(serde_json::json!({"pane":f.pane,"visible":actual}))
                }).collect::<Result<Vec<_>>>()?;
                Ok(serde_json::json!({"workspace":serde_json::from_str::<serde_json::Value>(&app.model.to_json())?,"shown":shown}))
            })?
        }.await;
        match result{Ok(v)=>println!("AXOLOTL_FLOATING_SESSION {v}"),Err(e)=>{eprintln!("Floating session probe failed: {e:#}");std::process::exit(1);}}
        let _=cx.update(|_,cx|cx.quit());
    }).detach();
}
