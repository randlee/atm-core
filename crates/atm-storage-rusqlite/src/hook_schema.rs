//! SQLite-owned schema for the hook registration and cron schedule stores
//! (ADR-061 storage MINOR, ADR-069 D1 and D4).
//!
//! Both tables are additive: no foreign key, created idempotently, and
//! ignored by a pre-bk binary. The lifecycle CHECKs keep an active row
//! without a stop or disable timestamp, and an inactive row without one,
//! unstorable.
//!
//! Kept beside the stores so `shared_db.rs` stays under the file-length
//! threshold (RULE-003), mirroring `graft_receiver_endpoint_schema`.

use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};
use atm_storage::error::AtmError;

const HOOK_REGISTRATION_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS hook_registration (
    team TEXT NOT NULL,
    phase TEXT NOT NULL,
    hook TEXT NOT NULL,
    module_path TEXT NOT NULL,
    cwd TEXT NOT NULL,
    timeout_ms INTEGER NOT NULL CHECK(timeout_ms > 0),
    custom TEXT NOT NULL DEFAULT '{}',
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0, 1)),
    registered_by TEXT NOT NULL,
    registered_at TEXT NOT NULL,
    disabled_at TEXT NULL,
    CHECK ((active = 1) = (disabled_at IS NULL)),
    PRIMARY KEY (team, phase, hook)
);
"#;

const CRON_SCHEDULE_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS cron_schedule (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    generation INTEGER NOT NULL DEFAULT 1 CHECK(generation >= 1),
    team TEXT NOT NULL,
    phase TEXT NOT NULL,
    name TEXT NOT NULL,
    cron_expr TEXT NOT NULL,
    module_path TEXT NOT NULL,
    cwd TEXT NOT NULL,
    args TEXT NOT NULL,
    timeout_ms INTEGER NOT NULL CHECK(timeout_ms > 0),
    jitter_s INTEGER NOT NULL DEFAULT 0 CHECK(jitter_s >= 0),
    custom TEXT NOT NULL DEFAULT '{}',
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0, 1)),
    until_at TEXT NULL,
    stop_hook INTEGER NOT NULL DEFAULT 0 CHECK(stop_hook IN (0, 1)),
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_run_at TEXT NULL,
    last_outcome TEXT NULL,
    ended_at TEXT NULL,
    CHECK ((until_at IS NOT NULL) + (stop_hook = 1) = 1),
    CHECK ((active = 1) = (ended_at IS NULL)),
    UNIQUE (team, phase, name)
);
"#;

pub(crate) fn ensure_schema(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    connection
        .execute_batch(&format!("{HOOK_REGISTRATION_DDL}{CRON_SCHEDULE_DDL}"))
        .map_err(|error| sqlite_error(target, "failed to initialize hook schema", error))
}
