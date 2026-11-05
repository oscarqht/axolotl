//! Native-window integration of measured strip geometry and drag handlers.
use crate::{app::BrowserApp, smoke::pause};
use anyhow::{Result, ensure};
use gpui::*;

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
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
    Ok(vec![
        "space drag: indicator, stationary edge scrolling, Escape, outside cancellation, scrolled end/start drops and stable selection",
    ])
}
