//! WebKit measurements in deterministic HTTP documents, not an external browser.
use crate::{
    app::BrowserApp,
    benchmark::{Config, Samples, draw, event, js, settle, unavailable, wait},
    benchmark_fixture,
    smoke::Host,
};
use anyhow::{Result, ensure};
use gpui::*;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

async fn ready(
    host: &Host,
    tab: u64,
    old: Option<&str>,
    url: Option<&str>,
    cx: &mut AsyncWindowContext,
) -> Result<Value> {
    let start = Instant::now();
    loop {
        let value = js(host, tab, "window.bench?.navigation?.()||null", cx).await?;
        if value["ready"] == true
            && value["load"].as_f64().is_some_and(|n| n > 0.)
            && old.is_none_or(|old| value["documentId"].as_str() != Some(old))
            && url.is_none_or(|url| value["href"].as_str() == Some(url))
        {
            return Ok(value);
        }
        ensure!(
            start.elapsed() < Duration::from_secs(15),
            "web document readiness timed out"
        );
        wait(cx, 5).await;
    }
}
fn navigation(samples: &mut Samples, kind: &str, data: &Value) {
    for (field, label) in [
        ("ttfb", "ttfb_ms"),
        ("domContentLoaded", "dom_content_loaded_ms"),
        ("load", "load_ms"),
    ] {
        if let Some(value) = data[field].as_f64() {
            samples.add(
                &format!("web.{kind}.{label}"),
                "ms",
                "web_navigation_timing",
                value,
            );
        }
    }
    if let Some(value) = data["transferBytes"].as_f64() {
        samples.add(
            &format!("web.{kind}.resource_bytes"),
            "bytes",
            "web_resource_timing",
            value,
        );
    }
}
pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
    config: &Config,
) -> Result<()> {
    let f = benchmark_fixture::session(view, cx, config.large).await?;
    let origin = benchmark_fixture::server()?;
    let tab = view.update_in(cx, |app, _, _| app.active_tab())?;
    let mut samples = Samples::default();
    let mut paints = Vec::new();
    for i in 0..config.warmup + config.iterations {
        let measured = i >= config.warmup;
        for (kind, path) in [
            ("simple", "simple"),
            ("heavy_cold", "heavy?cold"),
            ("heavy_warm", "heavy?warm"),
        ] {
            let url = if path.contains('?') {
                format!("{origin}/{path}={i}")
            } else {
                format!("{origin}/{path}?n={i}")
            };
            let start = Instant::now();
            f.host.navigate(tab, &url)?;
            let data = ready(&f.host, tab, None, Some(&url), cx).await?;
            if measured {
                samples.add(
                    &format!("web.{kind}.command_to_ready_ms"),
                    "ms",
                    "native_navigation_to_document_ready",
                    start.elapsed().as_secs_f64() * 1000.,
                );
                navigation(&mut samples, kind, &data);
                if kind == "heavy_warm" {
                    for entry in data["paints"].as_array().into_iter().flatten() {
                        if entry["name"] == "first-contentful-paint"
                            && let Some(n) = entry["time"].as_f64()
                        {
                            paints.push(n);
                        }
                    }
                }
            }
        }
        let old = js(&f.host, tab, "bench.documentId", cx)
            .await?
            .as_str()
            .unwrap()
            .to_owned();
        let start = Instant::now();
        f.host.reload(tab)?;
        let data = ready(&f.host, tab, Some(&old), None, cx).await?;
        if measured {
            samples.add(
                "web.reload.command_to_ready_ms",
                "ms",
                "native_reload_to_new_document",
                start.elapsed().as_secs_f64() * 1000.,
            );
            navigation(&mut samples, "reload", &data);
        }
        // Back/forward require the exact expected URL and an initialized document.
        let back = format!("{origin}/heavy?cold={i}");
        let start = Instant::now();
        f.host.back(tab)?;
        ready(&f.host, tab, None, Some(&back), cx).await?;
        if measured {
            samples.add(
                "web.history.back_ms",
                "ms",
                "native_history_to_document_ready",
                start.elapsed().as_secs_f64() * 1000.,
            );
        }
        let forward = format!("{origin}/heavy?warm={i}");
        let start = Instant::now();
        f.host.forward(tab)?;
        ready(&f.host, tab, None, Some(&forward), cx).await?;
        if measured {
            samples.add(
                "web.history.forward_ms",
                "ms",
                "native_history_to_document_ready",
                start.elapsed().as_secs_f64() * 1000.,
            );
        }
        let data = js(&f.host, tab, "bench.dom()", cx).await?;
        ensure!(
            data["nodes"] == 2000 && data["height"].as_f64().is_some_and(|h| h > 0.),
            "DOM fixture did not execute"
        );
        if measured {
            samples.add(
                "web.dom.update_layout_ms",
                "ms",
                "javascript_style_update_and_forced_layout",
                data["duration"].as_f64().unwrap(),
            );
        }
        let data = js(&f.host, tab, "bench.compute()", cx).await?;
        ensure!(
            data["checksum"].as_u64().is_some_and(|n| n > 0),
            "JS workload invalid"
        );
        if measured {
            samples.add(
                "web.javascript.compute_ms",
                "ms",
                "fixed_5000000_iteration_checksum",
                data["duration"].as_f64().unwrap(),
            );
        }
        js(
            &f.host,
            tab,
            "bench.loopDelay=null;bench.loop().then(v=>bench.loopDelay=v);true",
            cx,
        )
        .await?;
        let start = Instant::now();
        loop {
            let delay = js(&f.host, tab, "bench.loopDelay", cx).await?;
            if let Some(delay) = delay.as_f64() {
                if measured {
                    samples.add(
                        "web.event_loop.delay_ms",
                        "ms",
                        "javascript_zero_delay_timer",
                        delay,
                    );
                }
                break;
            }
            ensure!(
                start.elapsed() < Duration::from_secs(5),
                "event-loop probe deadline"
            );
            wait(cx, 2).await;
        }
    }
    if paints.is_empty() {
        unavailable(
            "web.paint.fcp_ms",
            "ms",
            "web_paint_timing",
            "WebKit returned no first-contentful-paint entries",
        );
    } else {
        for n in paints {
            samples.add("web.paint.fcp_ms", "ms", "web_paint_timing", n);
        }
    }
    native_input(&f.host, tab, cx, &mut samples, config).await?;
    js(&f.host, tab, "bench.framesStart();true", cx).await?;
    wait(cx, 1400).await;
    let frames = js(&f.host, tab, "bench.frames", cx).await?;
    let intervals: Vec<_> = frames
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    if intervals.len() < 10 {
        unavailable(
            "web.animation.frame_interval_ms",
            "ms",
            "request_animation_frame_callback_interval",
            "fewer than 10 rAF intervals; window may be occluded (not a zero FPS result)",
        );
    } else {
        for value in intervals {
            samples.add(
                "web.animation.frame_interval_ms",
                "ms",
                "request_animation_frame_callback_interval",
                value,
            );
        }
    }
    let events=js(&f.host,tab,"({age:performance.now(),supported:bench.supported.includes('event'),durations:bench.events.filter(e=>e.interactionId).map(e=>e.duration)})",cx).await?;
    if events["supported"] == true
        && events["durations"].as_array().is_some_and(|entries| {
            entries.iter().all(|e| {
                e.as_f64().is_some_and(|n| {
                    n.is_finite() && n >= 0. && n <= events["age"].as_f64().unwrap_or(0.)
                })
            })
        })
        && events["durations"]
            .as_array()
            .is_some_and(|v| !v.is_empty())
    {
        for n in events["durations"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_f64)
        {
            samples.add(
                "web.input.event_timing_ms",
                "ms",
                "performance_event_timing_observed_interactions",
                n,
            );
        }
    } else {
        unavailable(
            "web.input.event_timing_ms",
            "ms",
            "performance_event_timing_observed_interactions",
            "Event Timing unsupported, invalid, or no qualifying trusted interactions; not an INP score",
        );
    }
    let active = cx.update(|w, _| w.is_window_active())?;
    event(
        json!({"type":"quality","window_active_at_web_end":active,"frame_interval_is_not_presentation_fps":true}),
    );
    samples.emit();
    Ok(())
}
#[cfg(target_os = "macos")]
async fn native_input(
    host: &Host,
    tab: u64,
    cx: &mut AsyncWindowContext,
    samples: &mut Samples,
    config: &Config,
) -> Result<()> {
    let mut values = Vec::new();
    for i in 0..config.warmup + config.iterations {
        js(host, tab, "scrollTo(0,0);bench.clicked=null;true", cx).await?;
        let r=js(host,tab,"(()=>{const r=response.getBoundingClientRect();return [r.x+r.width/2,r.y+r.height/2]})()",cx).await?;
        let start = Instant::now();
        host.focus(tab)?;
        host.test_click_at(tab, r[0].as_f64().unwrap(), r[1].as_f64().unwrap(), false)?;
        let mut delivered = false;
        while start.elapsed() < Duration::from_millis(750) {
            let clicked = js(host, tab, "bench.clicked||null", cx).await?;
            if clicked["trusted"] == true {
                delivered = true;
                break;
            }
            wait(cx, 2).await;
        }
        if !delivered {
            unavailable(
                "web.input.native_click_response_ms",
                "ms",
                "appkit_click_to_javascript_handler_response",
                "native click not delivered, likely foreground/occlusion interference",
            );
            return Ok(());
        }
        if i >= config.warmup {
            values.push(start.elapsed().as_secs_f64() * 1000.);
        }
    }
    for n in values {
        samples.add(
            "web.input.native_click_response_ms",
            "ms",
            "appkit_click_to_javascript_handler_response",
            n,
        );
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
async fn native_input(
    _host: &Host,
    _tab: u64,
    _cx: &mut AsyncWindowContext,
    _samples: &mut Samples,
    _config: &Config,
) -> Result<()> {
    unavailable(
        "web.input.native_click_response_ms",
        "ms",
        "appkit_click_to_javascript_handler_response",
        "trusted native adapter currently macOS only",
    );
    Ok(())
}
pub async fn energy(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
    config: &Config,
) -> Result<()> {
    let f = benchmark_fixture::session(view, cx, config.large).await?;
    let origin = benchmark_fixture::server()?;
    let tab = view.update_in(cx, |app, _, _| app.active_tab())?;
    f.host.navigate(tab, &format!("{origin}/heavy?energy=1"))?;
    ready(&f.host, tab, None, None, cx).await?;
    settle(cx).await?;
    for mode in ["idle", "active"] {
        crate::benchmark::phase(cx, &format!("energy.{mode}"), true).await?;
        let start = Instant::now();
        let mut iterations = 0;
        let mut inactive = 0;
        while start.elapsed() < Duration::from_secs(config.energy_seconds) {
            if mode == "active" {
                js(
                    &f.host,
                    tab,
                    "bench.dom();bench.compute();scrollBy(0,120);true",
                    cx,
                )
                .await?;
                view.update_in(cx, |app, _, cx| {
                    let pane = app.model.focused_pane().id;
                    app.model.cycle_tab(pane, true);
                    cx.notify();
                })?;
                draw(cx).await?;
            }
            if !cx.update(|w, _| w.is_window_active())? {
                inactive += 1;
            }
            iterations += 1;
            wait(cx, 1000).await;
        }
        if !cx.update(|w, _| w.is_window_active())? {
            inactive += 1;
        }
        event(
            json!({"type":"phase_end","id":format!("energy.{mode}"),"workload_seconds":start.elapsed().as_secs_f64(),"iterations":iterations,"inactive_ticks":inactive}),
        );
        crate::benchmark::acknowledge(cx, &format!("energy.{mode}"), "end").await?;
    }
    Ok(())
}
