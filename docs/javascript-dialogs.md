# JavaScript dialogs

Website tabs, temporary previews, pinned pages, window.open popups and their
frames use native JavaScript dialogs. Alert has OK; confirm returns true for OK
and false for Cancel; prompt starts with the supplied default text and returns
the submitted string (including an empty string), or null for Cancel.

On macOS the existing WKUIDelegate proxy owns asynchronous NSAlert sheets and
forwards popup creation and all other optional methods to Wry. Sheets identify
the initiating frame's host. Prompt uses an editable native text field. Return
accepts and Escape cancels confirm/prompt; AppKit handles focus, selection and
clipboard editing. Removing the owning webview dismisses its pending sheets and
resolves each completion once using the cancel result. A detached view resolves
immediately without presenting UI. AppKit queues sheets sharing a window.

On Windows Axolotl explicitly enables WebView2's default script dialogs on each
native controller, before applying queued navigation. WebView2 owns presentation,
frame dispatch, return values, keyboard handling and controller-close cleanup.
No JavaScript API replacement or IPC roundtrip is involved on either platform.

## Native regression

```sh
cargo build -p axolotl --locked --features smoke
AXOLOTL_DIALOG_SNAPSHOT=/tmp/axolotl-js-prompt.png \
  target/debug/axolotl --smoke-test --smoke-script-dialogs
```

The offline macOS regression invokes the real JavaScript APIs, checks their
results, sends Return/Escape through the AppKit event queue, exercises a frame
and a related popup, closes a popup while a prompt is pending, then opens another
alert to check responsiveness. The snapshot option captures the native prompt
window. An unlocked graphical session is required for foreground keyboard and
OS screenshot verification.

For a locked session, `--smoke-dialog-direct` invokes the actual NSAlert buttons
instead of queueing keyboard events. This verifies engine results and native button actions; it does not verify
foreground keyboard behavior. Omit the snapshot option in this mode.

For manual checks on either platform, serve `scripts/fixtures` with
`python3 -m http.server 8765 --directory scripts/fixtures`, then open
`http://127.0.0.1:8765/javascript-dialogs.html` in Axolotl. Check OK and Cancel,
prompt defaults, an empty submission, Unicode, Return/Escape, Tab focus,
selection, and copy/paste. Repeat in the frame and popup, then close the owning
tab/window with a dialog pending and verify subsequent dialogs still work.

## Verification on this change

- macOS native button regression passed, including real JavaScript return values,
  frame/popup dispatch, pending-prompt owner closure and a subsequent alert.
- Workspace tests: 134 passed, 1 ignored. Strict Clippy and formatting passed.
- The exact Windows Wry/WebView2 settings call passed an isolated
  `x86_64-pc-windows-msvc` compile check. A full app cross-check on the Mac stopped
  in `ring` because Windows C headers (`assert.h`) are unavailable. No Windows
  runtime test was performed.
- The Mac session was locked during validation. Foreground Return/Escape,
  interactive text editing and on-screen screenshot verification remain pending.
- The broader `--smoke-oauth` regression failed at `child.closed`; the same failure
  reproduced from an unmodified `main` archive in this locked session. The new
  focused popup-dialog and pending-dialog close checks passed.
