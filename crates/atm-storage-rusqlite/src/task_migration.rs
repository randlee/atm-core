//! One-way migration from the assignee-keyed task ledger to logical task identity.

use std::path::PathBuf;

use atm_storage::AtmError;
use chrono::Utc;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};

use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};
use crate::task_store::{TASK_INDEX_DDL, TASK_TABLES_DDL};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TaskMigrationReport {
    pub rows_before: u64,
    pub tasks_after: u64,
    pub merged_duplicate_rows: u64,
    pub demoted_active_conflicts: u64,
    pub backup_path: Option<PathBuf>,
}

pub(crate) fn migrate_task_identity(
    connection: &mut SqliteConnection,
    target: &SharedDbTarget,
) -> Result<TaskMigrationReport, AtmError> {
    let assignee_pk: Option<u32> = connection
        .query_row(
            "SELECT pk FROM pragma_table_info('tasks') WHERE name = 'assignee'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to inspect task identity schema", error))?;
    if assignee_pk != Some(3) {
        return Ok(TaskMigrationReport::default());
    }

    let rows_before = count(connection, "tasks", target)?;
    let backup_path = if rows_before == 0 {
        None
    } else {
        let path = backup_path(target);
        let escaped = path.to_string_lossy().replace('\'', "''");
        connection
            .execute_batch(&format!("VACUUM INTO '{escaped}'"))
            .map_err(|error| {
                sqlite_error(target, "failed to create pre-BA.2 task backup", error)
            })?;
        tracing::info!(path = %path.display(), "created pre-BA.2 task-ledger backup; restoring it loses writes after this snapshot");
        Some(path)
    };

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| sqlite_error(target, "failed to begin BA.2 task migration", error))?;
    let result = migrate_transaction(&transaction, target, rows_before, backup_path.clone());
    match result {
        Ok(report) => {
            transaction.commit().map_err(|error| {
                sqlite_error(target, "failed to commit BA.2 task migration", error)
            })?;
            Ok(report)
        }
        Err(error) => {
            drop(transaction);
            let recovery = backup_path.as_ref().map_or_else(
                || "the legacy tables were left unchanged".to_owned(),
                |path| format!("restore {} after stopping the daemon; writes after the snapshot will be lost", path.display()),
            );
            Err(AtmError::mailbox_write(format!(
                "BA.2 task migration failed: {}; {recovery}",
                error.message()
            )))
        }
    }
}

fn migrate_transaction(
    transaction: &Transaction<'_>,
    target: &SharedDbTarget,
    rows_before: u64,
    backup_path: Option<PathBuf>,
) -> Result<TaskMigrationReport, AtmError> {
    transaction
        .execute_batch(
            "ALTER TABLE tasks RENAME TO tasks_legacy;
             ALTER TABLE task_events RENAME TO task_events_legacy;",
        )
        .map_err(|error| sqlite_error(target, "failed to preserve legacy task tables", error))?;
    transaction
        .execute_batch(TASK_TABLES_DDL)
        .map_err(|error| sqlite_error(target, "failed to create BA.2 task tables", error))?;

    transaction.execute_batch(
        r#"
        WITH ranked AS (
            SELECT *, ROW_NUMBER() OVER (
                PARTITION BY team, task_id
                ORDER BY CASE state WHEN 'complete' THEN 0 WHEN 'active' THEN 1 ELSE 2 END,
                         updated_at DESC, assignee ASC) AS winner
            FROM tasks_legacy
        ), winners AS (SELECT * FROM ranked WHERE winner = 1)
        INSERT INTO tasks(team, task_id, assignee, assigner, state, close_outcome, position,
                          assignment_message_id, description, assigned_at, updated_at,
                          last_reminded_at, reminder_count, lead_notified_count)
        SELECT team, task_id, assignee, assigner, state,
               CASE state WHEN 'complete' THEN 'completed' END,
               CASE WHEN state = 'complete' THEN NULL ELSE ROW_NUMBER() OVER (
                   PARTITION BY team, assignee, (state = 'complete')
                   ORDER BY CASE state WHEN 'active' THEN 0 ELSE 1 END,
                            assigned_at ASC, task_id ASC) END,
               assignment_message_id, description, assigned_at, updated_at,
               last_reminded_at, reminder_count, lead_notified_count
        FROM winners;

        INSERT INTO task_events(team, task_id, assignee, seq, at, event, from_state, to_state,
                                close_outcome, actor, message_id, outcome, marker, detail)
        SELECT team, task_id, assignee,
               ROW_NUMBER() OVER (PARTITION BY team, task_id ORDER BY at, assignee, seq),
               at, event, from_state, to_state,
               CASE WHEN to_state = 'complete' OR from_state = 'complete' THEN 'completed' END,
               actor, message_id, outcome, marker, detail
        FROM task_events_legacy;

        WITH ranked AS (
            SELECT *, ROW_NUMBER() OVER (
                PARTITION BY team, task_id
                ORDER BY CASE state WHEN 'complete' THEN 0 WHEN 'active' THEN 1 ELSE 2 END,
                         updated_at DESC, assignee ASC) AS winner
            FROM tasks_legacy
        ), losers AS (
            SELECT *, ROW_NUMBER() OVER (PARTITION BY team, task_id ORDER BY updated_at, assignee) AS loser_no
            FROM ranked WHERE winner <> 1
        )
        INSERT INTO task_events(team, task_id, assignee, seq, at, event, from_state, to_state,
                                close_outcome, actor, detail)
        SELECT loser.team, loser.task_id, loser.assignee,
               COALESCE((SELECT MAX(event.seq) FROM task_events event
                         WHERE event.team = loser.team AND event.task_id = loser.task_id), 0) + loser.loser_no,
               loser.updated_at, 'migrated', winner.state, winner.state,
               CASE winner.state WHEN 'complete' THEN 'completed' END,
               'atm-daemon',
               'migrated source row: assignee=' || loser.assignee || ', state=' || loser.state || ', assigned_at=' || loser.assigned_at
        FROM losers loser
        JOIN ranked winner ON winner.team = loser.team AND winner.task_id = loser.task_id AND winner.winner = 1;

        INSERT INTO task_events(team, task_id, assignee, seq, at, event, from_state, to_state,
                                actor, detail)
        SELECT task.team, task.task_id, task.assignee,
               COALESCE((SELECT MAX(seq) FROM task_events event
                         WHERE event.team = task.team AND event.task_id = task.task_id), 0) + 1,
               task.updated_at, 'started', 'assigned', 'active', 'atm-daemon',
               'synthesized by BA.2 migration'
        FROM tasks task
        WHERE task.state = 'active'
          AND NOT EXISTS (SELECT 1 FROM task_events event
                          WHERE event.team = task.team AND event.task_id = task.task_id
                            AND event.event = 'started');

        INSERT INTO task_events(team, task_id, assignee, seq, at, event, from_state, to_state,
                                close_outcome, actor, detail)
        SELECT task.team, task.task_id, task.assignee,
               COALESCE((SELECT MAX(seq) FROM task_events event
                         WHERE event.team = task.team AND event.task_id = task.task_id), 0) + 1,
               task.updated_at, 'completed', 'assigned', 'complete', 'completed', 'atm-daemon',
               'synthesized by BA.2 migration'
        FROM tasks task
        WHERE task.state = 'complete'
          AND NOT EXISTS (SELECT 1 FROM task_events event
                          WHERE event.team = task.team AND event.task_id = task.task_id
                            AND event.event IN ('completed', 'refused', 'cancelled'));

        CREATE TEMP TABLE active_demotions AS
        SELECT team, task_id, assignee, assigned_at,
               ROW_NUMBER() OVER (PARTITION BY team, assignee ORDER BY assigned_at, task_id) AS active_rank
        FROM tasks WHERE state = 'active';

        UPDATE tasks SET state = 'assigned'
        WHERE (team, task_id) IN (SELECT team, task_id FROM active_demotions WHERE active_rank > 1);

        INSERT INTO task_events(team, task_id, assignee, seq, at, event, from_state, to_state,
                                actor, detail)
        SELECT demotion.team, demotion.task_id, demotion.assignee,
               COALESCE((SELECT MAX(seq) FROM task_events event
                         WHERE event.team = demotion.team AND event.task_id = demotion.task_id), 0) + 1,
               task.updated_at, 'migrated', 'active', 'assigned', 'atm-daemon',
               'demoted because another active task for this member wins by original assignment time and task id'
        FROM active_demotions demotion
        JOIN tasks task ON task.team = demotion.team AND task.task_id = demotion.task_id
        WHERE demotion.active_rank > 1;

        UPDATE tasks AS current SET position = (
            SELECT COUNT(*) FROM tasks preceding
            WHERE preceding.team = current.team AND preceding.assignee = current.assignee
              AND preceding.state <> 'complete'
              AND (CASE preceding.state WHEN 'active' THEN 0 ELSE 1 END,
                   preceding.assigned_at, preceding.task_id)
                  <= (CASE current.state WHEN 'active' THEN 0 ELSE 1 END,
                      current.assigned_at, current.task_id)
        ) WHERE current.state <> 'complete';

        DROP TABLE active_demotions;
        "#,
    ).map_err(|error| sqlite_error(target, "failed to migrate task identity rows", error))?;

    verify_positions(transaction, target)?;
    verify_replay(transaction, target)?;
    let tasks_after = count(transaction, "tasks", target)?;
    let merged_duplicate_rows = rows_before.saturating_sub(tasks_after);
    let demoted_active_conflicts = transaction
        .query_row(
            "SELECT COUNT(*) FROM task_events WHERE event = 'migrated' AND detail = 'demoted because another active task for this member wins by original assignment time and task id'",
            [], |row| row.get(0),
        )
        .map_err(|error| sqlite_error(target, "failed to count migrated active conflicts", error))?;

    transaction
        .execute_batch(&format!(
            "DROP TABLE tasks_legacy; DROP TABLE task_events_legacy; {TASK_INDEX_DDL}"
        ))
        .map_err(|error| sqlite_error(target, "failed to finalize BA.2 task migration", error))?;
    Ok(TaskMigrationReport {
        rows_before,
        tasks_after,
        merged_duplicate_rows,
        demoted_active_conflicts,
        backup_path,
    })
}

fn verify_positions(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let invalid: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE state <> 'complete' GROUP BY team, assignee HAVING COUNT(*) <> MAX(position))",
            [], |row| row.get(0),
        )
        .map_err(|error| sqlite_error(target, "failed to verify migrated task positions", error))?;
    if invalid {
        return Err(AtmError::validation(
            "migrated task queue positions are not contiguous",
        ));
    }
    Ok(())
}

fn verify_replay(connection: &SqliteConnection, target: &SharedDbTarget) -> Result<(), AtmError> {
    let invalid: bool = connection
        .query_row(
            r#"SELECT EXISTS(
                SELECT 1 FROM tasks task
                LEFT JOIN task_events event
                  ON event.team = task.team AND event.task_id = task.task_id
                 AND event.seq = (SELECT MAX(candidate.seq) FROM task_events candidate
                                  WHERE candidate.team = task.team AND candidate.task_id = task.task_id
                                    AND candidate.to_state IS NOT NULL)
                WHERE event.to_state IS NULL OR event.to_state <> task.state
                   OR COALESCE(event.close_outcome, '') <> COALESCE(task.close_outcome, '')
            )"#,
            [], |row| row.get(0),
        )
        .map_err(|error| sqlite_error(target, "failed to verify migrated task replay", error))?;
    if invalid {
        return Err(AtmError::validation(
            "migrated task history does not replay to its task row",
        ));
    }
    Ok(())
}

fn count(
    connection: &SqliteConnection,
    table: &str,
    target: &SharedDbTarget,
) -> Result<u64, AtmError> {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .map_err(|error| sqlite_error(target, format!("failed to count {table}"), error))
}

fn backup_path(target: &SharedDbTarget) -> PathBuf {
    let suffix = Utc::now().format("%Y%m%dT%H%M%S%.fZ");
    match target {
        SharedDbTarget::Path(path) => {
            PathBuf::from(format!("{}.pre-ba2.{suffix}.sqlite", path.display()))
        }
        #[cfg(test)]
        SharedDbTarget::InMemory { .. } => std::env::temp_dir().join(format!(
            "atm-task-{}.pre-ba2.{suffix}.sqlite",
            std::process::id()
        )),
    }
}
