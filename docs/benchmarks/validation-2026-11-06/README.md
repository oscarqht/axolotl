# Diagnostic validation — 2026-11-06

**These are not clean performance baselines.** This Mac was on AC power, had a
one-minute load average well above its 10 logical CPUs, and the native window
lost foreground status. Reports are deliberately `incomplete`; raw timings are
retained for diagnostics and excluded from pooled/gating measurements.

The release native suite completed all 23 required action workloads and 18
required webpage workloads for both small and large sessions. Each workload ran
with one warmup iteration and three measured iterations. Both launches ended
with successful native postconditions and collected separate app/WebKit CPU and
RSS observations. Optional browser API availability is retained per metric.

- [Standard suite raw report](standard.json)
- [Small native output](small.native.stdout.log) and [stderr](small.native.stderr.log)
- [Large native output](large.native.stdout.log) and [stderr](large.native.stderr.log)
- [Energy workflow report](energy.json)

The three-second idle/active energy validation exercises phase synchronization,
resource collection and selected battery-state observation. Battery discharge
and SoC power were **not requested or measured** in that short validation.
`--battery` preflight correctly rejects AC power. `--power` preflight correctly
rejects unavailable noninteractive sudo authorization, before launching the GUI.
No actual battery rate or estimated SoC energy baseline was captured here.

Commands used (exit 3 is expected for non-comparable diagnostics):

```sh
cargo build -p axolotl --release --locked --offline --features smoke -j 2
python3 scripts/benchmark.py run --output /tmp/axolotl-benchmark-archive-v1 \
  --iterations 3 --warmup 1 --runs 1 --allow-busy --label diagnostic-validation
python3 scripts/benchmark.py run --suite energy --sessions small --runs 1 \
  --energy-seconds 3 --allow-busy --label energy-workflow-validation \
  --output /tmp/axolotl-energy-archive-v1
```

Comparing the standard report with itself returns `incomplete` (exit 3), rather
than accepting invalid measurements. Unit checks cover finite-number handling,
percentiles/confidence, incompatible and missing metrics, unavailable telemetry,
power integration, native process isolation, stale stimulus hashes, partial
stdout records, deadlines, phase pairing and collector failure cleanup.

Validation: 46 Python tests passed; 103 Rust workspace release tests passed, with
one existing manual benchmark ignored. Release build, formatting and whitespace
checks passed. Strict all-feature Clippy is blocked by two unchanged
`collapsible_if` warnings in `crates/desktop/src/render.rs` (lines 651 and 693).
With that existing lint class allowed, all-target/all-feature Clippy passes.
Windows execution was unavailable on this Mac.

Use the [benchmark guide](../../benchmarks.md) to capture a clean baseline with
three launches and 20 iterations on an idle, foreground desktop. For discharge,
unplug the Mac and use the longer energy protocol. Keep the benchmark files and
conditions unchanged when measuring a future production-code improvement.
