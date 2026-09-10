//! Phase AZ schema-version marker and v2 task-ledger schema owner.

use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};
use atm_storage::AtmError;
use rusqlite::TransactionBehavior;

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
    FOREIGN KEY (team, task_id, current_attempt)
        REFERENCES task_assignment_attempts(team, task_id, attempt)
        DEFERRABLE INITIALLY DEFERRED,
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
    FOREIGN KEY (team, task_id) REFERENCES tasks_v2(team, task_id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED
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
    FOREIGN KEY (team, task_id) REFERENCES tasks_v2(team, task_id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE IF NOT EXISTS task_operations (
    team TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    request_fingerprint TEXT NOT NULL,
    result_json TEXT NOT NULL,
    PRIMARY KEY (team, operation_id)
);

CREATE INDEX IF NOT EXISTS task_list_order
    ON tasks_v2(team, current_assignee, state, priority, original_assigned_at, task_id);
CREATE TRIGGER IF NOT EXISTS task_v1_insert_bridge
AFTER INSERT ON tasks
BEGIN
    INSERT INTO tasks_v2(
        team, task_id, current_assignee, state, outcome, abort_reason,
        superseded_by, priority, original_assigned_at, current_attempt,
        reminder_ordinal, revision, updated_at
    ) VALUES (
        NEW.team, NEW.task_id, NEW.assignee,
        CASE NEW.state WHEN 'complete' THEN 'closed' ELSE NEW.state END,
        CASE NEW.state WHEN 'complete' THEN 'succeeded' ELSE NULL END,
        NULL, NULL, 'normal', NEW.assigned_at, 1, NEW.reminder_count, 1, NEW.updated_at
    ) ON CONFLICT(team, task_id) DO UPDATE SET
        current_assignee = excluded.current_assignee,
        state = excluded.state,
        outcome = excluded.outcome,
        reminder_ordinal = excluded.reminder_ordinal,
        revision = tasks_v2.revision + 1,
        updated_at = excluded.updated_at;
    INSERT OR IGNORE INTO task_assignment_attempts(
        team, task_id, attempt, assignee, assigner, assignment_message_id, template_sha, assigned_at
    ) VALUES (NEW.team, NEW.task_id, 1, NEW.assignee, NEW.assigner, NEW.assignment_message_id, NULL, NEW.assigned_at);
END;

CREATE TRIGGER IF NOT EXISTS task_v1_update_bridge
AFTER UPDATE OF state, assignment_message_id, updated_at, reminder_count ON tasks
BEGIN
    UPDATE tasks_v2 SET
        current_assignee = NEW.assignee,
        state = CASE NEW.state WHEN 'complete' THEN 'closed' ELSE NEW.state END,
        outcome = CASE NEW.state WHEN 'complete' THEN 'succeeded' ELSE NULL END,
        reminder_ordinal = NEW.reminder_count,
        revision = revision + 1,
        updated_at = NEW.updated_at
     WHERE team = NEW.team AND task_id = NEW.task_id;
END;
"#;

const V2_POST_MIGRATION_DDL: &str = r#"
CREATE UNIQUE INDEX IF NOT EXISTS one_active_task_per_agent
    ON tasks_v2(team, current_assignee) WHERE state = 'active';
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

WITH ranked AS (
    SELECT team, task_id, assignee, state, assigned_at, updated_at,
           ROW_NUMBER() OVER (
               PARTITION BY team, task_id ORDER BY assigned_at ASC, assignee ASC
           ) AS attempt
      FROM tasks
)
INSERT OR IGNORE INTO task_events_v2(
    team, task_id, seq, operation_id, attempt, at, actor, event, outcome, related_task_id, detail
)
SELECT team, task_id, attempt, NULL, attempt, updated_at, assignee,
       'migrated', CASE state WHEN 'complete' THEN 'succeeded' ELSE NULL END, NULL,
       'migrated source row: assignee=' || assignee || ', state=' || state || ', assigned_at=' || assigned_at
  FROM ranked;

INSERT INTO task_events_v2(
    team, task_id, seq, operation_id, attempt, at, actor, event, outcome, related_task_id, detail
)
SELECT candidate.team, candidate.task_id,
       COALESCE((SELECT MAX(event.seq) + 1 FROM task_events_v2 AS event
                 WHERE event.team = candidate.team AND event.task_id = candidate.task_id), 1),
       NULL, candidate.current_attempt, candidate.updated_at, candidate.current_assignee,
       'migrated_active_conflict_demotion', NULL, NULL,
       'demoted because another active task for this member wins by original assignment time and task id'
  FROM tasks_v2 AS candidate
 WHERE candidate.state = 'active'
   AND EXISTS (
       SELECT 1 FROM tasks_v2 AS winner
        WHERE winner.team = candidate.team
          AND winner.current_assignee = candidate.current_assignee
          AND winner.state = 'active'
          AND (
              winner.original_assigned_at < candidate.original_assigned_at
              OR (winner.original_assigned_at = candidate.original_assigned_at
                  AND winner.task_id < candidate.task_id)
          )
   );

UPDATE tasks_v2 AS candidate
   SET state = 'assigned', updated_at = candidate.updated_at
 WHERE candidate.state = 'active'
   AND EXISTS (
       SELECT 1 FROM tasks_v2 AS winner
        WHERE winner.team = candidate.team
          AND winner.current_assignee = candidate.current_assignee
          AND winner.state = 'active'
          AND (
              winner.original_assigned_at < candidate.original_assigned_at
              OR (winner.original_assigned_at = candidate.original_assigned_at
                  AND winner.task_id < candidate.task_id)
          )
   );
"#;

pub(crate) fn ensure_task_v2_schema(
    connection: &mut SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| sqlite_error(target, "failed to start task v2 migration", error))?;
    transaction
        .execute_batch(V2_TASK_DDL)
        .map_err(|error| sqlite_error(target, "failed to initialize task v2 schema", error))?;
    transaction
        .execute_batch(MIGRATE_V1_TASKS)
        .map_err(|error| sqlite_error(target, "failed to migrate v1 task projection", error))?;
    transaction
        .execute_batch(V2_POST_MIGRATION_DDL)
        .map_err(|error| sqlite_error(target, "failed to finalize task v2 indexes", error))?;
    transaction
        .execute(
            "INSERT INTO storage_schema_versions(component, version) VALUES ('task-ledger', ?1)
             ON CONFLICT(component) DO UPDATE SET version = excluded.version",
            [STORAGE_SCHEMA_VERSION],
        )
        .map_err(|error| sqlite_error(target, "failed to persist task schema version", error))?;
    transaction
        .commit()
        .map_err(|error| sqlite_error(target, "failed to commit task v2 migration", error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::STORAGE_SCHEMA_VERSION;
    use crate::shared_db::{SharedDbTarget, ensure_schema};
    use rusqlite::{Connection, params};

    const LEGACY_TASKS_DDL: &str = r#"
CREATE TABLE tasks (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    assignee TEXT NOT NULL,
    assigner TEXT NOT NULL,
    state TEXT NOT NULL,
    assignment_message_id TEXT NOT NULL,
    description TEXT NOT NULL,
    assigned_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_reminded_at TEXT NULL,
    reminder_count INTEGER NOT NULL DEFAULT 0,
    lead_notified_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (team, task_id, assignee)
);
CREATE TABLE task_events (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    assignee TEXT NOT NULL,
    seq INTEGER NOT NULL,
    at TEXT NOT NULL,
    event TEXT NOT NULL,
    from_state TEXT NULL,
    to_state TEXT NULL,
    actor TEXT NOT NULL,
    message_id TEXT NULL,
    outcome TEXT NULL,
    marker TEXT NULL,
    detail TEXT NULL,
    PRIMARY KEY (team, task_id, assignee, seq)
);
"#;

    fn insert_legacy_task(
        connection: &Connection,
        task_id: &str,
        assignee: &str,
        state: &str,
        assigned_at: &str,
        updated_at: &str,
    ) {
        connection
            .execute(
                "INSERT INTO tasks(team, task_id, assignee, assigner, state, assignment_message_id, description, assigned_at, updated_at, reminder_count, lead_notified_count)
                 VALUES ('migration-team', ?1, ?2, 'lead', ?3, '01ARZ3NDEKTSV4RRFFQ69G5FAV', 'legacy body', ?4, ?5, 0, 0)",
                params![task_id, assignee, state, assigned_at, updated_at],
            )
            .expect("insert legacy task");
    }

    #[test]
    fn migration_collapses_multi_assignee_rows_and_demotes_active_conflicts() {
        let mut connection = Connection::open_in_memory().expect("connection");
        connection
            .execute_batch(LEGACY_TASKS_DDL)
            .expect("legacy schema");
        insert_legacy_task(
            &connection,
            "multi-assignee",
            "alpha",
            "assigned",
            "2026-01-01T00:00:00Z",
            "2026-01-02T00:00:00Z",
        );
        insert_legacy_task(
            &connection,
            "multi-assignee",
            "beta",
            "active",
            "2026-01-03T00:00:00Z",
            "2026-01-03T00:00:00Z",
        );
        insert_legacy_task(
            &connection,
            "active-earlier",
            "shared",
            "active",
            "2026-01-01T00:00:00Z",
            "2026-01-04T00:00:00Z",
        );
        insert_legacy_task(
            &connection,
            "active-later",
            "shared",
            "active",
            "2026-01-02T00:00:00Z",
            "2026-01-04T00:00:00Z",
        );
        let target = SharedDbTarget::InMemory {
            uri: "schema-version-migration-test".to_owned(),
        };

        ensure_schema(&mut connection, &target).expect("migrate legacy database");

        let multi: (String, String, u32) = connection
            .query_row(
                "SELECT current_assignee, state, current_attempt FROM tasks_v2 WHERE task_id = 'multi-assignee'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("multi-assignee projection");
        assert_eq!(multi, ("beta".to_owned(), "active".to_owned(), 2));
        let active_rows: Vec<(String, String)> = connection
            .prepare("SELECT task_id, state FROM tasks_v2 WHERE current_assignee = 'shared' ORDER BY task_id")
            .expect("query")
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("map")
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            active_rows,
            vec![
                ("active-earlier".to_owned(), "active".to_owned()),
                ("active-later".to_owned(), "assigned".to_owned()),
            ]
        );
        let migrated_sources: u64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_events_v2 WHERE task_id = 'multi-assignee' AND event = 'migrated'",
                [],
                |row| row.get(0),
            )
            .expect("source events");
        assert_eq!(migrated_sources, 2, "every collapsed source row is audited");
        let demotions: u64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_events_v2 WHERE task_id = 'active-later' AND event = 'migrated_active_conflict_demotion'",
                [],
                |row| row.get(0),
            )
            .expect("demotion event");
        assert_eq!(demotions, 1);
        let version: String = connection
            .query_row(
                "SELECT version FROM storage_schema_versions WHERE component = 'task-ledger'",
                [],
                |row| row.get(0),
            )
            .expect("schema version");
        assert_eq!(version, STORAGE_SCHEMA_VERSION);
    }
}
