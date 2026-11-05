# Application persistence

Axolotl keeps its working model in memory and uses one `app.sqlite3` for durable
workspace snapshots (all spaces, panes, layouts, tab order, URLs, titles and pins),
preferences, window geometry, sleep/Keep Alive metadata, protection preferences,
and browsing history. Workspace snapshots retain their existing validated,
versioned JSON format in `app_state`; visits use the indexed `visits` table.
`PRAGMA user_version` versions the database schema. Unknown future versions are
rejected without modifying them.

Screenshots (`sleep/*.png`), downloaded filter lists (`filter-lists.json`), generated
caches, engine-managed cookies and site storage, and OS credentials stay outside
this database. Live native views and transient UI interaction state are not saved.

## Import and authority

On the first successful open, one transaction imports `session.json`,
`settings.json`, `window-state.json`, `sleep/state.json`, `protection.json`, and
`history.sqlite3`, then records `migration_metadata.legacy_import=complete`.
History is read through SQLite, including committed WAL contents; IDs and visit
order are preserved. An interrupted transaction rolls back completely and can be
retried. Invalid/unreadable imports report an error and disable persistence for
that run, preserving the source data. Existing authoritative state rows take
priority if recovering an incomplete import.

Original legacy files stay at their original paths as migration backups. They
are never synchronized with the database. Subsequent opens, history clears, and
restarts do not reimport them. Once migrated, the database is authoritative.
Back up `app.sqlite3` with SQLite's backup API or after the application exits;
copying only the main file while WAL writes are active can omit recent changes.

## Writes, shutdown, and failures

Mutable workspace access advances an explicit revision, including edits through
public model fields. Idle 40 ms UI polls compare only that revision: they neither
clone nor serialize an unchanged workspace. A changed workspace is cloned for the
writer; snapshot serialization and database writes happen on one background
thread. Small preference records are serialized only when changed.

The writer coalesces state by key and starts a commit at most 500 ms after the first
observed change, even during continuous edits. The workspace mutation timestamp
includes time spent waiting for the next UI poll. History operations retain event order in
the same transactional batch. Sleep/Keep Alive changes enqueue their metadata and
matching workspace IDs together. History searches and confirmed clears wait for
preceding writes so they observe the user's latest actions. Normal app exit
queues the final workspace, sleep metadata, and window state, then waits for a
flush of all pending changes.

An OS-managed `app.lock` permits one writer process per profile; a competing
instance reports an error and disables persistence. The lock releases on exit or
crash.

A failed transaction leaves the previous committed data intact and retains the
pending batch for retry. Errors appear in the browser status and stderr. When the
profile directory is writable, `app-recovery.json` also retains failed changes for
the next launch. Successful writes remove it; a receipt committed in the database
prevents replaying an already committed recovery batch after an interrupted
cleanup. If both the database and recovery file are unwritable, the pending data
remains in memory and an error reports that it could not be made durable.

Abrupt termination may lose the latest routine batch within the 500 ms window.
Legacy backups remain available independently. Corrupt stored JSON or unsupported
workspace/schema versions disable writes to avoid replacing recoverable data with
a default workspace.

## Checks

```sh
cargo test --workspace --locked
cargo build -p axolotl --locked --features smoke
python3 scripts/test-space-session.py target/debug/axolotl
python3 scripts/test-sleep-session.py target/debug/axolotl
python3 scripts/test-window-state.py target/debug/axolotl
python3 scripts/test-sqlite-session.py target/debug/axolotl
python3 scripts/test-persistence-exit.py target/debug/axolotl
```

Native restart scripts require a GUI macOS desktop and create temporary profiles
using `AXOLOTL_DATA_DIR`; they do not touch the user's profile. Unit tests cover
migration, transaction rollback/retry, history behavior, batching, recovery replay,
and idle revision checks. Windows uses the same database/worker, but native
Windows restore/exit flows require validation on Windows.

Validation results and measured idle overhead are in [persistence validation](persistence-validation.md).
