//! Phase AZ schema-version marker and v2 task-ledger schema owner.

use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};
use atm_storage::AtmError;

/// ADR-061-approved major storage schema. The v1 projection remains present
/// for all 1.6.x readers and writers.
pub(crate) const STORAGE_SCHEMA_VERSION: &str = "2.0.0";

const V2_TASK_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS storage_schema_versions (
    component TEXT NOT NULL PRIMARY KEY,
    version TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tasks_v2 (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    current_assignee TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('assigned', 'active', 'blocked', 'closed')),
    outcome TEXT NULL CHECK(outcome IN ('succeeded', 'failed', 'aborted')),
    abort_reason TEXT NULL,
    superseded_by TEXT NULL,
    priority TEXT NOT NULL CHECK(priority IN ('high', 'normal', 'low')),
    original_assigned_at TEXT NOT NULL,
    current_attempt INTEGER NOT NULL CHECK(current_attempt >= 1),
    reminder_ordinal INTEGER NOT NULL DEFAULT 0 CHECK(reminder_ordinal >= 0),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(revision >= 0),
    updated_at TEXT NOT NULL,
    PRIMARY KEY (team, task_id),
    CHECK((state = 'closed') = (outcome IS NOT NULL)),
    CHECK((outcome = 'aborted') OR abort_reason IS NULL),
    CHECK((superseded_by IS NULL) OR (outcome = 'aborted'))
);

CREATE TABLE IF NOT EXISTS task_assignment_attempts (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK(attempt >= 1),
    assignee TEXT NOT NULL,
    assigner TEXT NOT NULL,
    assignment_message_id TEXT NOT NULL,
    template_sha TEXT NULL,
    assigned_at TEXT NOT NULL,
    PRIMARY KEY (team, task_id, attempt),
    FOREIGN KEY (team, task_id) REFERENCES tasks_v2(team, task_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS task_events_v2 (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    seq INTEGER NOT NULL CHECK(seq >= 1),
    operation_id TEXT NULL,
    attempt INTEGER NULL,
    at TEXT NOT NULL,
    actor TEXT NOT NULL,
    event TEXT NOT NULL,
    outcome TEXT NULL,
    related_task_id TEXT NULL,
    detail TEXT NULL,
    PRIMARY KEY (team, task_id, seq),
    FOREIGN KEY (team, task_id) REFERENCES tasks_v2(team, task_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS task_operations (
    team TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    request_fingerprint TEXT NOT NULL,
    result_json TEXT NOT NULL,
    PRIMARY KEY (team, operation_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS one_active_task_per_agent
    ON tasks_v2(team, current_assignee) WHERE state = 'active';
CREATE INDEX IF NOT EXISTS task_list_order
    ON tasks_v2(team, current_assignee, state, priority, original_assigned_at, task_id);
CREATE INDEX IF NOT EXISTS idx_mail_messages_task_id
    ON mail_messages(team, json_extract(envelope_json, '$.taskId'));
"#;

pub(crate) fn ensure_task_v2_schema(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    connection
        .execute_batch(V2_TASK_DDL)
        .map_err(|error| sqlite_error(target, "failed to initialize task v2 schema", error))?;
    connection
        .execute(
            "INSERT INTO storage_schema_versions(component, version) VALUES ('task-ledger', ?1)
             ON CONFLICT(component) DO UPDATE SET version = excluded.version",
            [STORAGE_SCHEMA_VERSION],
        )
        .map_err(|error| sqlite_error(target, "failed to persist task schema version", error))?;
    Ok(())
}
