//! Exercise registered GPUI separator handlers in a real native window.
//! Queued AppKit events enter GPUI; physical OS input is not synthesized.
use crate::{
    app::BrowserApp,
    smoke::{eval, pause, wait_title},
};
use anyhow::{Context as _, Result, ensure};
use gpui::*;
use workspace_core::{Axis, Divider, Geometry, SplitId};

fn geometry(app: &BrowserApp, window: &Window) -> Geometry {
    let viewport = window.viewport_size();
    app.model.active_space().layout.geometry(
        crate::render::panel_content_rect(f32::from(viewport.width), f32::from(viewport.height)),
        6.,
        100.,
    )
}

fn center(divider: Divider) -> Point<Pixels> {
    point(
        px(divider.bounds.x + divider.bounds.width / 2.),
        px(divider.bounds.y + divider.bounds.height / 2.),
    )
}

async fn dispatch(input: PlatformInput, cx: &mut AsyncWindowContext) -> Result<()> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSView};
    use objc2_foundation::{NSPoint, NSProcessInfo};
    use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let (kind, position) = match input {
        PlatformInput::MouseDown(event) => (NSEventType::LeftMouseDown, event.position),
        PlatformInput::MouseUp(event) => (NSEventType::LeftMouseUp, event.position),
        PlatformInput::MouseMove(event) => (
            if event.pressed_button.is_some() {
                NSEventType::LeftMouseDragged
            } else {
                NSEventType::MouseMoved
            },
            event.position,
        ),
        _ => anyhow::bail!("Expected mouse event"),
    };
    if kind == NSEventType::LeftMouseDown {
        cx.update(|window, cx| {
            window.activate_window();
            cx.activate(true);
        })?;
        pause(cx).await;
    }
    cx.update(|window, _| -> Result<()> {
        let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window)?.as_raw() else { anyhow::bail!("Expected AppKit window") };
        // GPUI retains the view throughout this window borrow.
        let root = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        let native = root.window().context("Missing native drag window")?;
        let y = f64::from(f32::from(position.y));
        let location = NSPoint::new(f64::from(f32::from(position.x)), if root.isFlipped() { y } else { root.frame().size.height - y });
        let location = root.convertPoint_toView(location, None);
        let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind, location, NSEventModifierFlags::empty(), NSProcessInfo::processInfo().systemUptime(), native.windowNumber(), None, 0, 1, 1.,
        ).context("Create native separator event")?;
        NSApplication::sharedApplication(MainThreadMarker::new().context("Expected main thread")?).postEvent_atStart(&event, false);
        Ok(())
    })??;
    pause(cx).await;
    Ok(())
}

async fn divider(
    view: &WeakEntity<BrowserApp>,
    id: SplitId,
    cx: &mut AsyncWindowContext,
) -> Result<Divider> {
    view.update_in(cx, |app, window, _| {
        geometry(app, window)
            .dividers
            .into_iter()
            .find(|d| d.id == id)
            .unwrap()
    })
}

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let preview = std::env::args().any(|arg| arg == "--smoke-split-snap-preview");
    // Leave room for quarter-sized children even in a nested vertical split.
    view.update_in(cx, |_, window, _| window.resize(size(px(1400.), px(1100.))))?;
    pause(cx).await;
    for axis in [Axis::Horizontal, Axis::Vertical] {
        if preview && axis != Axis::Vertical {
            continue;
        }
        for nested in [false, true] {
            if preview && !nested {
                continue;
            }
            view.update_in(cx, |app, _, cx| {
                app.model
                    .add_space(format!("Snap {axis:?} nested={nested}"), "about:blank");
                if nested {
                    app.model.split_focused(axis, "about:blank");
                }
                app.model.split_focused(axis, "about:blank");
                cx.notify();
            })?;
            pause(cx).await;
            pause(cx).await;
            let original = view.update_in(cx, |app, window, _| {
                *geometry(app, window).dividers.last().unwrap()
            })?;
            let stops: &[f32] = if preview {
                &[0.75]
            } else {
                &[0.25, 1. / 3., 0.5, 2. / 3., 0.75]
            };
            for &stop in stops {
                let current = divider(view, original.id, cx).await?;
                dispatch(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position: center(current),
                        click_count: 1,
                        ..Default::default()
                    }),
                    cx,
                )
                .await?;
                view.update_in(cx, |app, _, _| -> Result<()> {
                    ensure!(
                        app.dragging.map(|d| d.id) == Some(original.id),
                        "{axis:?} nested={nested} stop={stop}: separator mouse down at {:?} did not start drag; active={:?}", center(current), app.dragging
                    );
                    Ok(())
                })??;
                let (start, length) = match axis {
                    Axis::Horizontal => (original.container.x, original.container.width),
                    Axis::Vertical => (original.container.y, original.container.height),
                };
                let target = start + length * stop;
                let mut position = center(original);
                for offset in [-51., -50., 0., 50., 51., 0.] {
                    match axis {
                        Axis::Horizontal => position.x = px(target + offset),
                        Axis::Vertical => position.y = px(target + offset),
                    }
                    dispatch(
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position,
                            pressed_button: Some(MouseButton::Left),
                            ..Default::default()
                        }),
                        cx,
                    )
                    .await?;
                    let actual = center(divider(view, original.id, cx).await?);
                    let actual = match axis {
                        Axis::Horizontal => f32::from(actual.x),
                        Axis::Vertical => f32::from(actual.y),
                    };
                    // Wider snap ranges can overlap in small nested panels.
                    // The nearest stop wins, while pane limits still take priority.
                    let pointer = target + offset;
                    let nearest = [0.25, 1. / 3., 0.5, 2. / 3., 0.75]
                        .into_iter()
                        .map(|fraction| start + length * fraction)
                        .min_by(|a, b| (pointer - a).abs().total_cmp(&(pointer - b).abs()))
                        .unwrap();
                    let expected = if (pointer - nearest).abs() <= 50.01 {
                        nearest
                    } else {
                        pointer
                    };
                    let margin = ((length - 6.) * 0.1).max(100.) + 3.;
                    let expected = expected.clamp(start + margin, start + length - margin);
                    ensure!(
                        (actual - expected).abs() < 0.02,
                        "{axis:?} nested={nested} stop={stop} offset={offset}: center {actual}, expected {expected}"
                    );
                }
                dispatch(
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position,
                        click_count: 1,
                        ..Default::default()
                    }),
                    cx,
                )
                .await?;
                let released = divider(view, original.id, cx).await?;
                view.update_in(cx, |app, _, _| -> Result<()> {
                    ensure!(app.dragging.is_none(), "release retained separator drag");
                    Ok(())
                })??;
                dispatch(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position: position + point(px(30.), px(30.)),
                        ..Default::default()
                    }),
                    cx,
                )
                .await?;
                ensure!(
                    divider(view, original.id, cx).await? == released,
                    "free movement resized released separator"
                );
            }
            // Drag far past each edge through the same handlers: the existing
            // ratio clamp and 100-point pane minimum still take precedence.
            let edges: &[f32] = if preview { &[] } else { &[0., 1., 0.75] };
            for &edge in edges {
                let current = divider(view, original.id, cx).await?;
                dispatch(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position: center(current),
                        click_count: 1,
                        ..Default::default()
                    }),
                    cx,
                )
                .await?;
                let (start, length) = match axis {
                    Axis::Horizontal => (original.container.x, original.container.width),
                    Axis::Vertical => (original.container.y, original.container.height),
                };
                let mut position = center(original);
                match axis {
                    Axis::Horizontal => position.x = px(start + length * edge),
                    Axis::Vertical => position.y = px(start + length * edge),
                }
                dispatch(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position,
                        pressed_button: Some(MouseButton::Left),
                        ..Default::default()
                    }),
                    cx,
                )
                .await?;
                if edge != 0.75 {
                    let actual = center(divider(view, original.id, cx).await?);
                    let actual = match axis {
                        Axis::Horizontal => f32::from(actual.x),
                        Axis::Vertical => f32::from(actual.y),
                    };
                    let margin = ((length - 6.) * 0.1).max(100.) + 3.;
                    let expected = if edge == 0. {
                        start + margin
                    } else {
                        start + length - margin
                    };
                    ensure!(
                        (actual - expected).abs() < 0.02,
                        "{axis:?} nested={nested}: edge drag violated existing ratio/pane limits: {actual} != {expected}"
                    );
                }
                view.update_in(cx, |app, window, _| -> Result<()> {
                    ensure!(
                        geometry(app, window)
                            .panes
                            .iter()
                            .all(|p| p.bounds.width >= 99.99 && p.bounds.height >= 99.99),
                        "edge drag violated pane minimum"
                    );
                    Ok(())
                })??;
                dispatch(
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position,
                        click_count: 1,
                        ..Default::default()
                    }),
                    cx,
                )
                .await?;
            }
            if !preview {
                eprintln!(
                    "AXOLOTL_SPLIT_SNAP {axis:?} nested={nested}: five stops, 50px range, nearest overlapping stop, release, pane/ratio limits passed"
                );
            }
        }
    }
    // An offline, colored native-page fixture makes the final nested split visible.
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
    for (i, &tab) in tabs.iter().enumerate() {
        wait_title(&host, tab, "New tab · Axolotl", cx).await?;
        eval(&host, tab, &format!(r#"(()=>{{document.documentElement.style.background='{}';document.body.style.cssText='margin:0;height:100vh;display:grid;place-items:center;color:#163149;font:22px system-ui;text-align:center';document.body.textContent='{}';return true}})()"#, ["#ccece8", "#ffe0ba", "#ded6fa"][i], ["Outer panel · 50% split", "Nested upper panel · 75% stop", "Nested lower panel · 25% remainder"][i]), cx).await?;
    }
    cx.update(|window, cx| {
        window.activate_window();
        cx.activate(true);
    })?;
    pause(cx).await;
    eprintln!("AXOLOTL_SPLIT_SNAP_READY");
    if preview || std::env::args().any(|arg| arg == "--smoke-hold-split-snap") {
        for _ in 0..if preview { 900 } else { 150 } {
            pause(cx).await;
        }
    }
    Ok(if preview {
        vec!["native nested vertical split preview at 75%"]
    } else {
        vec![
            "queued AppKit mouse events through GPUI: both axes, nested parent-relative five stops, inclusive 50px range, nearest overlapping stop, snapped release, post-release movement and existing pane/ratio limits",
        ]
    })
}
