//! Shell handlers and native docking release handlers, with postconditions.
use crate::{
    app::{BrowserApp, Command, DragInput, TabAction, TabTarget},
    benchmark::{Config, Samples, draw, settle, timed, wait},
    benchmark_fixture,
};
use anyhow::{Result, ensure};
use gpui::*;
use std::time::Instant;
use workspace_core::{Axis, PaneId, Rect, SplitEdge, TabId};

fn rect(app: &BrowserApp, window: &Window, pane: PaneId) -> Rect {
    let size = window.viewport_size();
    app.model
        .active_space()
        .layout
        .geometry(
            Rect::new(
                0.,
                crate::render::TOP,
                f32::from(size.width),
                f32::from(size.height) - crate::render::TOP,
            ),
            6.,
            100.,
        )
        .panes
        .iter()
        .find(|p| p.id == pane)
        .unwrap()
        .bounds
}
#[expect(
    clippy::too_many_arguments,
    reason = "explicit native release target and drag state in a test fixture"
)]
pub(crate) fn release(
    app: &mut BrowserApp,
    window: &mut Window,
    cx: &mut Context<BrowserApp>,
    source: PaneId,
    tab: TabId,
    destination: PaneId,
    edge: Option<SplitEdge>,
    whole: bool,
) {
    let r = rect(app, window, destination);
    let (x, y) = match edge {
        Some(SplitEdge::Left) => (0.02, 0.5),
        Some(SplitEdge::Right) => (0.98, 0.5),
        Some(SplitEdge::Top) => (0.5, 0.02),
        Some(SplitEdge::Bottom) => (0.5, 0.9),
        None => (0.5, 0.5),
    };
    let point = point(px(r.x + r.width * x), px(r.y + r.height * y));
    app.arm_tab_drag(source, tab, point);
    let drag = app.tab_drag.as_mut().unwrap();
    drag.active = true;
    drag.input = DragInput::NativeBar;
    drag.whole = whole;
    app.finish_native_tab_drag(source, Some(point), window, cx);
}
pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
    config: &Config,
) -> Result<()> {
    let f = benchmark_fixture::session(view, cx, config.large).await?;
    // Materialize both tabs and spaces before warm-switch measurements.
    for tab in f.tabs.iter().take(2) {
        view.update_in(cx, |app, window, cx| {
            app.select_target(TabTarget::Page(f.first, *tab), window, cx)
        })?;
        draw(cx).await?;
    }
    for space in &f.spaces[..2] {
        view.update_in(cx, |app, window, cx| {
            app.select_target(TabTarget::Space(*space), window, cx)
        })?;
        draw(cx).await?;
    }
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Space(f.spaces[0]), window, cx)
    })?;
    draw(cx).await?;
    settle(cx).await?;
    let mut samples = Samples::default();
    for i in 0..config.warmup + config.iterations {
        let measured = i >= config.warmup;
        for index in [1, 0] {
            timed(
                view,
                cx,
                &mut samples,
                "action.space.switch",
                measured,
                |app, window, cx| {
                    app.select_target(TabTarget::Space(f.spaces[index]), window, cx);
                    ensure!(
                        app.model.active_space == f.spaces[index],
                        "space switch did not run"
                    );
                    Ok(())
                },
            )
            .await?;
        }
        let before = view.update_in(cx, |app, _, _| app.model.spaces.len())?;
        timed(
            view,
            cx,
            &mut samples,
            "action.space.create",
            measured,
            |app, window, cx| {
                app.command(Command::NewSpace, window, cx);
                ensure!(
                    app.model.spaces.len() == before + 1,
                    "space creation failed"
                );
                Ok(())
            },
        )
        .await?;
        let created = view.update_in(cx, |app, _, _| app.model.active_space)?;
        timed(
            view,
            cx,
            &mut samples,
            "action.space.close",
            measured,
            |app, window, cx| {
                app.tab_action(TabTarget::Space(created), TabAction::Close, window, cx);
                ensure!(app.model.spaces.len() == before, "space close failed");
                Ok(())
            },
        )
        .await?;
        view.update_in(cx, |app, window, cx| {
            app.select_target(TabTarget::Space(f.spaces[0]), window, cx)
        })?;
        draw(cx).await?;
        for pane in [f.second, f.first] {
            timed(
                view,
                cx,
                &mut samples,
                "action.panel.focus",
                measured,
                |app, window, cx| {
                    app.focus_pane(pane, window, cx);
                    ensure!(app.model.focused_pane().id == pane, "panel focus failed");
                    Ok(())
                },
            )
            .await?;
        }
        let tab_count =
            view.update_in(cx, |app, _, _| app.model.pane(f.first).unwrap().tabs.len())?;
        timed(
            view,
            cx,
            &mut samples,
            "action.tab.create",
            measured,
            |app, window, cx| {
                app.command(Command::NewTab, window, cx);
                ensure!(
                    app.model.pane(f.first).unwrap().tabs.len() == tab_count + 1,
                    "tab creation failed"
                );
                Ok(())
            },
        )
        .await?;
        let new_tab = view.update_in(cx, |app, _, _| app.active_tab())?;
        timed(
            view,
            cx,
            &mut samples,
            "action.tab.close",
            measured,
            |app, window, cx| {
                app.command(Command::CloseTab, window, cx);
                ensure!(app.model.tab(new_tab).is_none(), "tab close failed");
                Ok(())
            },
        )
        .await?;
        for tab in &f.tabs[..2] {
            timed(
                view,
                cx,
                &mut samples,
                "action.tab.switch",
                measured,
                |app, window, cx| {
                    app.select_target(TabTarget::Page(f.first, *tab), window, cx);
                    ensure!(app.active_tab() == *tab, "tab switch failed");
                    Ok(())
                },
            )
            .await?;
        }
        timed(
            view,
            cx,
            &mut samples,
            "action.panel.create",
            measured,
            |app, window, cx| {
                app.command(Command::Split(Axis::Vertical), window, cx);
                Ok(())
            },
        )
        .await?;
        let temporary = view.update_in(cx, |app, _, _| app.model.focused_pane().id)?;
        ensure!(
            temporary != f.first && temporary != f.second,
            "split did not create a panel"
        );
        timed(
            view,
            cx,
            &mut samples,
            "action.panel.close",
            measured,
            |app, _, cx| {
                ensure!(app.model.close_pane(temporary), "panel close failed");
                cx.notify();
                Ok(())
            },
        )
        .await?;
        view.update_in(cx, |app, window, cx| app.focus_pane(f.first, window, cx))?;
        draw(cx).await?;
        for (edge, name) in [
            (SplitEdge::Left, "left"),
            (SplitEdge::Right, "right"),
            (SplitEdge::Top, "top"),
            (SplitEdge::Bottom, "bottom"),
        ] {
            let moved = f.tabs[0];
            timed(
                view,
                cx,
                &mut samples,
                &format!("action.tab.split.{name}"),
                measured,
                |app, window, cx| {
                    release(app, window, cx, f.first, moved, f.second, Some(edge), false);
                    ensure!(
                        app.model.active_space().panes.len() == 3,
                        "split drop failed"
                    );
                    Ok(())
                },
            )
            .await?;
            let source = view.update_in(cx, |app, _, _| app.model.focused_pane().id)?;
            timed(
                view,
                cx,
                &mut samples,
                "action.tab.dock.center",
                measured,
                |app, window, cx| {
                    release(app, window, cx, source, moved, f.first, None, false);
                    ensure!(
                        app.model.active_space().panes.len() == 2
                            && app
                                .model
                                .pane(f.first)
                                .unwrap()
                                .tabs
                                .iter()
                                .any(|t| t.id == moved),
                        "center docking failed"
                    );
                    Ok(())
                },
            )
            .await?;
        }
        timed(
            view,
            cx,
            &mut samples,
            "action.panel.dock.edge",
            measured,
            |app, window, cx| {
                let tab = app.model.pane(f.first).unwrap().active_tab;
                release(
                    app,
                    window,
                    cx,
                    f.first,
                    tab,
                    f.second,
                    Some(if i % 2 == 0 {
                        SplitEdge::Right
                    } else {
                        SplitEdge::Left
                    }),
                    true,
                );
                ensure!(
                    app.model.active_space().panes.len() == 2,
                    "panel edge docking lost panel"
                );
                let source = rect(app, window, f.first);
                let destination = rect(app, window, f.second);
                ensure!(
                    if i % 2 == 0 {
                        source.x > destination.x
                    } else {
                        source.x < destination.x
                    },
                    "panel edge docking did not move to target side"
                );
                Ok(())
            },
        )
        .await?;
        let divider = view.update_in(cx, |app, window, _| {
            let size = window.viewport_size();
            app.model
                .active_space()
                .layout
                .geometry(
                    Rect::new(
                        0.,
                        crate::render::TOP,
                        f32::from(size.width),
                        f32::from(size.height) - crate::render::TOP,
                    ),
                    6.,
                    100.,
                )
                .dividers[0]
                .id
        })?;
        timed(
            view,
            cx,
            &mut samples,
            "action.split.resize",
            measured,
            |app, _, cx| {
                ensure!(
                    app.model
                        .resize_split(divider, if i % 2 == 0 { 0.4 } else { 0.6 }),
                    "resize failed"
                );
                cx.notify();
                Ok(())
            },
        )
        .await?;
        timed(
            view,
            cx,
            &mut samples,
            "action.pin.open",
            measured,
            |app, window, cx| {
                app.toggle_pin(f.pins[i % 4], window, cx);
                ensure!(app.pins.open.is_some(), "pin open failed");
                Ok(())
            },
        )
        .await?;
        timed(
            view,
            cx,
            &mut samples,
            "action.pin.close",
            measured,
            |app, window, cx| {
                app.dismiss_pin(window, cx);
                ensure!(app.pins.open.is_none(), "pin close failed");
                Ok(())
            },
        )
        .await?;
        view.update_in(cx, |app, window, cx| app.focus_pane(f.first, window, cx))?;
        draw(cx).await?;
        wait(cx, 10).await;
    }
    crate::benchmark_extra_actions::run(view, cx, config, &f, &mut samples).await?;
    // Verify that moving/docking preserved the original native document.
    let moved = f.tabs[0];
    crate::benchmark::js(&f.host, moved, "window.benchIdentity='retained';true", cx).await?;
    view.update_in(cx, |app, window, cx| {
        release(app, window, cx, f.first, moved, f.second, None, false);
        release(app, window, cx, f.second, moved, f.first, None, false);
    })?;
    draw(cx).await?;
    ensure!(
        crate::benchmark::js(&f.host, moved, "window.benchIdentity", cx).await? == "retained",
        "docking replaced the page"
    );
    ipc(view, cx, &f, &mut samples, config).await?;
    samples.emit();
    Ok(())
}
async fn ipc(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
    f: &benchmark_fixture::Session,
    samples: &mut Samples,
    config: &Config,
) -> Result<()> {
    for i in 0..config.warmup + config.iterations {
        let current = view.update_in(cx, |app, _, _| app.active_tab())?;
        let tab = if current == f.tabs[0] {
            f.tabs[1]
        } else {
            f.tabs[0]
        };
        let start = Instant::now();
        view.update_in(cx, |app, _, _| {
            app.bars[&f.first].evaluate(
                &format!("document.querySelector('.chip[data-id=\"{tab}\"]').click()"),
                |_| {},
            )
        })??;
        loop {
            if view.update_in(cx, |app, _, _| app.active_tab() == tab)? {
                break;
            }
            ensure!(
                start.elapsed().as_secs() < 5,
                "native bar IPC selection timed out"
            );
            wait(cx, 2).await;
        }
        draw(cx).await?;
        ensure!(
            f.host.is_visible(tab),
            "selected native page not visible after draw"
        );
        if i >= config.warmup {
            samples.add(
                "action.tab.ipc_select",
                "ms",
                "native_control_script_to_view_visibility",
                start.elapsed().as_secs_f64() * 1000.,
            );
        }
    }
    Ok(())
}
