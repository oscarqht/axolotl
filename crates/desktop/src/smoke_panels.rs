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
    let control_pane = view.update_in(cx, |app, _, _| app.model.focused_pane().id)?;
    let triggers=bar_eval(view,control_pane,"(()=>{for(const id of ['bar','handle','add'])document.getElementById(id).dispatchEvent(new PointerEvent('pointerenter',{pointerType:'mouse'}));document.querySelector('#handle').focus();const handle=expanded;document.querySelector('#add').focus();const plus=expanded;document.activeElement?.blur();return {handle,plus}})()",cx).await?;
    ensure!(
        triggers["handle"] == false && triggers["plus"] == false,
        "Handle/+ hover or focus expanded compact bar: {triggers}"
    );
    let before = view.update_in(cx, |app, _, _| app.model.focused_pane().tabs.len())?;
    // Creating a page replaces the native control surface, so observe the
    // resulting model instead of awaiting JS in the surface being destroyed.
    view.update_in(cx, |app, _, _| {
        app.bars[&control_pane].evaluate("document.querySelector('#add').click();true", |_| {})
    })??;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        ensure!(
            app.model.focused_pane().tabs.len() == before + 1
                && app.editing.is_none()
                && !app.bars[&control_pane].expanded,
            "Plus must create a tab without expanding/editing"
        );
        app.command(crate::app::Command::CloseTab, window, cx);
        Ok(())
    })??;
    pause(cx).await;
    view.update_in(cx,|app,_,_|app.bars[&control_pane].evaluate("document.querySelector('#count').dispatchEvent(new PointerEvent('pointerenter',{pointerType:'mouse'}));true",|_|{}))??;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.bars[&control_pane].expanded)?,
        "Count hover did not expand chips"
    );
    checks.extend(check_sizing(view, cx).await?);
    view.update_in(cx, |app, _, _| {
        app.bars[&control_pane].evaluate(
            "pinned=false;document.activeElement?.blur();expand(false);true",
            |_| {},
        )
    })??;
    pause(cx).await;
    pause(cx).await;
    checks.push("only count/chip hover expands; handle/plus hover and focus stay compact; plus creates a tab without address editing");

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
        "window.barTween=null;const probe=new MutationObserver(()=>{if(!bar.classList.contains('expanded'))return;probe.disconnect();window.barTween={start:bar.getBoundingClientRect().width,viewport:innerWidth};setTimeout(()=>{const width=bar.getAnimations().find(a=>a.transitionProperty==='width');if(width)width.currentTime=70;window.barTween.middle=bar.getBoundingClientRect().width;window.barTween.sameViewport=innerWidth===window.barTween.viewport},70)});probe.observe(bar,{attributes:true,attributeFilter:['class']}); document.querySelector('#count').click(); true",
        cx,
    )
    .await?;
    pause(cx).await;
    pause(cx).await;
    let animation=bar_eval(view,pane,"({width:bar.getBoundingClientRect().width,viewport:innerWidth,tween:window.barTween,classes:bar.className,expanded,hostExpanded:state.expanded,pending:!!pendingStart,reduced:matchMedia('(prefers-reduced-motion: reduce)').matches,transition:getComputedStyle(bar).transitionDuration})",cx).await?;
    let expected = view.update_in(cx, |app, window, _| {
        let v = window.viewport_size();
        let geometry = app.model.active_space().layout.geometry(
            workspace_core::Rect::new(
                0.,
                crate::render::TOP,
                f32::from(v.width),
                f32::from(v.height) - crate::render::TOP,
            ),
            6.,
            100.,
        );
        let width = geometry
            .panes
            .iter()
            .find(|p| p.id == pane)
            .unwrap()
            .bounds
            .width;
        ensure!(
            (app.bars[&pane].bounds.width - (width - 24.)).abs() < 1.,
            "Expanded native bar did not use full panel width"
        );
        Ok(width - 32.)
    })??;
    ensure!(
        (animation["width"].as_f64().unwrap_or(0.) - f64::from(expected)).abs() < 2.,
        "Expanded pill did not fill its surface: {animation}"
    );
    if animation["reduced"] == false {
        let middle = animation["tween"]["middle"].as_f64().unwrap_or(0.);
        ensure!(
            middle > 129.
                && middle < f64::from(expected) - 1.
                && animation["tween"]["sameViewport"] == true,
            "Expansion did not animate within a stable native viewport: {animation}"
        );
    }
    let overflow=bar_eval(view,pane,"({expanded:document.querySelector('#bar').classList.contains('expanded'),overflow:document.querySelector('#bar').classList.contains('overflow'),closes:document.querySelectorAll('.close').length,rows:new Set([...document.querySelectorAll('.chip')].map(c=>c.offsetTop)).size})",cx).await?;
    ensure!(
        overflow["expanded"] == true
            && overflow["overflow"] == true
            && overflow["closes"] == 13
            && overflow["rows"] == 1,
        "Native overflow layout failed: {overflow}"
    );
    let fades=bar_eval(view,pane,"(()=>{tabs.scrollLeft=0;metrics();const start=!bar.classList.contains('fade-left')&&bar.classList.contains('fade-right');tabs.scrollLeft=(tabs.scrollWidth-tabs.clientWidth)/2;metrics();const middle=bar.classList.contains('fade-left')&&bar.classList.contains('fade-right');tabs.scrollLeft=tabs.scrollWidth;metrics();const end=bar.classList.contains('fade-left')&&!bar.classList.contains('fade-right');tabs.scrollLeft=0;metrics();return {start,middle,end,mask:getComputedStyle(tabs).maskImage||getComputedStyle(tabs).webkitMaskImage}})()",cx).await?;
    ensure!(
        fades["start"] == true
            && fades["middle"] == true
            && fades["end"] == true
            && fades["mask"]
                .as_str()
                .is_some_and(|m| m.contains("linear-gradient")),
        "Overflow fades did not track scroll edges: {fades}"
    );
    bar_eval(view,pane,"window.collapseTween=null;pinned=false;document.activeElement?.blur();expand(false);setTimeout(()=>{const width=bar.getAnimations().find(a=>a.transitionProperty==='width');if(width)width.currentTime=70;window.collapseTween={width:bar.getBoundingClientRect().width,viewport:innerWidth}},70);true",cx).await?;
    pause(cx).await;
    pause(cx).await;
    let collapsed=bar_eval(view,pane,"({width:bar.getBoundingClientRect().width,viewport:innerWidth,tween:window.collapseTween})",cx).await?;
    ensure!(
        collapsed["width"].as_f64() == Some(128.) && collapsed["viewport"].as_f64() == Some(144.),
        "Collapse did not restore compact native bounds: {collapsed}"
    );
    if animation["reduced"] == false {
        ensure!(
            collapsed["tween"]["width"]
                .as_f64()
                .is_some_and(|w| w > 129. && w < f64::from(expected))
                && collapsed["tween"]["viewport"] == animation["viewport"],
            "Native surface clipped the collapse animation: {collapsed}"
        );
    }
    bar_eval(view, pane, "expand(true);true", cx).await?;
    pause(cx).await;
    pause(cx).await;
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
    pause(cx).await;
    let last = view.update_in(cx, |app, _, _| {
        app.model.pane(pane).unwrap().tabs.last().unwrap().id
    })?;
    ensure!(
        view.update_in(cx, |app, _, _| app.active_tab())? == last,
        "Previous-tab selection did not wrap to last"
    );
    // There is no real hover/focus in unattended native tests. Re-enter the
    // expanded state after the lazy page creates a replacement native bar.
    bar_eval(view, pane, "pinned=true;expand(true);true", cx).await?;
    pause(cx).await;
    pause(cx).await;
    let revealed=bar_eval(view,pane,"(()=>{let t=document.querySelector('#tabs').getBoundingClientRect(),a=document.querySelector('.active').getBoundingClientRect();return {visible:a.left>=t.left-1&&a.right<=t.right+1,left:a.left,right:a.right,viewportLeft:t.left,viewportRight:t.right,barWidth:bar.getBoundingClientRect().width,scroll:tabs.scrollLeft,client:tabs.clientWidth,expanded,pending:!!pendingStart}})()",cx).await?;
    ensure!(
        revealed["visible"] == true,
        "Overflow did not reveal active chip: {revealed}"
    );
    bar_eval(view,pane,"document.querySelector('.active').dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true})); true",cx).await?;
    pause(cx).await;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.active_tab())? == moved,
        "Next-tab selection did not wrap to first"
    );
    checks.push("native compact count, animated full-panel expansion, scroll-edge fades, single-row chip overflow, visible close buttons, double-click editing, Escape IPC, tab wraparound and active-chip reveal");
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

/// Focused native geometry checks, independent of network/history/drag fixtures.
pub(crate) async fn check_sizing(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (control_pane, moved, before) = view.update_in(cx, |app, _, _| {
        (
            app.model.focused_pane().id,
            app.active_tab(),
            app.model.focused_pane().tabs.len(),
        )
    })?;
    view.update_in(cx, |app, _, _| {
        app.bars[&control_pane]
            .evaluate(include_str!("../assets/smoke-hover-transition.js"), |_| {})
    })??;
    pause(cx).await;
    pause(cx).await;
    pause(cx).await;
    let samples = bar_eval(view, control_pane, "window.hoverTransitionSamples", cx).await?;
    ensure!(
        samples.as_array().is_some_and(|steps| steps.len() == 6
            && steps.iter().all(|s| s["overflow"] == false
                && s["faded"] == false
                && s["arrowWidth"].as_f64() == Some(0.))),
        "Fitting chips introduced a second overflow/arrow animation: {samples}"
    );
    let single=bar_eval(view,control_pane,"pinned=true;({width:bar.getBoundingClientRect().width,overflow:bar.classList.contains('overflow'),gap:document.querySelector('#add').getBoundingClientRect().left-tabs.lastElementChild.getBoundingClientRect().right})",cx).await?;
    ensure!(
        before == 1
            && single["width"]
                .as_f64()
                .is_some_and(|w| (w - 250.).abs() < 1.)
            && single["overflow"] == false
            && single["gap"].as_f64().is_some_and(|g| g <= 12.),
        "Single-chip bar did not fit contents: {single}"
    );
    view.update_in(cx, |app, _, _| {
        ensure!(
            (app.bars[&control_pane].bounds.width - 258.).abs() < 1.,
            "Single-chip native surface is oversized"
        );
        Ok(())
    })??;
    let added = view.update_in(cx, |app, _, cx| {
        let tab = app.model.add_tab("about:blank");
        app.model.switch_tab(control_pane, moved);
        cx.notify();
        tab
    })?;
    pause(cx).await;
    pause(cx).await;
    // The first forced frame starts the CSS transition in an occluded window.
    pause(cx).await;
    let two = bar_eval(
        view,
        control_pane,
        "({width:bar.getBoundingClientRect().width,overflow:bar.classList.contains('overflow'),expanded,target:state.expandedWidth,surface:innerWidth})",
        cx,
    )
    .await?;
    ensure!(
        two["width"].as_f64().is_some_and(|w| (w - 426.).abs() < 1.) && two["overflow"] == false,
        "Two-chip bar did not fit contents: {two}"
    );
    view.update_in(cx, |app, _, cx| {
        app.model.close_tab(control_pane, added);
        cx.notify()
    })?;
    pause(cx).await;
    pause(cx).await;
    pause(cx).await;
    ensure!(
        bar_eval(view, control_pane, "bar.getBoundingClientRect().width", cx)
            .await?
            .as_f64()
            .is_some_and(|w| (w - 250.).abs() < 1.),
        "Removing chip did not restore natural width"
    );

    let extras = view.update_in(cx, |app, _, cx| {
        let tabs = (0..12)
            .map(|_| app.model.add_tab("about:blank"))
            .collect::<Vec<_>>();
        app.model.switch_tab(control_pane, moved);
        cx.notify();
        tabs
    })?;
    pause(cx).await;
    pause(cx).await;
    pause(cx).await;
    let capped=bar_eval(view,control_pane,"(()=>{tabs.scrollLeft=(tabs.scrollWidth-tabs.clientWidth)/2;metrics();return {width:bar.getBoundingClientRect().width,overflow:bar.classList.contains('overflow'),left:bar.classList.contains('fade-left'),right:bar.classList.contains('fade-right')}})()",cx).await?;
    let expected = view.update_in(cx, |app, window, _| {
        let v = window.viewport_size();
        let geometry = app.model.active_space().layout.geometry(
            workspace_core::Rect::new(
                0.,
                crate::render::TOP,
                f32::from(v.width),
                f32::from(v.height) - crate::render::TOP,
            ),
            6.,
            100.,
        );
        geometry
            .panes
            .iter()
            .find(|p| p.id == control_pane)
            .unwrap()
            .bounds
            .width
            - 32.
    })?;
    ensure!(
        capped["width"]
            .as_f64()
            .is_some_and(|w| (w - f64::from(expected)).abs() < 1.)
            && capped["overflow"] == true
            && capped["left"] == true
            && capped["right"] == true,
        "Many chips did not cap/fade at panel edges: {capped}"
    );
    view.update_in(cx, |app, _, cx| {
        for tab in extras {
            app.model.close_tab(control_pane, tab);
        }
        cx.notify()
    })?;
    pause(cx).await;
    pause(cx).await;
    pause(cx).await;
    Ok(vec![
        "one/two chips fit their contents; removal shrinks the bar; many chips cap at panel width with edge fades",
    ])
}
