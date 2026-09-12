//! Crate-private task-ledger read projections shared by every SQLite lane.

use atm_storage::types::{AgentName, TaskId, TeamName};
use atm_storage::{RefusalRun, TaskEventRow, TaskRow};
use rusqlite::{Connection, OptionalExtension, params};

use crate::SqliteTaskStore;

pub(crate) const TASK_COLUMNS: &str = "team, task_id, assignee, assigner, state, close_outcome, position, assignment_message_id, description, assigned_at, updated_at, last_reminded_at, reminder_count, lead_notified_count";
pub(crate) const TASK_EVENT_COLUMNS: &str = "team, task_id, assignee, seq, at, event, from_state, to_state, close_outcome, actor, message_id, outcome, marker, detail";

pub(crate) fn select_tasks_for_team_sql() -> String {
    format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE team = ?1 AND (?2 IS NULL OR assignee = ?2) ORDER BY assigned_at DESC, task_id DESC"
    )
}

pub(crate) fn select_task_events_sql() -> String {
    format!(
        "SELECT {TASK_EVENT_COLUMNS} FROM task_events WHERE team = ?1 AND task_id = ?2 AND (?3 IS NULL OR assignee = ?3) ORDER BY seq ASC"
    )
}

pub(crate) fn select_task_row(
    connection: &Connection,
    team: &TeamName,
    task_id: &TaskId,
) -> rusqlite::Result<Option<TaskRow>> {
    connection
        .query_row(
            &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE team = ?1 AND task_id = ?2"),
            params![team.as_str(), task_id.as_str()],
            SqliteTaskStore::decode_row,
        )
        .optional()
}

pub(crate) fn select_open_tasks_for_member(
    connection: &Connection,
    team: &TeamName,
    assignee: &AgentName,
) -> rusqlite::Result<Vec<TaskRow>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE team = ?1 AND assignee = ?2 AND state <> 'complete' ORDER BY position ASC, assigned_at ASC, task_id ASC"
    ))?;
    statement
        .query_map(
            params![team.as_str(), assignee.as_str()],
            SqliteTaskStore::decode_row,
        )?
        .collect()
}

pub(crate) fn select_open_tasks_for_team(
    connection: &Connection,
    team: &TeamName,
) -> rusqlite::Result<Vec<TaskRow>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE team=?1 AND state<>'complete' ORDER BY assignee, position, assigned_at, task_id"
    ))?;
    statement
        .query_map(params![team.as_str()], SqliteTaskStore::decode_row)?
        .collect()
}

pub(crate) fn trailing_refusal_run(
    connection: &Connection,
    team: &TeamName,
    assignee: &AgentName,
) -> rusqlite::Result<RefusalRun> {
    let mut statement = connection.prepare(
        "SELECT event, at FROM task_events WHERE team=?1 AND assignee=?2
         AND event IN ('assigned','reassigned','reopened','completed','refused','cancelled')
         ORDER BY rowid DESC",
    )?;
    let rows = statement
        .query_map(params![team.as_str(), assignee.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let refused = rows
        .iter()
        .take_while(|(event, _)| event == "refused")
        .collect::<Vec<_>>();
    let started_at = refused
        .last()
        .map(|(_, at)| {
            at.parse().map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()?;
    Ok(RefusalRun {
        count: u32::try_from(refused.len()).unwrap_or(u32::MAX),
        started_at,
    })
}

pub(crate) fn select_tasks_for_team(
    connection: &Connection,
    team: &TeamName,
    member: Option<&AgentName>,
) -> rusqlite::Result<Vec<TaskRow>> {
    let mut statement = connection.prepare(&select_tasks_for_team_sql())?;
    statement
        .query_map(
            params![team.as_str(), member.map(AgentName::as_str)],
            SqliteTaskStore::decode_row,
        )?
        .collect()
}

pub(crate) fn select_task_events(
    connection: &Connection,
    team: &TeamName,
    task_id: &TaskId,
    member: Option<&AgentName>,
) -> rusqlite::Result<Vec<TaskEventRow>> {
    let mut statement = connection.prepare(&select_task_events_sql())?;
    statement
        .query_map(
            params![
                team.as_str(),
                task_id.as_str(),
                member.map(AgentName::as_str)
            ],
            SqliteTaskStore::decode_event_row,
        )?
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn task_projection_column_lists_have_one_crate_owner() {
        let task_sources = [
            include_str!("task_sql.rs"),
            include_str!("task_store.rs"),
            include_str!("task_ledger_reader.rs"),
            include_str!("writer/task_ops.rs"),
        ];
        assert_eq!(
            task_sources
                .iter()
                .map(|source| source
                    .matches(concat!("const TASK_", "COLUMNS: &str ="))
                    .count())
                .sum::<usize>(),
            1,
            "task row column projection must remain owned by task_sql",
        );
        assert_eq!(
            task_sources
                .iter()
                .map(|source| {
                    source
                        .matches(concat!("const TASK_EVENT_", "COLUMNS: &str ="))
                        .count()
                })
                .sum::<usize>(),
            1,
            "task event column projection must remain owned by task_sql",
        );
    }
}
