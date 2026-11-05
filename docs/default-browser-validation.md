# Default-browser integration validation

Validated on macOS 27.0.1, Apple Silicon, on 2026-11-05.

## Passed

- macOS workspace check with all targets and features.
- 74 Rust unit tests, including strict external URL filtering, rejected IPC
  authentication, acknowledged duplicate delivery suppression, secondary
  handoff, and replacement of an abandoned instance endpoint.
- Native Settings smoke: initial OS status delivery, Make default error handling
  for a bare executable, modal focus order including the new button, existing
  preferences, retained tabs/live pages, theme updates, dismissal and reopening.
- Native temporary-bundle regression: Make default displays the move-to-Applications
  error and remains retryable, without changing the OS association.
- Signed packaged-app URL checks with a temporary data directory: two spaces,
  split panels, cold command-line URLs, running-process handoff, multiple URLs,
  separate repeated requests, unsafe scheme rejection, minimized-window
  activation, and session preservation on restart.
- Real cold/running macOS Apple events, including `open -n` forced-secondary
  launches forwarding their URL events to the existing owner.
- Packaging: valid HTTP/HTTPS plist registration, ad-hoc signing verification,
  and LaunchServices registration.
- Interactive native Settings inspection and Make default action. The OS
  accepted both associations; after moving the test bundle out of the temporary
  directory, effective status reported HTTP and HTTPS as default.
- Windows-target type checks of the new default-browser and single-instance
  modules in an isolated Rust harness, including registry/COM/shell API types,
  file-lock APIs and foreground permission transfer.
- Scoped rustfmt, shell syntax, Python syntax and `git diff --check`.
- Clippy with the existing `collapsible_if` warnings allowed.

## Platform behavior found during testing

LaunchServices records a bundle in the temporary directory but excludes it from
usable browser handlers. Its default-application API can still report success
and persist that bundle's identity. Make default now requires the **running copy**
to appear in LaunchServices' usable handlers, and tells the user to move it to
Applications otherwise. Status always reflects effective OS resolution, never
just a successful request callback.

A forced second macOS process must run its startup Apple-event loop before it
exits; URLs are not present in argv in that case. The forwarding helper creates
no browser window and never restores or writes another session.

## Remaining native coverage

Windows execution was unavailable. Registry writes, actual Default apps UI,
Windows cold/running protocol launches, and Windows foreground behavior still
need a Windows desktop with WebView2. The full cross-target workspace check was
blocked in `ring` by missing Windows SDK/C headers (`assert.h`), before checking
the desktop crate. The isolated module checks are not a full Windows app build.

Cancellation and partial-association changes through macOS System Settings,
and Windows user-consent behavior, remain manual platform checks. These checks
do not establish screen-reader announcements or every OS release's dialog UI.

## Existing baseline checks

Strict Clippy fails on two pre-existing nested conditions in `render.rs`
(lines 643 and 685, pinned-tab dragging). No new warnings remain with
`-A clippy::collapsible_if`.

Whole-workspace `cargo fmt --all -- --check` reports a pre-existing extra blank
line in `browser.rs` near line 264. That unrelated source file is unchanged;
the changed Rust files pass scoped rustfmt checks.
