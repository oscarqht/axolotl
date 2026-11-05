# Native UI responsiveness investigation

Measured on 2026-11-05, macOS 27.0.1 (26A434), MacBookPro18,1, Apple Silicon,
16 GiB RAM, Rust 1.91.1. Baseline: `ea3b725` with only the same smoke timing
instrumentation and fixture added. Both executables use Cargo's optimized release
profile and the `smoke` feature. No user session, cookies or history are loaded.

## Changes

- Native browser, floating-bar, settings, space-menu, suggestions and pin IPC now
  wake the GPUI event task immediately. Wakeups are coalesced; the existing event
  queues and their bounds are retained. Idle maintenance runs every 250 ms rather
  than every 40 ms. Dragging, captures and transient popup setup/disposal use a
  16 ms fallback. URL reconciliation remains time based, approximately every
  500 ms, independently of event volume. Windows reentrant borrow conflicts retry
  after 1 ms instead of dropping the poll loop.
- Newly created native pages no longer destroy every floating bar. Existing bars
  are raised above the pages. Surfaces from inactive panes are recycled across
  spaces, bounded by the currently visible pane count. Pane IDs on IPC reject
  messages left over from a previous assignment. Recycling resets menus,
  expansion, timers, rename state, history and scroll. Same-pane updates retain
  editors, drafts, focus and scroll. Unchanged floating-bar bounds skip native
  geometry calls.
- The pinned-popup shield/menu uses one retained, hidden native surface between
  openings. A generation ID rejects queued actions from an earlier opening.
  Page webviews and browsing state remain retained as before.
- Session polling compares borrowed model state with the last observed model;
  it no longer allocates/serializes the entire session on every tick. It still
  detects direct changes to public model fields. JSON is generated after the
  existing 500 ms debounce, at checkpoints and on shutdown. Failed writes retry.
- Both address suggestion paths debounce for 80 ms and query on a background
  executor using an independent read-only SQLite connection. WAL readers do not
  hold the UI writer's connection. New queries, cancellation and history
  invalidation cancel pending work; a generation and editor target reject stale
  completions. The native suggestion surface is hidden and reused while typing,
  then released when editing is dismissed. Search matching, Unicode handling, deduplication and result limits
  are unchanged. Isolated smoke history uses a temporary SQLite file so it can
  exercise the same independent-reader path; it is deleted after readers close.

## Reproduce

```sh
python3 scripts/build-performance-baseline.py /tmp/axolotl-baseline
python3 scripts/profile-ui.py /tmp/axolotl-baseline/baseline > before.jsonl
cargo build -p axolotl --release --locked --features smoke
python3 scripts/profile-ui.py target/release/axolotl > after.jsonl

target/release/axolotl --smoke-test --smoke-responsiveness
target/release/axolotl --smoke-test --smoke-pins
target/release/axolotl --smoke-test --smoke-history
target/release/axolotl --smoke-test --smoke-sleep
target/release/axolotl --smoke-test --smoke-docking
target/release/axolotl --smoke-test --smoke-space-drag
target/release/axolotl --smoke-test --smoke-popup-transition
target/release/axolotl --smoke-test --smoke-link-popup
```

The timing fixture uses two sessions: 2 spaces × 4 ordinary tabs and 12 spaces ×
40 ordinary tabs, each with 4 pinned pages. Native page views are created lazily;
this is 480 saved tabs, **not 480 loaded websites**. Pages are deterministic local
start pages, with no network/rendering variability from heavy real websites.
Each scenario starts with fresh page/control surfaces. It measures 10 seconds
of idle polling, cold tab materialization, 40 warm tab selections, 30 control IPC
selections, cold/mixed space selection, 40 warm space selections, 20 pin
switches, floating-bar expansion/collapse, and 60 divider resize submissions.
The history case uses 50,000 distinct visit URLs and a query with no matches.

`switch_submit`, `pin_submit` and `resize_submit` measure command execution plus
a forced native GPUI draw submission. `ipc_select` measures invoking the real
floating control click handler through WebKit, waiting for its IPC to select the
model tab, then submitting a draw. These are **not OS mouse-to-photon latency**.
`draw_cpu` is CPU draw-submission time, not frame presentation time. Cold cases
include synchronous native page creation, which can still exceed a frame budget.

The Python runner builds an ad hoc signed, temporary app bundle with a unique
identifier and launches it through LaunchServices. It verifies that the app has
its own macOS responsibility identity before sampling. CPU-time deltas use `ps`;
RSS is sampled at the end of each idle interval. Only WebKit processes in that
app's independent resource coalition are included. Initial CLI-only trials were
excluded from the final results: terminal responsibility inheritance allowed
concurrent worktree tests to contaminate their WebKit attribution. CPU percentages use one
logical core as 100%. RSS is resident memory, not total physical footprint or a
peak; compression, reclamation and process teardown can change it substantially.
The runner marks combined/WebKit resources unavailable if coalition discovery
fails, and rejects a launch that inherits another process's responsibility identity. Run without other Axolotl tests in parallel. Other host apps still run.
The current fixture holds a process-scoped NSProcessInfo activity token in both
versions to suppress App Nap and timer throttling during measurement, while
allowing system idle sleep. This is limited to `--smoke-performance`; the normal
app and its tab-sleep behavior have no activity-token changes.

## Results

### Controlled repeat

An additional identical baseline/improved run held the same benchmark-only activity token in both processes. App Nap suppression reduces one background-scheduling variable; the host is still shared. This is a single controlled repeat, supported by the earlier desktop trials below. Times are milliseconds.

| Session | Interaction | Before p50 / p95 | After p50 / p95 |
| --- | --- | ---: | ---: |
| small | Cold tab draw submission | 73.28 / 86.44 | 42.37 / 46.38 |
| small | Native-bar IPC selection + draw | 36.24 / 41.34 | 2.58 / 4.51 |
| small | Warm space command + draw | 31.64 / 34.91 | 1.59 / 3.16 |
| small | Pinned-page command + draw | 31.57 / 32.75 | 1.60 / 34.84 |
| small | Divider resize + draw | 0.74 / 2.24 | 0.76 / 1.67 |
| large | Cold tab draw submission | 75.24 / 79.98 | 43.69 / 44.58 |
| large | Native-bar IPC selection + draw | 36.87 / 41.99 | 2.89 / 5.20 |
| large | Warm space command + draw | 34.06 / 38.42 | 2.43 / 4.54 |
| large | Pinned-page command + draw | 33.56 / 38.16 | 2.90 / 4.87 |
| large | Divider resize + draw | 1.19 / 3.66 | 1.31 / 1.73 |

| Session | Processes | Before CPU % | After CPU % | Before RSS MiB | After RSS MiB |
| --- | --- | ---: | ---: | ---: | ---: |
| small | app | 4.70 | 1.39 | 307.48 | 202.02 |
| small | webkit | 2.10 | 1.49 | 190.23 | 175.91 |
| small | combined | 6.81 | 2.89 | 497.72 | 377.92 |
| large | app | 6.09 | 1.76 | 325.75 | 140.22 |
| large | webkit | 2.20 | 2.93 | 251.88 | 298.14 |
| large | combined | 8.28 | 4.68 | 577.62 | 438.36 |

Raw controlled data: [baseline](performance/controlled-before.jsonl), [improved](performance/controlled-after.jsonl). These are draw submissions and IPC handling, not display presentation measurements.

### Initial desktop repeats

Two earlier release runs per version, before suppressing App Nap. Each cell lists run 1 / run 2; times are milliseconds. Percentiles are computed within each run, not pooled. The host also ran other worktree tasks, so scheduling outliers remain visible in the raw data.

| Session | Interaction | Before p50 | After p50 | Before p95 | After p95 |
| --- | --- | ---: | ---: | ---: | ---: |
| small | Cold tab draw submission | 75.40 / 70.86 | 54.97 / 39.36 | 483.95 / 72.16 | 55.64 / 55.40 |
| small | Warm tab command + draw | 1.33 / 1.45 | 1.34 / 1.38 | 5.38 / 2.48 | 5.55 / 3.91 |
| small | Native-bar IPC selection + draw | 36.36 / 37.03 | 2.65 / 2.53 | 42.55 / 42.77 | 8.93 / 3.33 |
| small | Warm space command + draw | 32.48 / 32.68 | 1.33 / 1.19 | 37.88 / 37.28 | 3.27 / 2.89 |
| small | Pinned-page command + draw | 32.77 / 35.05 | 1.54 / 2.30 | 38.82 / 44.02 | 36.48 / 43.74 |
| small | Divider resize + draw | 0.71 / 8.97 | 0.80 / 0.65 | 2.63 / 305.26 | 8.10 / 0.94 |
| large | Cold tab draw submission | 75.52 / 165.72 | 152.53 / 39.75 | 93.93 / 166.21 | 301.37 / 40.23 |
| large | Warm tab command + draw | 1.15 / 9.75 | 2.99 / 2.54 | 4.96 / 56.73 | 12.62 / 3.44 |
| large | Native-bar IPC selection + draw | 36.18 / 79.75 | 21.89 / 3.04 | 41.70 / 173.00 | 132.95 / 6.37 |
| large | Warm space command + draw | 33.67 / 33.68 | 2.70 / 1.69 | 37.48 / 39.45 | 1541.67 / 3.53 |
| large | Pinned-page command + draw | 33.22 / 39.64 | 3.51 / 3.05 | 55.24 / 1728.95 | 1463.20 / 3.84 |
| large | Divider resize + draw | 1.29 / 1.31 | 9.33 / 1.13 | 4.52 / 4.02 | 135.29 / 1.52 |

Cold page creation still exceeds a frame budget. The small pin run includes the first popup surface construction; retaining it mainly improves subsequent openings. Already-loaded tab switching was inexpensive before this change and did not consistently improve. Resize results did not consistently improve across these runs. One improved run still had second-scale space/pin stalls. Concurrent host activity and background-window scheduling were not isolated in these trials; they do not establish foreground tail latency. The controlled repeat above suppresses App Nap in both versions.

| Session | Processes | Before CPU % | After CPU % | Before RSS MiB | After RSS MiB |
| --- | --- | ---: | ---: | ---: | ---: |
| small | app | 4.58 / 4.97 | 1.09 / 2.59 | 62.67 / 269.08 | 231.52 / 156.88 |
| small | webkit | 5.57 / 2.58 | 2.49 / 1.79 | 130.03 / 172.38 | 219.20 / 198.97 |
| small | combined | 10.15 / 7.56 | 3.58 / 4.38 | 192.70 / 441.45 | 450.72 / 355.84 |
| large | app | 5.18 / 2.58 | 0.90 / 1.79 | 84.73 / 91.23 | 66.22 / 162.16 |
| large | webkit | 5.58 / 4.36 | 2.69 / 1.69 | 215.44 / 248.00 | 172.31 / 222.62 |
| large | combined | 10.76 / 6.93 | 3.59 / 3.49 | 300.17 / 339.23 | 238.53 / 384.78 |

CPU uses one logical core as 100%, over each approximately 10-second idle interval. RSS varied between runs; no consistent memory reduction is claimed. Native chrome caching trades a bounded retained surface for avoiding repeated WebKit construction. Engine process caches also vary.

Large idle persistence comparison p50: 0.25 / 0.32 ms before, 0.02 / 0.02 ms after.
50k-visit UI query dispatch p95: 26.07 / 27.95 ms before, 0.18 / 0.01 ms after. This measures UI work, excluding the new 80 ms debounce and background SQL execution. The full SQL scan is still linear; it is moved off the UI thread.

Raw results: [before 1](performance/before-1.jsonl), [before 2](performance/before-2.jsonl), [after 1](performance/after-1.jsonl), [after 2](performance/after-2.jsonl). Each records the executable SHA-256 and the verified independent app coalition.

## Validation and limits

All 75 release Rust tests pass (36 desktop, 39 workspace). Native history, pinning,
sleep, 16 pointer-capture docking cases, space dragging, popup transitions, link
popups, indexed shortcuts, notifications and settings checks passed. The new
history regression confirms suggestion-view identity survives input changes.

The focused responsiveness regression checks retained native control identity,
recycling state, rejection of stale pane IPC, retained address drafts and focus,
and cancellation/invalidation of asynchronous suggestions. Pin regression also
checks retained popup control identity and rejection of an old generation's
queued dismissal. Native sleep and docking checks retain page DOM/history and
exercise pointer capture, cancellation and release through AppKit/WebKit.

The responsiveness check passed initially. Later foreground-dependent draft/focus
and space-editor runs lost window activation; the space-editor dismissal also
reproduced in the baseline. Other worktrees were launching native browser tests
on the same desktop. Exclusive-foreground validation of these flows remains
outstanding. The notification check skipped its foreground-suppression branch
when the window was inactive.

Native tests use programmatic GPUI/AppKit input and actual WKWebViews; they do not
substitute for physical trackpad/OS mouse testing. Popup transition checks inspect
actual Core Animation presentation opacity, interrupted fades, promotion and
Reduce Motion. No reliable WebKit `requestAnimationFrame` samples were delivered
in this GUI session. FPS, dropped display frames and visually perceived smoothness
therefore remain unverified. Windows runtime validation is unavailable on this
macOS host; shared code and the existing deferred WebView2 creation path are kept.
The fixtures use local start pages and disable real session persistence. Session
write correctness is unit-tested, but disk-save latency and heavy live websites
were not profiled.

Known baseline checks, reproduced before these changes:

- Floating-bar sizing: `Single-chip bar did not fit contents`, width 286 px,
  gap 40 px. The shield's existing layout gap is outside this performance pass.
- `test-input-bridge.cjs`: trusted Escape assertion expects one IPC message and
  observes two. That bridge and its test are unchanged.
- Strict Clippy: two `collapsible_if` errors in unchanged pinned drag handlers in
  `render.rs`. Clippy with just that existing lint allowed passes.
