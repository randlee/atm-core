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

const MIGRATE_V1_TASKS: &str = r#"
WITH ranked AS (
    SELECT team, task_id, assignee, assigner, state, assignment_message_id,
           assigned_at, updated_at, reminder_count,
           ROW_NUMBER() OVER (
               PARTITION BY team, task_id
               ORDER BY CASE state WHEN 'active' THEN 0 WHEN 'assigned' THEN 1 ELSE 2 END,
                        updated_at DESC, assignee ASC
           ) AS winner,
           ROW_NUMBER() OVER (
               PARTITION BY team, task_id
               ORDER BY assigned_at ASC, assignee ASC
           ) AS attempt
      FROM tasks
)
INSERT OR IGNORE INTO tasks_v2(
    team, task_id, current_assignee, state, outcome, abort_reason,
    superseded_by, priority, original_assigned_at, current_attempt,
    reminder_ordinal, revision, updated_at
)
SELECT team, task_id, assignee,
       CASE state WHEN 'complete' THEN 'closed' ELSE state END,
       CASE state WHEN 'complete' THEN 'succeeded' ELSE NULL END,
       NULL, NULL, 'normal',
       (SELECT MIN(other.assigned_at) FROM tasks AS other
         WHERE other.team = ranked.team AND other.task_id = ranked.task_id),
       attempt, reminder_count, 1, updated_at
  FROM ranked WHERE winner = 1;

WITH numbered AS (
    SELECT team, task_id, assignee, assigner, assignment_message_id, assigned_at,
           ROW_NUMBER() OVER (
               PARTITION BY team, task_id ORDER BY assigned_at ASC, assignee ASC
           ) AS attempt
      FROM tasks
)
INSERT OR IGNORE INTO task_assignment_attempts(
    team, task_id, attempt, assignee, assigner, assignment_message_id, template_sha, assigned_at
)
SELECT team, task_id, attempt, assignee, assigner, assignment_message_id, NULL, assigned_at
  FROM numbered;

INSERT OR IGNORE INTO task_events_v2(
    team, task_id, seq, operation_id, attempt, at, actor, event, outcome, related_task_id, detail
)
SELECT team, task_id, 1, NULL, current_attempt, updated_at, current_assignee,
       'migrated', outcome, NULL, 'migrated from retained v1 task projection'
  FROM tasks_v2;
"#;

pub(crate) fn ensure_task_v2_schema(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    connection
        .execute_batch(V2_TASK_DDL)
        .map_err(|error| sqlite_error(target, "failed to initialize task v2 schema", error))?;
    connection
        .execute_batch(MIGRATE_V1_TASKS)
        .map_err(|error| sqlite_error(target, "failed to migrate v1 task projection", error))?;
    connection
        .execute(
            "INSERT INTO storage_schema_versions(component, version) VALUES ('task-ledger', ?1)
             ON CONFLICT(component) DO UPDATE SET version = excluded.version",
            [STORAGE_SCHEMA_VERSION],
        )
        .map_err(|error| sqlite_error(target, "failed to persist task schema version", error))?;
    Ok(())
}
