# SQLite persistence validation

Validated on macOS ARM64 in `bro-unify-sqlite-persistence`, 2026-11-05. All native
runs used temporary profiles or `--smoke-test`; the user's profile was untouched.

## Passed

- `cargo test --workspace --locked --all-features`: 91 tests passed (52 desktop,
  39 workspace-core); one manual benchmark intentionally ignored in routine runs.
- `cargo build -p axolotl --locked --features smoke`.
- `cargo fmt --all --check` and `git diff --check`.
- `scripts/test-sqlite-session.py`: fresh native processes import every legacy
  scope; restore split layout, spaces, dormant tab order/URL/title overrides,
  pinned sizes/icons, settings, window geometry, protection policy, and sleeping
  metadata. The probe inspects actual in-memory app state as well as SQLite.
  Sleeping views remain unloaded, PNG caches load, waking clears the cache while
  preserving Keep Alive, history IDs remain unchanged, and a history clear does
  not trigger reimport. Original JSON/history backup bytes remain unchanged.
- `scripts/test-space-session.py`: complete native title/icon editing, restart,
  and older-session import sequence passed.
- `scripts/test-persistence-exit.py`: edits made after the last checkpoint and
  followed immediately by window close or app quit restore in a fresh process.
- `scripts/test-window-state.py`: complete native relaunch sequence passed for
  move/resize, zoom/unzoom, fullscreen, zoom/fullscreen combinations, normal
  bounds, off-screen/oversized geometry, invalid state, and header spacing.
- `--smoke-test --smoke-history`: native HTTP fixtures verify successful visit
  recording, redirect/subframe/internal/failure exclusions, repeat visits,
  delayed titles, both suggestion surfaces, retention settings, and confirmed
  clear preserving workspace, cookies, and Back/Forward state.

Database/worker tests additionally exercise legacy WAL import, interrupted import,
rollback/retry, future schema rejection, corrupt stored data, fixed deadlines under
continuous edits, delayed UI queueing, atomic workspace/sleep/history writes,
profile writer exclusion, retained failed batches, and receipt-protected recovery
replay without duplicated visits.

## Measured overhead

The manual `measure_idle_snapshot_overhead` test compares the previous operation
(serializing the workspace on every idle poll) with the revision check. In a
251-tab workspace over 10,000 iterations, a debug build measured:

| Operation | Total time |
| --- | ---: |
| Repeated workspace serialization | 4.483 s |
| Unchanged revision checks | 116.208 microseconds |

This isolates idle persistence overhead, not overall application startup, memory,
or rendering performance. The unchanged path captures and serializes no snapshots.
Run it explicitly with:

```sh
cargo test -p axolotl --locked --all-features measure_idle_snapshot_overhead -- --ignored --nocapture
```

## Verification limits

- The native screenshot-creation sleep probe fails with `Native snapshot failed`
  in this environment. The exact unchanged `HEAD` sources, exported and built
  separately, reproduce the same failure. Native import/restart/wake using a
  known PNG cache fixture passes; creating a fresh WebKit snapshot remains an
  existing verification blocker.
- Native UI automation is intermittent: the space-menu probe passed an earlier
  complete run but also lost its menu during reruns, and the history smoke had
  a settings callback timeout before a successful complete run. The dedicated
  SQLite state probe avoids dependence on editor focus for fixture creation.
- Strict Clippy is blocked by two pre-existing `collapsible_if` warnings in
  `render.rs` at lines 643 and 685. Both constructs are unchanged from `HEAD`.
  All other checks pass with only that lint allowed:
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -A clippy::collapsible_if`.
- Native Windows startup, restart, shutdown, and file locking were not run on this
  Mac. The common database/worker code is tested here; Windows runtime validation
  remains necessary.
