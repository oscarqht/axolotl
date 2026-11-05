# Settings

Open **Settings** using the gear at the right of the spaces header,
**Cmd+,** on macOS, **Ctrl+,** on Windows, or **Axolotl → Settings…** in the
macOS app menu. The shortcut is also handled while native pages or floating
panel controls have keyboard focus.

The **Search engine** dropdown offers **Google** (the default), **DuckDuckGo**,
and **Bing**. Changes apply immediately to future address-bar searches; there
is no Save/Cancel step. Existing pages are not reloaded, and explicit URLs,
hostnames, localhost addresses, and `about:blank` retain their normal behavior.

The theme-matched modal focuses the dropdown on opening, keeps Tab/Shift+Tab
inside it, and dismisses with Escape or its labeled close button. Dismissal
restores focus to the invoking header control or page. Background app commands
are blocked while the modal is open.

## Persistence

The choice is written atomically to `settings.json`, separately from
`session.json`, in the app's data directory:

- macOS: `~/Library/Application Support/Axolotl/`
- Windows: `%APPDATA%\Axolotl\`
- `AXOLOTL_DATA_DIR` overrides the directory for isolated development runs.

The persisted `search_engine` identifiers are `google`, `duckduckgo`, and `bing`.
Missing/unreadable files, malformed JSON, missing fields, and unknown or invalid
values fall back to Google. If saving fails, the current choice still applies
in memory and the dialog displays a warning that it may not survive restart.
Smoke runs neither read nor write the user's session or settings.

## Verification

```sh
node --test scripts/test-input-bridge.cjs scripts/test-notifications.cjs
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo run -p axolotl --locked --features smoke -- --smoke-test --smoke-settings
```

The focused native check needs a GUI session but no network. It covers the
shipped dialog's labels/options, initial focus, forward/reverse Tab wrapping,
immediate changes, invalid IPC rejection, action dispatch, close/Escape, focus
restoration, background command blocking, and retained workspace/live page
state. On macOS it also checks live light/dark appearance updates. The same
check runs in the full native smoke suite. File round-trip/fallback and provider
URL encoding are covered by unit tests; shortcut authentication and modifiers
are checked against the shipped input bridge.

These programmatic checks do not synthesize OS keyboard/mouse input or verify
screen-reader announcements. Windows native execution and manual accessibility
checks must be performed on a suitable Windows desktop.
