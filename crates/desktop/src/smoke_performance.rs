//! Repeatable release-build native UI timings with a synthetic, private session.
//! Draw timings measure CPU submission, not display presentation latency.
use crate::{
    app::{BrowserApp, TabTarget},
    smoke::{bar_eval, eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    time::{Duration, Instant},
};
use workspace_core::Workspace;

// An occluded profiling window must not compare App Nap scheduling against
// foreground work. This token exists only for this opt-in benchmark, in both
// baseline and improved builds; normal application sleep policy is untouched.
#[cfg(target_os = "macos")]
struct BenchmarkActivity(
    objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>,
);
#[cfg(target_os = "macos")]
impl BenchmarkActivity {
    fn new() -> Self {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
        Self(
            NSProcessInfo::processInfo().beginActivityWithOptions_reason(
                NSActivityOptions::UserInitiatedAllowingIdleSystemSleep
                    | NSActivityOptions::LatencyCritical,
                &NSString::from_str("Axolotl native UI performance measurement"),
            ),
        )
    }
}
#[cfg(target_os = "macos")]
impl Drop for BenchmarkActivity {
    fn drop(&mut self) {
        // This is the activity token returned by beginActivity above.
        unsafe {
            objc2_foundation::NSProcessInfo::processInfo().endActivity(&self.0);
        }
    }
}

thread_local! {
    static SAMPLES: RefCell<Option<BTreeMap<&'static str, Vec<f64>>>> = const { RefCell::new(None) };
}
pub struct Timing(&'static str, Option<Instant>);
pub fn time(name: &'static str) -> Timing {
    Timing(
        name,
        SAMPLES.with(|s| s.borrow().is_some().then(Instant::now)),
    )
}
impl Drop for Timing {
    fn drop(&mut self) {
        if let Some(start) = self.1 {
            record(self.0, start.elapsed().as_secs_f64() * 1000.);
        }
    }
}
fn record(name: &'static str, ms: f64) {
    SAMPLES.with(|s| {
        if let Some(samples) = s.borrow_mut().as_mut() {
            samples.entry(name).or_default().push(ms);
        }
    });
}
fn summary(mut samples: Vec<f64>) -> Value {
    samples.sort_by(f64::total_cmp);
    if samples.is_empty() {
        return json!({"n":0});
    }
    let percentile = |p: f64| samples[((samples.len() - 1) as f64 * p).ceil() as usize];
    json!({"n":samples.len(), "p50_ms":percentile(0.5), "p95_ms":percentile(0.95), "max_ms":samples.last(), "total_ms":samples.iter().sum::<f64>()})
}
fn finish(phase: &str) {
    let samples = SAMPLES.with(|s| s.borrow_mut().take().unwrap_or_default());
    let metrics: BTreeMap<_, _> = samples
        .into_iter()
        .map(|(name, values)| (name, summary(values)))
        .collect();
    println!("AXOLOTL_PERF {}", json!({"phase":phase,"metrics":metrics}));
}
fn start() {
    SAMPLES.with(|s| *s.borrow_mut() = Some(BTreeMap::new()));
}
async fn draw(cx: &mut AsyncWindowContext) -> Result<()> {
    cx.update(|window, cx| {
        let started = Instant::now();
        window.refresh();
        window.draw(cx).clear();
        record("draw_cpu", started.elapsed().as_secs_f64() * 1000.);
    })?;
    Ok(())
}
async fn tick(cx: &mut AsyncWindowContext, ms: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(ms))
        .await;
}

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    #[cfg(target_os = "macos")]
    let _activity = BenchmarkActivity::new();
    println!(
        "AXOLOTL_PERF {}",
        json!({"phase":"measurement_policy", "app_nap":"suppressed during benchmark only", "system_idle_sleep":"allowed"})
    );
    for (label, spaces, tabs) in [("small", 2, 4), ("large", 12, 40)] {
        let (host, pane, ids, space_ids, pins) = view.update_in(cx, |app, _, cx| {
            let mut model = Workspace::new("about:blank");
            for space in 0..spaces {
                if space > 0 {
                    model.add_space("Performance fixture", "about:blank");
                }
                for _ in 1..tabs {
                    model.add_tab("about:blank");
                }
            }
            let space_ids: Vec<_> = model.spaces.iter().map(|s| s.id).collect();
            model.active_space = space_ids[0];
            let pane = model.focused_pane().id;
            for _ in 0..4 {
                let tab = model.add_tab("about:blank");
                model.pin_tab(pane, tab);
            }
            let pins: Vec<_> = model.pinned_tabs.iter().map(|p| p.tab.id).collect();
            let ids: Vec<_> = model.focused_pane().tabs.iter().map(|t| t.id).collect();
            app.host.retain(&Default::default());
            app.bars.clear();
            app.chrome_views.clear();
            app.model = model.into();
            app.editing = None;
            app.preferences.auto_sleep = false;
            cx.notify();
            (app.host.clone(), pane, ids, space_ids, pins)
        })?;
        draw(cx).await?;
        wait_title(&host, *ids.last().unwrap(), "New tab · Axolotl", cx).await?;
        for _ in 0..5 {
            pause(cx).await;
        }
        println!(
            "AXOLOTL_PERF {}",
            json!({"phase":format!("{label}_idle_begin"),"pid":std::process::id(),"spaces":spaces,"tabs":spaces*tabs,"pins":pins.len()})
        );
        start();
        // No forced draws while measuring idle polling and process resources.
        tick(cx, 10000).await;
        finish(&format!("{label}_idle"));

        start();
        for id in ids.iter().take(4) {
            view.update_in(cx, |app, window, cx| {
                app.select_target(TabTarget::Page(pane, *id), window, cx)
            })?;
            draw(cx).await?;
            wait_title(&host, *id, "New tab · Axolotl", cx).await?;
            tick(cx, 80).await;
        }
        finish(&format!("{label}_cold_tabs"));
        // A live page identity must survive tab switching.
        eval(&host, ids[0], "window.perfIdentity='retained'; true", cx).await?;
        start();
        for i in 0..40 {
            let started = Instant::now();
            view.update_in(cx, |app, window, cx| {
                app.select_target(TabTarget::Page(pane, ids[i % 4]), window, cx)
            })?;
            draw(cx).await?;
            record("switch_submit", started.elapsed().as_secs_f64() * 1000.);
            tick(cx, 20).await;
        }
        finish(&format!("{label}_warm_tabs"));
        // Exercise the real native control IPC queue. The script invokes the
        // control's normal click handler; this is not physical mouse latency.
        start();
        for i in 0..30 {
            let target = ids[i % 2];
            let started = Instant::now();
            view.update_in(cx, |app, _, _| {
                app.bars[&pane].evaluate(
                    &format!("document.querySelector('.chip[data-id=\"{target}\"]').click()"),
                    |_| {},
                )
            })??;
            let mut selected = false;
            for _ in 0..1000 {
                if view.update_in(cx, |app, _, _| app.active_tab() == target)? {
                    selected = true;
                    break;
                }
                tick(cx, 1).await;
            }
            ensure!(selected, "native bar select did not reach model");
            draw(cx).await?;
            record("ipc_select", started.elapsed().as_secs_f64() * 1000.);
            tick(cx, (i % 7 + 1) as u64).await;
        }
        finish(&format!("{label}_ipc_tabs"));
        ensure!(
            eval(&host, ids[0], "window.perfIdentity", cx).await? == "retained",
            "switching recreated a page"
        );
        start();
        for i in 0..20 {
            let started = Instant::now();
            view.update_in(cx, |app, window, cx| {
                app.select_target(TabTarget::Space(space_ids[i % spaces]), window, cx)
            })?;
            draw(cx).await?;
            record("switch_submit", started.elapsed().as_secs_f64() * 1000.);
            tick(cx, 80).await;
        }
        finish(&format!("{label}_spaces"));
        start();
        for i in 0..40 {
            let started = Instant::now();
            view.update_in(cx, |app, window, cx| {
                app.select_target(TabTarget::Space(space_ids[i % spaces]), window, cx)
            })?;
            draw(cx).await?;
            record("switch_submit", started.elapsed().as_secs_f64() * 1000.);
            tick(cx, 20).await;
        }
        finish(&format!("{label}_warm_spaces"));
        start();
        for i in 0..20 {
            let started = Instant::now();
            view.update_in(cx, |app, window, cx| {
                app.toggle_pin(pins[i % pins.len()], window, cx)
            })?;
            draw(cx).await?;
            record("pin_submit", started.elapsed().as_secs_f64() * 1000.);
            tick(cx, 50).await;
        }
        view.update_in(cx, |app, window, cx| app.dismiss_pin(window, cx))?;
        finish(&format!("{label}_pins"));
        view.update_in(cx, |app, window, cx| {
            app.select_target(TabTarget::Space(space_ids[0]), window, cx)
        })?;
        for _ in 0..5 {
            pause(cx).await;
        }
        bar_eval(view, pane, "window.perfFrames=[]; window.perfLast=0; window.perfUntil=performance.now()+2200; requestAnimationFrame(function frame(t){if(perfLast)perfFrames.push(t-perfLast);perfLast=t;if(t<perfUntil)requestAnimationFrame(frame)}); true", cx).await?;
        start();
        for i in 0..6 {
            bar_eval(
                view,
                pane,
                &format!("pinned=true; expand({}); true", i % 2 == 0),
                cx,
            )
            .await?;
            tick(cx, 200).await;
        }
        tick(cx, 400).await;
        finish(&format!("{label}_bar_animation"));
        let frames = bar_eval(view, pane, "perfFrames", cx).await?;
        println!(
            "AXOLOTL_PERF {}",
            json!({"phase":format!("{label}_webkit_frames"),"raf_interval":summary(frames.as_array().unwrap().iter().filter_map(Value::as_f64).collect())})
        );
        let divider = view.update_in(cx, |app, _, cx| {
            app.model
                .split_focused(workspace_core::Axis::Horizontal, "about:blank");
            cx.notify();
            app.model
                .active_space()
                .layout
                .geometry(workspace_core::Rect::new(0., 34., 1200., 800.), 6., 100.)
                .dividers[0]
                .id
        })?;
        for _ in 0..5 {
            pause(cx).await;
        }
        start();
        for i in 0..60 {
            let started = Instant::now();
            view.update_in(cx, |app, _, cx| {
                app.model
                    .resize_split(divider, 0.3 + (i % 30) as f32 / 100.);
                cx.notify();
            })?;
            draw(cx).await?;
            record("resize_submit", started.elapsed().as_secs_f64() * 1000.);
            tick(cx, 16).await;
        }
        finish(&format!("{label}_resize"));
    }
    // Worst-case history lookup: no match in a mature visit log.
    view.update_in(cx, |app, _, cx| -> Result<()> {
        let db = app.browsing_history.as_ref().unwrap();
        for i in 0..50000 {
            db.record(
                &format!("https://fixture.test/{i}"),
                "Performance history fixture",
                i,
            )?;
        }
        let mut values = Vec::new();
        for _ in 0..10 {
            let started = Instant::now();
            ensure!(db.search("no-such-page").unwrap().is_empty());
            values.push(started.elapsed().as_secs_f64() * 1000.);
        }
        println!(
            "AXOLOTL_PERF {}",
            json!({"phase":"history_50k_no_match", "search":summary(values)})
        );
        app.editing = Some(TabTarget::Space(app.model.active_space));
        let mut dispatch = Vec::new();
        for _ in 0..10 {
            let started = Instant::now();
            app.search_history("no-such-page", cx);
            dispatch.push(started.elapsed().as_secs_f64() * 1000.);
        }
        println!(
            "AXOLOTL_PERF {}",
            json!({"phase":"history_ui_dispatch", "dispatch":summary(dispatch)})
        );
        app.dismiss_history();
        app.editing = None;
        Ok(())
    })??;
    Ok(vec![
        "release performance fixture: idle, cold/warm tabs, spaces, native bar animation rAF, retained page identity and 50k-visit history",
    ])
}
