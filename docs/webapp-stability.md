# macOS webapp reliability investigation

Axolotl had separate application and WebKit lifecycle policies. Keep Alive only
excluded a tab from Axolotl's unload timer; retained WKWebViews still used the
engine's default inactive scheduling. Loaded content views now request Wry's
`BackgroundThrottlingPolicy::Disabled`, which maps to WebKit's
`WKInactiveSchedulingPolicyNone` on macOS 14+. Axolotl can still unload ordinary
idle tabs. This does not stop macOS system sleep or remove normal background
DOM timer clamping.

The sleep activity probe also missed connecting/open WebSocket and EventSource
connections. Those now prevent automatic unloading, including EventSource
reconnection. Closed connections no longer protect a page, and weak references
avoid retaining discarded connections. Trusted edits stay protected for the
current document: a submit event is not evidence that an asynchronous save
succeeded. Custom editors, service-worker-only activity, and page overrides can
still escape this best-effort probe; Keep Alive is the explicit exemption.

Reload now follows native focus, with pinned-page and preview fallbacks. The
pre-change native regression reproduced a pinned-page reload that affected the
underlying pane instead. Cmd+R is reserved by AppKit and dispatched through the
application, independent of the page's input bridge. WebKit itself can still
wait for a blocked JavaScript event loop before completing a normal reload.
Normal reload is therefore not a guarantee of recovery from a permanently hung
engine.

**Recover Tab…** in a tab's context menu offers explicit page recreation when
Refresh cannot recover it. The native warning defaults to Cancel and explains
that unsaved changes and Back/Forward history will be lost. Only accepting
Recreate Tab discards the native page and opens its cached committed URL. It
preserves the tab ID, Keep Alive choice, cookies and persistent site data.
Cancellation and stale/closed targets leave the document untouched. Recovery
is blocked during active downloads or a linked sign-in popup interaction. It
does not query the blocked page for its URL and never runs automatically.

## Live incident snapshot

During the user's recurrence on 2026-11-06, a read-only profile snapshot showed
SeaTalk Keep Alive enabled and no sleeping flag on the Trello tabs, including
the active Trello pane. A five-second sample of the running Axolotl process was
predominantly idle in the AppKit run loop, rather than blocked in an Axolotl
operation. Short samples of the networking process and two content processes
were also idle. They could not be conclusively mapped to the affected Trello
page. Memory-pressure query reported 46% free at that moment.

These observations do not identify the exact Trello renderer/network failure.
The running user app was not restarted, reloaded, or replaced. Native fixtures
run separately from the user's workspace state. No authenticated messages or
boards are modified by the checks.

Content blocking was examined: the generic protection layer does not replace
SeaTalk/Trello fetch; its special fetch behavior is scoped to YouTube. No
captured failing endpoint justified changing the blocking policy. Real third-party
API failures and webapp-specific reconnect behavior remain possible.

## Reproduction and validation

Use this worktree's own target directory. Concurrent worktrees can replace the
shared parent `target/debug/axolotl` executable and invalidate a test run.

```sh
cargo test --workspace --locked
node --test scripts/test-input-bridge.cjs scripts/test-notifications.cjs \
  scripts/test-protection.cjs scripts/test-history-suggestions.cjs \
  scripts/test-sleep-activity.cjs
cargo clippy --workspace --all-targets --features smoke --locked -- -D warnings
cargo build -p axolotl --locked --features smoke

target/debug/axolotl --smoke-test --smoke-reload
target/debug/axolotl --smoke-test --smoke-webapp-recovery
target/debug/axolotl --smoke-test --smoke-webapp-stability \
  --webapp-background-seconds=330 --webapp-live-check \
  --webapp-snapshot=/absolute/path/native-webkit.png
target/debug/axolotl --smoke-test --smoke-sleep
```

The webapp fixture uses local HTTP/SSE, a document identity marker, trusted
native input, and server-side one-second heartbeats. The long hidden interval
contains no JavaScript evaluations of the page. Before the first evaluation,
server-side checks require fresh heartbeats and no gap of three seconds. This
avoids treating buffered events delivered after inspection as continuous
background activity. Auto-sleep deadlines are aged; the background interval
itself uses real wall time. A normal idle page must still sleep after its live
connection closes.

The initial long check assumed one heartbeat each second and rejected its count.
A subsequent timed calibration recorded 32 heartbeats in 60 seconds with a
maximum gap of 2.098 seconds. WebKit coalesces that hidden one-second timer to
roughly two seconds. The final test therefore checks fresh heartbeats and gaps,
allowing the measured background cadence rather than requiring foreground speed.

Pre-change failures: native inactive scheduling was not `None`; native pinned
reload did not replace the pinned document; the injected-script regression
cleared dirty state on a prevented submission.

## Focused results (2026-11-06, macOS 27.0.1)

| Check | Result |
| --- | --- |
| Rust workspace unit tests | 132 passed; one manual persistence benchmark ignored |
| JavaScript regression checks | 29 passed |
| Clippy, including smoke targets | Passed with warnings denied |
| Native reload | Correct split/pin/preview target, one navigation per key, Settings no-op |
| Busy normal reload | Navigation waited 4.185 seconds for a four-second JavaScript stall |
| Explicit native recovery | Final run recreated a page during a ten-second JavaScript stall in 0.778 seconds |
| Recovery cancellation | Original document identity and edited input preserved; Cmd+R cannot reload behind the warning |
| Recovery storage/focus | Cookies/local storage retained; background target preserves foreground focus/document |
| Recovery guards | Stale/closed target and linked popup rejected |
| Native context menu | Recover Tab enabled for a loaded page; grouped menu fits native bounds |
| Existing native sleep suite | Passed manual sleep/Keep Alive, snapshots/previews, auto-sleep exemption, visible splits and space return |
| Hidden native webapp, 330 real seconds | Passed: 168 server heartbeats, longest gap 2.778 seconds, final heartbeat 30 ms old before inspection |
| Retained document after interval | Same document; 2,725 SSE events and 336 timer ticks, then normal auto-sleep after closing its connection |

Recovery uses a real AppKit warning and actual native Cancel/Recreate buttons.
The tests operate only on loopback fixtures. Active-download refusal is checked
against the existing download manager; the new fixture does not simulate a
transfer. A background page's connection and unsaved-input protections are
validated independently of explicit recovery.

Screenshots: [native tab context menu](webapp-stability/tab-menu.png) and
[native 330-second background check](webapp-stability/native-webkit.png).
Event/timer totals include initial visible loading and settling time; the hidden
interval is measured separately with wall time and server receipts.

Opt-in live navigation reached `seatalkweb.com` and `trello.com`. Generic DOM
checks did not establish an authenticated app state. These were reachability
checks, not evidence of successful message delivery or board saves. No login,
message send, or board write was performed.

These checks do not establish Windows runtime behavior, physical system
sleep/wake recovery, or authenticated SeaTalk message delivery/Trello saving.
The exact user incident remains unattributed; the changes address reproduced
Axolotl lifecycle/reload weaknesses and add explicit recovery for a blocked page.
