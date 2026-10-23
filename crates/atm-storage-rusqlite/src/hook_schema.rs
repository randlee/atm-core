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

pub(crate) const TOOL_SENDER_REGISTRY_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS tool_sender_registry (
    sender TEXT PRIMARY KEY,
    installer_id TEXT NOT NULL,
    installer_key_id TEXT NOT NULL,
    host_id TEXT NOT NULL,
    runtime_key_id TEXT NOT NULL,
    runtime_public_key TEXT NOT NULL,
    registration_generation INTEGER NOT NULL CHECK(registration_generation >= 1),
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0, 1)),
    installed_at TEXT NOT NULL,
    revoked_at TEXT NULL
);
CREATE TABLE IF NOT EXISTS tool_sender_nonce_ledger (
    nonce TEXT PRIMARY KEY,
    sender TEXT NOT NULL,
    operation_digest TEXT NOT NULL,
    result_digest TEXT NULL,
    consumed_at TEXT NOT NULL
);
"#;

pub(crate) const CRON_DELIVERY_INTENT_DDL: &str = r#"
-- A transition event is retained before its delivery projections.  This makes
-- the generation fence and failure-suppression key durable across reopen, and
-- gives every output/lifecycle intent a stable transition sequence.
CREATE TABLE IF NOT EXISTS cron_transition_event (
    schedule_id INTEGER NOT NULL,
    generation INTEGER NOT NULL CHECK(generation >= 1),
    transition_seq INTEGER NOT NULL CHECK(transition_seq >= 1),
    transition_at TEXT NOT NULL,
    transition_kind TEXT NOT NULL CHECK(transition_kind IN ('success', 'continue', 'failure', 'stopped')),
    failure_kind TEXT NULL,
    failure_code TEXT NULL,
    failure_detail TEXT NULL,
    PRIMARY KEY (schedule_id, generation, transition_seq),
    UNIQUE (schedule_id, generation, transition_at),
    CHECK ((transition_kind = 'failure') = (failure_kind IS NOT NULL)),
    CHECK (failure_kind IS NOT NULL OR (failure_code IS NULL AND failure_detail IS NULL)),
    CHECK (failure_kind IS NULL OR failure_detail IS NOT NULL)
);
CREATE INDEX IF NOT EXISTS cron_transition_event_schedule_generation
    ON cron_transition_event(schedule_id, generation, transition_seq);

CREATE TABLE IF NOT EXISTS cron_delivery_intent (
    schedule_id INTEGER NOT NULL,
    generation INTEGER NOT NULL,
    transition_seq INTEGER NOT NULL CHECK(transition_seq >= 1),
    output_index INTEGER NULL,
    kind TEXT NOT NULL CHECK(kind IN ('lifecycle', 'module_mail')),
    payload TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending', 'delivered', 'suppressed', 'recipient_unavailable', 'unknown')),
    receipt TEXT NULL,
    retry_cause TEXT NULL,
    CHECK ((state = 'unknown') = (retry_cause IS NOT NULL)),
    PRIMARY KEY(schedule_id, generation, transition_seq, output_index)
);
-- SQLite considers NULL values distinct in a composite primary key. Lifecycle
-- intents use no module output index, so normalize that absence to a value
-- outside the u16 module-output domain when enforcing their durable key.
CREATE UNIQUE INDEX IF NOT EXISTS cron_delivery_intent_delivery_key
    ON cron_delivery_intent(
        schedule_id,
        generation,
        transition_seq,
        kind,
        COALESCE(output_index, -1)
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
        .execute_batch(&format!("{HOOK_REGISTRATION_DDL}{CRON_SCHEDULE_DDL}{TOOL_SENDER_REGISTRY_DDL}{CRON_DELIVERY_INTENT_DDL}"))
        .map_err(|error| sqlite_error(target, "failed to initialize hook schema", error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{Connection, params};

    fn target() -> SharedDbTarget {
        SharedDbTarget::InMemory {
            uri: "file:atm-storage-rusqlite-hook-schema-module?mode=memory&cache=shared".to_owned(),
        }
    }

    #[test]
    fn delivery_key_rejects_duplicate_null_lifecycle_output_index() {
        let connection = Connection::open_in_memory().expect("open sqlite database");
        let target = target();
        ensure_schema(&connection, &target).expect("initialise hook schema");

        let lifecycle = || {
            connection.execute(
                "INSERT INTO cron_delivery_intent \
                 (schedule_id, generation, transition_seq, output_index, kind, payload) \
                 VALUES (?1, ?2, ?3, NULL, 'lifecycle', ?4)",
                params![7_i64, 3_i64, 11_i64, "{}"],
            )
        };

        lifecycle().expect("insert first lifecycle intent");
        assert!(
            lifecycle().is_err(),
            "the lifecycle intent key must be unique even though its output index is NULL"
        );
        connection
            .execute(
                "INSERT INTO cron_delivery_intent \
                 (schedule_id, generation, transition_seq, output_index, kind, payload) \
                 VALUES (?1, ?2, ?3, ?4, 'module_mail', ?5)",
                params![7_i64, 3_i64, 11_i64, 0_i64, "{}"],
            )
            .expect("module mail output is distinct from the lifecycle intent");
    }
}
