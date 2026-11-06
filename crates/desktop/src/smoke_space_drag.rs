//! Native-window integration of measured strip geometry and drag handlers.
use crate::{app::BrowserApp, smoke::pause};
use anyhow::{Result, ensure};
use gpui::*;

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    #[cfg(target_os = "macos")]
    cx.update(|window, _| crate::macos_titlebar::check_drag_policy(window))??;
    view.update_in(cx, |app, _, cx| {
        for i in 0..12 {
            app.model.add_space(format!("Drag {i}"), "about:blank");
        }
        app.model.active_space = app.model.spaces[0].id;
        app.space_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    })?;
    pause(cx).await;
    pause(cx).await;
    let original = view.update_in(cx, |app, _, _| app.model.spaces.clone())?;
    view.update_in(cx, |app, window, cx| -> Result<()> {
        let bounds = app.space_scroll.bounds();
        let start = bounds.origin + point(px(25.), px(15.));
        let end = point(bounds.right() - px(2.), start.y);
        app.moving_window = true;
        app.arm_space_drag(original[0].id, start);
        ensure!(
            !app.moving_window,
            "space drag retained a pending window drag"
        );
        app.update_space_drag(
            &MouseMoveEvent {
                position: end,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
        ensure!(
            app.space_drop_indicator().is_some(),
            "missing insertion indicator"
        );
        ensure!(app.model.spaces == original, "mutated before drop");
        Ok(())
    })??;
    // The pointer stays stationary; the existing poll timer must keep scrolling.
    for _ in 0..30 {
        pause(cx).await;
    }
    view.update_in(cx, |app, window, cx| -> Result<()> {
        ensure!(
            app.space_scroll.offset().x < px(-100.),
            "edge scrolling stopped with stationary pointer"
        );
        ensure!(app.model.spaces == original, "scrolling changed order");
        window.dispatch_action(Box::new(gpui_component::input::Escape), cx);
        Ok(())
    })??;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| -> Result<()> {
        ensure!(app.space_drag.is_none(), "Escape did not cancel");
        ensure!(app.model.spaces == original, "Escape changed order");
        let strip = app.space_scroll.bounds();
        let origin = strip.origin + point(px(30.), px(15.));
        app.arm_space_drag(original[0].id, origin);
        let outside = point(origin.x, strip.bottom() + px(20.));
        app.update_space_drag(
            &MouseMoveEvent {
                position: outside,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
        ensure!(
            app.space_drop_indicator().is_none(),
            "indicator outside strip"
        );
        app.finish_space_drag(Some(outside), cx);
        ensure!(app.model.spaces == original, "outside drop changed order");
        app.space_scroll
            .set_offset(point(-app.space_scroll.max_offset().width, px(0.)));
        cx.notify();
        Ok(())
    })??;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| -> Result<()> {
        let strip = app.space_scroll.bounds();
        let origin = strip.origin + point(px(30.), px(15.));
        let end = point(strip.right() - px(1.), origin.y);
        app.arm_space_drag(original[0].id, origin);
        app.update_space_drag(
            &MouseMoveEvent {
                position: end,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
        app.finish_space_drag(Some(end), cx);
        ensure!(
            app.model.spaces.last().unwrap().id == original[0].id,
            "scrolled drop missed final slot"
        );
        ensure!(
            app.model.active_space == original[0].id,
            "selection changed"
        );
        app.space_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
        Ok(())
    })??;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| -> Result<()> {
        let strip = app.space_scroll.bounds();
        let origin = strip.origin + point(px(30.), px(15.));
        let end = point(strip.left() + px(1.), origin.y);
        app.arm_space_drag(original[0].id, origin);
        app.update_space_drag(
            &MouseMoveEvent {
                position: end,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
        app.finish_space_drag(Some(end), cx);
        ensure!(app.model.spaces == original, "move to start failed");
        app.arm_space_drag(original[0].id, origin);
        app.finish_space_drag(Some(origin), cx);
        ensure!(!app.suppress_space_click, "ordinary click suppressed");
        Ok(())
    })??;
    view.update_in(cx, |app, _, cx| {
        app.model
            .set_space_details(original[0].id, "Research", Some("🔬"));
        app.model.set_space_details(original[1].id, "Work", None);
        app.model.set_space_pinned(original[0].id, true);
        app.model.set_space_pinned(original[1].id, true);
        app.space_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    })?;
    pause(cx).await;
    pause(cx).await;
    let fixed = view.update_in(cx, |app, _, _| app.pinned_space_scroll.bounds())?;
    view.update_in(cx, |app, window, cx| -> Result<()> {
        let pin = app.pinned_space_scroll.bounds();
        ensure!(
            pin.size.width == px(64.),
            "Pinned tabs are not compact squares: {pin:?}"
        );
        ensure!(
            pin.right() <= app.space_scroll.bounds().left(),
            "Pins overlap regular spaces"
        );
        let start = pin.origin + point(px(10.), px(15.));
        let end = point(pin.right() - px(1.), start.y);
        app.arm_space_drag(original[0].id, start);
        app.update_space_drag(
            &MouseMoveEvent {
                position: end,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
        ensure!(
            app.space_drop_indicator().is_some(),
            "Pinned drop indicator missing"
        );
        app.finish_space_drag(Some(end), cx);
        ensure!(
            app.model.spaces[0].id == original[1].id,
            "Pinned reorder failed"
        );
        let before = (*app.model).clone();
        app.arm_space_drag(original[0].id, start);
        let regular = app.space_scroll.bounds().center();
        app.update_space_drag(
            &MouseMoveEvent {
                position: regular,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
        app.finish_space_drag(Some(regular), cx);
        ensure!(
            *app.model == before,
            "Cross-group drag changed order or pin state"
        );
        app.space_scroll
            .set_offset(point(-app.space_scroll.max_offset().width, px(0.)));
        cx.notify();
        Ok(())
    })??;
    pause(cx).await;
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.pinned_space_scroll.bounds() == fixed,
            "Regular scrolling moved pins"
        );
        ensure!(
            app.space_scroll.offset().x < px(-100.),
            "Regular strip did not scroll"
        );
        ensure!(
            workspace_core::Workspace::from_json(&app.model.to_json())? == *app.model,
            "Pinned session roundtrip failed"
        );
        Ok(())
    })??;
    let focus_before = view.update_in(cx, |app, _, _| {
        let focused = app.host.focused_page();
        app.hover_space_title(
            Some(original[0].id),
            point(fixed.right() - px(15.), px(19.)),
        );
        focused
    })?;
    for _ in 0..10 {
        pause(cx).await;
    }
    let title = std::sync::Arc::new(std::sync::Mutex::new(None));
    let response = title.clone();
    view.update_in(cx, |app, _, _| -> Result<()> {
        let tooltip = app
            .space_tooltip
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Hover title missing"))?;
        ensure!(tooltip.presented, "Native title tooltip was not presented");
        ensure!(
            app.host.focused_page() == focus_before,
            "Tooltip stole page focus"
        );
        tooltip.evaluate(
            "document.querySelector('[role=tooltip]').textContent",
            move |value| *response.lock().unwrap() = Some(value),
        )?;
        Ok(())
    })??;
    for _ in 0..10 {
        if title.lock().unwrap().is_some() {
            break;
        }
        pause(cx).await;
    }
    ensure!(
        title.lock().unwrap().as_deref() == Some("\"Research\""),
        "Tooltip did not show full displayed title: {:?}",
        title.lock().unwrap()
    );
    view.update_in(cx, |app, _, _| {
        // GPUI dispatches new-tab enter before old-tab exit when moving right.
        app.hover_space_title(Some(original[1].id), point(fixed.left() + px(15.), px(19.)));
        app.leave_space_title(original[0].id);
    })?;
    for _ in 0..10 {
        pause(cx).await;
    }
    let title = std::sync::Arc::new(std::sync::Mutex::new(None));
    let response = title.clone();
    view.update_in(cx, |app, _, _| -> Result<()> {
        let tooltip = app
            .space_tooltip
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Previous pin exit cleared the next pin tooltip"))?;
        ensure!(tooltip.presented, "Next pin tooltip was not presented");
        tooltip.evaluate(
            "document.querySelector('[role=tooltip]').textContent",
            move |value| *response.lock().unwrap() = Some(value),
        )?;
        Ok(())
    })??;
    for _ in 0..10 {
        if title.lock().unwrap().is_some() {
            break;
        }
        pause(cx).await;
    }
    ensure!(
        title.lock().unwrap().as_deref() == Some("\"Work\""),
        "Moving between pins lost the new title"
    );
    view.update_in(cx, |app, _, _| {
        app.leave_space_title(original[1].id);
        assert!(
            app.space_tooltip.is_none(),
            "Hover leave retained native title"
        );
    })?;
    view.update_in(cx, |app, _, _| {
        app.model.set_space_title(original[1].id, "");
        app.model
            .spaces
            .iter_mut()
            .find(|s| s.id == original[1].id)
            .unwrap()
            .panes[0]
            .tabs[0]
            .title
            .clear();
        app.hover_space_title(Some(original[1].id), point(fixed.left() + px(15.), px(19.)));
    })?;
    for _ in 0..10 {
        pause(cx).await;
    }
    let title = std::sync::Arc::new(std::sync::Mutex::new(None));
    let response = title.clone();
    view.update_in(cx, |app, _, _| -> Result<()> {
        app.space_tooltip
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Fallback tooltip missing"))?
            .evaluate(
                "document.querySelector('[role=tooltip]').textContent",
                move |value| *response.lock().unwrap() = Some(value),
            )?;
        Ok(())
    })??;
    for _ in 0..10 {
        if title.lock().unwrap().is_some() {
            break;
        }
        pause(cx).await;
    }
    ensure!(
        title.lock().unwrap().as_deref() == Some("\"New tab\""),
        "Empty title did not use New tab fallback"
    );
    view.update_in(cx, |app, _, _| {
        app.leave_space_title(original[1].id);
        app.model.set_space_title(original[1].id, "Work");
    })?;
    if std::env::args().any(|a| a == "--smoke-hold-space-pins") {
        view.update_in(cx, |app, window, cx| {
            app.hover_space_title(
                Some(original[0].id),
                point(fixed.right() - px(15.), px(19.)),
            );
            window.activate_window();
            cx.activate(true);
        })?;
        for _ in 0..10 {
            pause(cx).await;
        }
        println!("AXOLOTL_SPACE_PINS_READY");
        for _ in 0..100 {
            pause(cx).await;
        }
    }
    Ok(vec![
        "space drag: native window-drag isolation, indicator, stationary edge scrolling, Escape, outside cancellation, scrolled end/start drops, compact pinned tabs, group-local reorder, cross-group cancellation, fixed pins while regular strip scrolls, native full-title hover, dismissal without focus changes, persistence and stable selection",
    ])
}
