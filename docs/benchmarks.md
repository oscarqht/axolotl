# Repeatable performance benchmarks

The versioned benchmark runs the actual release app, GPUI shell and embedded
WKWebView on macOS. It uses an isolated temporary profile and local HTTP pages;
it never opens or edits your browsing session. Results are raw JSON plus a
Markdown report. Keep each report directory to compare future implementations.

## Capture a baseline

Requirements: macOS, Python 3.9+, Rust/build dependencies, an unlocked desktop
session, `open` and `codesign`. No Python packages or internet access are needed.
Build with the same release settings for every comparison:

```sh
cargo build -p axolotl --release --locked --features smoke
python3 scripts/benchmark.py doctor
python3 scripts/benchmark.py run --output /tmp/axolotl-baseline --label baseline
```

Default: three independent launches for each small/large session, three discarded
warmup iterations, then 20 measured iterations per workload. Some paired actions
produce two samples per iteration. The `actions` and `web` phases run in each
launch. Use `--suite actions` or `--suite web` to isolate a phase.

Keep other apps idle, the same display/scale/window size, brightness, refresh
rate, power source, power mode and thermal state. Leave the benchmark foreground
and avoid interacting with the desktop. Record your brightness and power-mode
settings alongside the baseline; they are not automatically normalized. App Nap
is suppressed only for latency runs, with a process-scoped activity token.
The energy suite retains natural App Nap. Fixture auto-sleep is disabled in both
suites so test tabs do not disappear at different times.

If automatic foreground activation is denied, bring the benchmark window to the
front before capturing a clean run; check the report for foreground warnings.

The runner rejects a host with a one-minute load average above its logical CPU
count. `--allow-busy` permits diagnostic execution but marks affected runs
non-comparable. Foreground loss and changed window geometry also invalidate
comparability. These checks detect some interference; a passing check does not
prove the machine is otherwise idle. Do not run a compiler alongside a baseline.

Each invocation creates a unique, ad hoc signed `.app` and launches it through
LaunchServices, so its WebKit processes have an independently verified app
responsibility and coalition. Owned processes are cleaned up on failure or
timeout. No accessibility permission is required. Native input is synthesized
inside AppKit; this is not a physical mouse/keyboard test.

## Coverage and measurement boundaries

| Area | Workloads | Measurement |
| --- | --- | --- |
| Spaces | Create, close, warm switch, reorder | Handler/commit through forced draw submission |
| Panels | Create split panel, close, focus, dock to edge, merge whole panel | Handler or native drop-release commit through draw submission |
| Tabs | Create, close, warm switch, reorder, native floating-bar IPC selection | Handler/draw, or control script through selected view visibility |
| Docking and layout | Split a tab left/right/top/bottom, dock into existing panel, start drag preview, resize split | Native release/preview/resize path through draw submission |
| Pinned pages | Open and dismiss | Handler through draw submission |
| Web loading | Simple page; heavy page with cold/warm subresources; reload; history back/forward | Native command to initialized document with load event and expected URL/document identity |
| Web internals | TTFB, DOMContentLoaded, load event, resource transfer bytes | WebKit Navigation/Resource Timing fields |
| Web responsiveness | Trusted native click response, zero-delay timer, 2,000-node style update and forced layout, fixed JS checksum workload | AppKit-to-handler bridge round trip or page-clock duration |
| Web callback cadence | 1.2-second requestAnimationFrame sampling | Callback intervals, not compositor presentation FPS |
| Optional browser APIs | FCP and Event Timing interactions | Available only when WebKit provides entries; Event Timing is not an INP score |
| Resources | App, owned WebKit, combined | Observed CPU time/one-core percent and sampled peak/end RSS by phase |
| Energy | Separate idle and active workloads | Whole-device discharge and optional estimated SoC rail power |

Small: 2 spaces with 4 tabs each, another two-tab panel in the first space,
and 4 global pins. Large: 12 spaces with 40 tabs each, the same extra panel and
pins. Tabs start with `about:blank`; WebViews are materialized on demand, as in
the app. This models workspace scaling, not 480 simultaneously running heavy
websites. Action setup/cleanup is outside the per-action timer. A page sentinel
checks that docking retains the existing native page state.

The heavy local page has 2,000 cells, a stylesheet and SVG image; the simple page
has 40 cells. HTML is `no-store`. Cold subresources have unique cache-busting
URLs, while warm subresource URLs are reused. This is resource-cache cold/warm,
not an OS-cold start, DNS/TLS benchmark or claim about public website speeds.
Browser-reported resource transfer bytes are retained even when zero.

Draw submission does not measure display presentation or mouse-to-photon
latency. Docking timers exclude the physical drag trajectory and snapshot
readiness. The native-control selection workload invokes the normal chip click
script and IPC path, not OS hit testing. The trusted webpage click does exercise
native AppKit event delivery and checks `isTrusted`. It includes the JS result
bridge/polling overhead; Navigation Timing uses the page clock instead. Timers
may legitimately report zero at browser clock resolution. Missing capabilities
remain unavailable, never zero. The rAF workload samples an idle callback loop,
not scrolling/animation smoothness or a full Core Web Vitals audit.

## Battery and power protocol

```sh
python3 scripts/benchmark.py doctor --power
# Unplug the Mac and confirm doctor reports a discharging battery first.
python3 scripts/benchmark.py run --suite energy --sessions small --runs 3 \
  --energy-seconds 1800 --battery --output /tmp/axolotl-battery-baseline
```

Each launch runs idle, then active, for the specified seconds **each**; the
example takes about three hours. Active work updates/layouts the heavy page,
runs the checksum, scrolls and cycles a tab about once per second. Idle leaves
the loaded page alone. Keep these conditions and the workload order identical
between runs. A 3-second energy run without `--battery`/`--power` checks workflow
and resource collection only; it does not measure energy.

Battery samples use the macOS integer percentage gauge once per second. A
battery run requires discharging and at least 300 seconds per phase; 1,800
seconds is recommended. Percentage-point/hour is emitted only after at least
one percentage point of observed loss. No observable drop is insufficient
resolution, not zero energy. AC/source changes invalidate the discharge rate.
The result is whole-device consumption including the screen, other apps and
system services. It cannot attribute battery use or watt-hours to Axolotl.

Optional `--power` collects finite one-second `powermetrics` samples when
existing noninteractive sudo authorization is available. The runner uses
`sudo -n`, never prompts or changes sudo policy. If desired, establish your
normal administrator authorization separately before running. Preflight rejects
unavailable access before launching the GUI; expiry during the run is recorded.

Rail mW and integrated estimated joules cover only successfully sampled
intervals, with coverage recorded. Requested power results with warnings or
less than 80% sampled coverage mark the report incomplete. They are **whole-SoC estimates**, not measured
per-app energy or total device power. CPU/GPU/ANE and the combined rail are
separate series; do not add the combined rail to its components. Collector overhead is disclosed. Battery and power files are scoped to each phase; the
native workload waits for collector start/stop acknowledgements.

## Compare future changes

```sh
# Rebuild the changed app with the same benchmark files/settings, then capture:
python3 scripts/benchmark.py run --output /tmp/axolotl-candidate --label candidate
python3 scripts/benchmark.py compare /tmp/axolotl-baseline/report.json \
  /tmp/axolotl-candidate/report.json --output /tmp/axolotl-comparison.md
```

Reports record binary SHA-256, checkout revision/dirty state, benchmark and
fixture hashes, OS/WebKit version, opaque device identity, hardware, configuration,
power source and window geometry. Git revision describes the runner checkout;
the binary hash identifies the actual artifact. A native embedded stimulus hash
must match the checkout, catching stale benchmark builds. Production code may
change between runs; benchmark stimulus and collection code must match.
Changing the fixture, workload, collector or metric meaning requires a new
baseline (and increment `suite_version` for intentional protocol revisions).

Raw samples, pooled nearest-rank p50/p95/max and per-run medians are retained.
Comparison requires matching hardware/OS, stimulus, configuration, units, scope
and recorded conditions. By default a latency regression must exceed both 10%
and 1 ms at p50 or p95. It requires at least 20 samples and three valid runs;
smaller samples produce informational suspected regressions. This is a guard,
not a statistical significance test. CPU/RSS and energy aggregate metrics have
one observation per phase/run and remain informational under this confidence
rule. Do not treat a short pilot as a regression gate.

Exit codes: run `0` passed, `1` workload failed, `2` invalid input/preflight,
`3` measurements incomplete/non-comparable. Compare `0` no confirmed regression
(including explicitly labeled suspected changes), `2` incompatible,
`3` incomplete, `4` confirmed regression. Inspect status/confidence as well as
exit code. Failed/invalid runs never enter pooled measurements. Required missing
workloads invalidate the run. Optional missing APIs are listed with reasons.
Output directories must be empty; existing archives are never overwritten.
Partial reports and native stdout/stderr remain available after failures.

Validation helpers:

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=scripts python3 -m unittest discover \
  -s scripts/benchmarks -t scripts
```

The Python launcher/resource/energy collectors are currently macOS only. Native
fixtures remain smoke-feature gated and use portable shell/model APIs except
for the explicitly platform-gated trusted input adapter. Windows performance
numbers have not been validated by this suite.

The [initial diagnostic validation](benchmarks/validation-2026-11-06/README.md)
archives successful native workload execution and the reasons those measurements
are excluded from a clean baseline.
