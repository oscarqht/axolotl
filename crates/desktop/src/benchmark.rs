//! Versioned, opt-in native benchmark protocol. No production runtime behavior.
use crate::{app::BrowserApp, smoke::Host};
use anyhow::{Context as _, Result, ensure};
use gpui::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const VERSION: u32 = 1;
// Keep this file list aligned with suite.json. Hash the stimulus, not production
// handlers: future implementation changes must remain comparable.
pub fn fingerprint() -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for (name, bytes) in [
        (
            "crates/desktop/src/benchmark.rs",
            include_bytes!("benchmark.rs").as_slice(),
        ),
        (
            "crates/desktop/src/benchmark_actions.rs",
            include_bytes!("benchmark_actions.rs").as_slice(),
        ),
        (
            "crates/desktop/src/benchmark_extra_actions.rs",
            include_bytes!("benchmark_extra_actions.rs").as_slice(),
        ),
        (
            "crates/desktop/src/benchmark_fixture.rs",
            include_bytes!("benchmark_fixture.rs").as_slice(),
        ),
        (
            "crates/desktop/src/benchmark_web.rs",
            include_bytes!("benchmark_web.rs").as_slice(),
        ),
        (
            "crates/desktop/assets/benchmark-page.html",
            include_bytes!("../assets/benchmark-page.html").as_slice(),
        ),
    ] {
        hash.update(name.as_bytes());
        hash.update([0]);
        hash.update(bytes);
    }
    format!("{:x}", hash.finalize())
}
pub struct Config {
    pub iterations: usize,
    pub warmup: usize,
    pub large: bool,
    pub suite: String,
    pub energy_seconds: u64,
}
impl Config {
    fn read() -> Result<Self> {
        fn number(name: &str, default: usize, min: usize, max: usize) -> Result<usize> {
            let value = std::env::var(name)
                .ok()
                .map(|v| v.parse())
                .transpose()?
                .unwrap_or(default);
            ensure!((min..=max).contains(&value), "{name} must be {min}..{max}");
            Ok(value)
        }
        let suite = std::env::var("AXOLOTL_BENCH_SUITE").unwrap_or("standard".into());
        ensure!(
            ["standard", "actions", "web", "energy"].contains(&suite.as_str()),
            "unknown benchmark suite"
        );
        let size = std::env::var("AXOLOTL_BENCH_SIZE").unwrap_or("small".into());
        ensure!(
            ["small", "large"].contains(&size.as_str()),
            "unknown session size"
        );
        Ok(Self {
            iterations: number("AXOLOTL_BENCH_ITERATIONS", 20, 1, 100)?,
            warmup: number("AXOLOTL_BENCH_WARMUP", 3, 0, 20)?,
            large: size == "large",
            suite,
            energy_seconds: number("AXOLOTL_BENCH_ENERGY_SECONDS", 300, 3, 3600)? as u64,
        })
    }
}
pub fn event(value: Value) {
    println!("AXOLOTL_BENCH {value}");
}
pub async fn wait(cx: &mut AsyncWindowContext, ms: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(ms))
        .await;
}
pub async fn phase(cx: &mut AsyncWindowContext, id: &str, start: bool) -> Result<()> {
    if !start && !cx.update(|w, _| w.is_window_active())? {
        event(json!({"type":"quality","comparable":false,"foreground_interference":true}));
    }
    event(json!({"type":if start {"phase_start"} else {"phase_end"},"id":id}));
    acknowledge(cx, id, if start { "start" } else { "end" }).await
}
pub async fn acknowledge(cx: &mut AsyncWindowContext, id: &str, boundary: &str) -> Result<()> {
    if let Some(directory) = std::env::var_os("AXOLOTL_BENCH_ACK_DIR") {
        let path = std::path::PathBuf::from(directory).join(format!("{id}.{boundary}"));
        let deadline = Instant::now() + Duration::from_secs(15);
        while !path.is_file() {
            ensure!(
                Instant::now() < deadline,
                "benchmark collector acknowledgement timed out: {id} {boundary}"
            );
            wait(cx, 5).await;
        }
    }
    Ok(())
}
pub async fn draw(cx: &mut AsyncWindowContext) -> Result<()> {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })?;
    Ok(())
}
pub async fn settle(cx: &mut AsyncWindowContext) -> Result<()> {
    for _ in 0..4 {
        wait(cx, 100).await;
        draw(cx).await?;
    }
    Ok(())
}
#[derive(Default)]
pub struct Samples(BTreeMap<String, (String, String, Vec<f64>)>);
impl Samples {
    pub fn add(&mut self, id: &str, unit: &str, scope: &str, value: f64) {
        if value.is_finite() && value >= 0. {
            self.0
                .entry(id.into())
                .or_insert_with(|| (unit.into(), scope.into(), vec![]))
                .2
                .push(value);
        }
    }
    pub fn emit(self) {
        for (id, (unit, scope, samples)) in self.0 {
            event(
                json!({"type":"metric", "metric":{"id":id,"unit":unit,"scope":scope,"status":"available","required":!(id.starts_with("web.paint.") || id.starts_with("web.input.") || id=="web.animation.frame_interval_ms"),"samples":samples}}),
            );
        }
    }
}
pub fn unavailable(id: &str, unit: &str, scope: &str, reason: &str) {
    event(
        json!({"type":"metric","metric":{"id":id,"unit":unit,"scope":scope,"status":"unavailable","required":false,"reason":reason,"samples":[]}}),
    );
}
pub async fn timed(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
    samples: &mut Samples,
    id: &str,
    measured: bool,
    operation: impl FnOnce(&mut BrowserApp, &mut Window, &mut Context<BrowserApp>) -> Result<()>,
) -> Result<()> {
    let start = Instant::now();
    view.update_in(cx, operation)??;
    draw(cx).await?;
    if measured {
        samples.add(
            id,
            "ms",
            "command_to_draw_submission",
            start.elapsed().as_secs_f64() * 1000.,
        );
    }
    Ok(())
}
/// Fast asynchronous evaluation, with a deadline. Polling adds up to 2 ms to
/// command-to-ready measurements; Navigation Timing fields use WebKit's clock.
pub async fn js(host: &Host, tab: u64, script: &str, cx: &mut AsyncWindowContext) -> Result<Value> {
    let response = Arc::new(Mutex::new(None));
    let target = response.clone();
    host.evaluate(
        tab,
        script,
        Box::new(move |value| *target.lock().unwrap() = Some(value)),
    )?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(value) = response.lock().unwrap().take() {
            return serde_json::from_str(&value)
                .with_context(|| format!("benchmark JS evaluation failed: {value}"));
        }
        ensure!(
            Instant::now() < deadline,
            "benchmark JS evaluation deadline"
        );
        wait(cx, 2).await;
    }
}
pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let config = Config::read()?;
    #[cfg(target_os = "macos")]
    let _activity = if config.suite != "energy" {
        Some(crate::smoke_performance::BenchmarkActivity::new())
    } else {
        None
    };
    cx.update(|window, cx| {
        window.activate_window();
        cx.activate(true);
    })?;
    wait(cx, 250).await;
    let state=view.update_in(cx,|app,window,_| json!({"viewport":[f32::from(window.viewport_size().width),f32::from(window.viewport_size().height)],"scale_factor":window.scale_factor(),"active":window.is_window_active(),"pid":std::process::id(),"protection":app.host.protection().borrow().policy.enabled}))?;
    if state["active"] != true {
        event(json!({"type":"quality","comparable":false,"foreground_activation_failed":true}));
    }
    event(
        json!({"type":"run_start","native_sha256":fingerprint(),"suite_version":VERSION,"session_size":if config.large {"large"} else {"small"},"state":state,"app_nap":if config.suite=="energy" {"natural"} else {"suppressed"}}),
    );
    if config.suite == "standard" || config.suite == "actions" {
        phase(cx, "actions", true).await?;
        crate::benchmark_actions::run(view, cx, &config).await?;
        phase(cx, "actions", false).await?;
    }
    if config.suite == "standard" || config.suite == "web" {
        phase(cx, "web", true).await?;
        crate::benchmark_web::run(view, cx, &config).await?;
        phase(cx, "web", false).await?;
    }
    if config.suite == "energy" {
        crate::benchmark_web::energy(view, cx, &config).await?;
    }
    event(json!({"type":"run_end","status":"passed"}));
    Ok(vec![
        "versioned native actions/webpage/energy benchmark completed with validated postconditions",
    ])
}
