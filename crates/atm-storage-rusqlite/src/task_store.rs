use std::sync::Arc;

use atm_storage::types::{AgentName, IsoTimestamp, TaskId, TeamName};
use atm_storage::{
    AtmError, AtmMessageId, EscalationScope, MemberKey, ReminderOutcome, TaskActor, TaskEventKind,
    TaskEventMarker, TaskEventRow, TaskRow, TaskState, TaskStore,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::SqliteTaskStore;
use crate::shared_db::SharedDb;
use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};

const TASK_SCHEMA_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS tasks (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    assignee TEXT NOT NULL,
    assigner TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('assigned', 'active', 'complete')),
    assignment_message_id TEXT NOT NULL,
    description TEXT NOT NULL,
    assigned_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_reminded_at TEXT NULL,
    reminder_count INTEGER NOT NULL DEFAULT 0,
    lead_notified_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (team, task_id, assignee)
);

CREATE INDEX IF NOT EXISTS tasks_open_by_member
    ON tasks(team, assignee, assigned_at) WHERE state <> 'complete';

CREATE TABLE IF NOT EXISTS escalation_recipients (
    scope_key TEXT NOT NULL,
    address TEXT NOT NULL,
    added_at TEXT NOT NULL,
    PRIMARY KEY (scope_key, address)
);

CREATE TABLE IF NOT EXISTS task_events (
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
    outcome TEXT NULL CHECK(outcome IN ('emitted', 'unrenderable', 'blocked')),
    marker TEXT NULL CHECK(marker IN ('resend', 'assignment_missing')),
    detail TEXT NULL,
    PRIMARY KEY (team, task_id, assignee, seq)
);
"#;

/// Initializes the task-ledger schema outside the generic shared DB module.
pub(crate) fn ensure_schema(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    connection
        .execute_batch(TASK_SCHEMA_DDL)
        .map_err(|error| sqlite_error(target, "failed to initialize task ledger schema", error))
}

impl SqliteTaskStore {
    pub(crate) fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }

    pub(crate) fn decode_row(row: &Row<'_>) -> rusqlite::Result<TaskRow> {
        let team: String = row.get(0)?;
        let task_id: String = row.get(1)?;
        let assignee: String = row.get(2)?;
        let assigner: String = row.get(3)?;
        let state: String = row.get(4)?;
        let assignment_message_id: String = row.get(5)?;
        let description: String = row.get(6)?;
        let assigned_at: String = row.get(7)?;
        let updated_at: String = row.get(8)?;
        let last_reminded_at: Option<String> = row.get(9)?;
        let reminder_count: u32 = row.get(10)?;
        let lead_notified_count: u32 = row.get(11)?;
        Ok(TaskRow {
            team: parse(&team, "task team")?,
            task_id: parse(&task_id, "task id")?,
            assignee: parse(&assignee, "task assignee")?,
            assigner: parse(&assigner, "task assigner")?,
            state: parse_state(&state)?,
            assignment_message_id: parse(&assignment_message_id, "assignment message id")?,
            description,
            assigned_at: parse(&assigned_at, "task assigned timestamp")?,
            updated_at: parse(&updated_at, "task updated timestamp")?,
            last_reminded_at: last_reminded_at
                .as_deref()
                .map(|value| parse(value, "task reminder timestamp"))
                .transpose()?,
            reminder_count,
            lead_notified_count,
        })
    }

    pub(crate) fn decode_event_row(row: &Row<'_>) -> rusqlite::Result<TaskEventRow> {
        let actor: String = row.get(8)?;
        let message_id: Option<String> = row.get(9)?;
        let outcome: Option<String> = row.get(10)?;
        let marker: Option<String> = row.get(11)?;
        Ok(TaskEventRow {
            team: parse(&row.get::<_, String>(0)?, "event team")?,
            task_id: parse(&row.get::<_, String>(1)?, "event task id")?,
            assignee: parse(&row.get::<_, String>(2)?, "event assignee")?,
            seq: row.get(3)?,
            at: parse(&row.get::<_, String>(4)?, "event timestamp")?,
            event: parse_event(&row.get::<_, String>(5)?)?,
            from_state: row
                .get::<_, Option<String>>(6)?
                .as_deref()
                .map(parse_state)
                .transpose()?,
            to_state: row
                .get::<_, Option<String>>(7)?
                .as_deref()
                .map(parse_state)
                .transpose()?,
            actor: if actor == atm_storage::DAEMON_ACTOR_NAME {
                TaskActor::Daemon
            } else {
                TaskActor::Member(parse(&actor, "event actor")?)
            },
            message_id: message_id
                .as_deref()
                .map(|value| parse(value, "event message id"))
                .transpose()?,
            outcome: outcome.as_deref().map(parse_outcome).transpose()?,
            marker: marker.as_deref().map(parse_marker).transpose()?,
            detail: row.get(12)?,
        })
    }

    fn load_row(
        &self,
        connection: &Connection,
        member: &MemberKey,
        task_id: &TaskId,
    ) -> Result<Option<TaskRow>, AtmError> {
        connection
            .query_row(
                "SELECT team, task_id, assignee, assigner, state, assignment_message_id,
                        description, assigned_at, updated_at, last_reminded_at, reminder_count,
                        lead_notified_count
                   FROM tasks WHERE team = ?1 AND task_id = ?2 AND assignee = ?3",
                params![
                    member.team().as_str(),
                    task_id.as_str(),
                    member.agent().as_str()
                ],
                Self::decode_row,
            )
            .optional()
            .map_err(|error| self.db.error("failed to load task row", error))
    }

    #[allow(clippy::too_many_arguments)]
    fn append_event(
        &self,
        connection: &Connection,
        member: &MemberKey,
        task_id: &TaskId,
        at: IsoTimestamp,
        event: TaskEventKind,
        state: TaskState,
        actor: &str,
        message_id: Option<&AtmMessageId>,
        outcome: Option<ReminderOutcome>,
    ) -> Result<(), AtmError> {
        let seq: u64 = connection
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM task_events
                 WHERE team = ?1 AND task_id = ?2 AND assignee = ?3",
                params![
                    member.team().as_str(),
                    task_id.as_str(),
                    member.agent().as_str()
                ],
                |row| row.get(0),
            )
            .map_err(|error| {
                self.db
                    .error("failed to allocate task event sequence", error)
            })?;
        connection
            .execute(
                "INSERT INTO task_events(
                     team, task_id, assignee, seq, at, event, from_state, to_state, actor,
                     message_id, outcome, marker, detail
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8, ?9, ?10, NULL, NULL)",
                params![
                    member.team().as_str(),
                    task_id.as_str(),
                    member.agent().as_str(),
                    seq,
                    at.to_string(),
                    event_name(event),
                    state_name(state),
                    actor,
                    message_id.map(ToString::to_string),
                    outcome.map(outcome_name),
                ],
            )
            .map_err(|error| self.db.error("failed to append task event", error))?;
        Ok(())
    }
}

impl atm_storage::contract::sealed::Sealed for SqliteTaskStore {}

impl TaskStore for SqliteTaskStore {
    fn load_task(&self, member: &MemberKey, task_id: &TaskId) -> Result<Option<TaskRow>, AtmError> {
        self.db
            .with_connection(|connection| self.load_row(connection, member, task_id))
    }

    fn open_tasks(&self, member: &MemberKey) -> Result<Vec<TaskRow>, AtmError> {
        let mut tasks = self.list_tasks(&member.team, Some(&member.agent))?;
        tasks.retain(|task| task.state != TaskState::Complete);
        tasks.sort_by(|left, right| {
            left.assigned_at
                .cmp(&right.assigned_at)
                .then_with(|| left.task_id.cmp(&right.task_id))
        });
        Ok(tasks)
    }

    fn list_tasks(
        &self,
        team: &TeamName,
        member: Option<&AgentName>,
    ) -> Result<Vec<TaskRow>, AtmError> {
        self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT team, task_id, assignee, assigner, state, assignment_message_id,
                            description, assigned_at, updated_at, last_reminded_at, reminder_count,
                            lead_notified_count
                       FROM tasks WHERE team = ?1 AND (?2 IS NULL OR assignee = ?2)
                       ORDER BY assigned_at DESC, task_id DESC",
                )
                .map_err(|error| self.db.error("failed to prepare task list", error))?;
            let rows = statement
                .query_map(
                    params![team.as_str(), member.map(AgentName::as_str)],
                    Self::decode_row,
                )
                .map_err(|error| self.db.error("failed to list tasks", error))?;
            rows.map(|row| row.map_err(|error| self.db.error("failed to decode task row", error)))
                .collect()
        })
    }

    fn list_task_events(
        &self,
        team: &TeamName,
        task_id: &TaskId,
        assignee: Option<&AgentName>,
    ) -> Result<Vec<TaskEventRow>, AtmError> {
        self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT team, task_id, assignee, seq, at, event, from_state, to_state, actor,
                            message_id, outcome, marker, detail
                       FROM task_events
                      WHERE team = ?1 AND task_id = ?2 AND (?3 IS NULL OR assignee = ?3)
                      ORDER BY seq ASC",
                )
                .map_err(|error| self.db.error("failed to prepare task event list", error))?;
            let rows = statement
                .query_map(
                    params![
                        team.as_str(),
                        task_id.as_str(),
                        assignee.map(AgentName::as_str)
                    ],
                    Self::decode_event_row,
                )
                .map_err(|error| self.db.error("failed to list task events", error))?;
            rows.map(|row| row.map_err(|error| self.db.error("failed to decode task event", error)))
                .collect()
        })
    }

    fn record_reminder(
        &self,
        member: &MemberKey,
        task_id: &TaskId,
        at: IsoTimestamp,
        outcome: ReminderOutcome,
    ) -> Result<TaskRow, AtmError> {
        self.db.with_transaction(|connection| {
            let row = self.load_row(connection, member, task_id)?.ok_or_else(|| {
                AtmError::validation("cannot record a reminder for a missing task")
            })?;
            connection
                .execute(
                    "UPDATE tasks SET reminder_count = reminder_count + 1, last_reminded_at = ?4,
                     updated_at = ?4 WHERE team = ?1 AND task_id = ?2 AND assignee = ?3",
                    params![
                        member.team().as_str(),
                        task_id.as_str(),
                        member.agent().as_str(),
                        at.to_string()
                    ],
                )
                .map_err(|error| self.db.error("failed to record task reminder", error))?;
            self.append_event(
                connection,
                member,
                task_id,
                at,
                TaskEventKind::Reminded,
                row.state,
                atm_storage::DAEMON_ACTOR_NAME,
                None,
                Some(outcome),
            )?;
            self.load_row(connection, member, task_id)?
                .ok_or_else(|| AtmError::mailbox_write("task disappeared after reminder write"))
        })
    }

    fn record_lead_notified(
        &self,
        member: &MemberKey,
        task_id: &TaskId,
        at: IsoTimestamp,
        lead: &AgentName,
        message_id: &AtmMessageId,
    ) -> Result<(), AtmError> {
        self.db.with_transaction(|connection| {
            let row = self.load_row(connection, member, task_id)?.ok_or_else(|| {
                AtmError::validation("cannot record lead notification for a missing task")
            })?;
            connection.execute(
                "UPDATE tasks SET lead_notified_count = lead_notified_count + 1, updated_at = ?4
                   WHERE team = ?1 AND task_id = ?2 AND assignee = ?3",
                params![member.team().as_str(), task_id.as_str(), member.agent().as_str(), at.to_string()],
            ).map_err(|error| self.db.error("failed to record lead notification", error))?;
            self.append_event(connection, member, task_id, at, TaskEventKind::LeadNotified, row.state,
                lead.as_str(), Some(message_id), None)
        })
    }

    fn list_escalation_recipients(&self, scope: &EscalationScope) -> Result<Vec<String>, AtmError> {
        let scope_key = scope.key();
        self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT address FROM escalation_recipients
                     WHERE scope_key = ?1 ORDER BY address ASC",
                )
                .map_err(|error| {
                    self.db
                        .error("failed to prepare escalation recipient list", error)
                })?;
            let rows = statement
                .query_map([scope_key], |row| row.get(0))
                .map_err(|error| self.db.error("failed to list escalation recipients", error))?;
            rows.map(|row| {
                row.map_err(|error| {
                    self.db
                        .error("failed to decode escalation recipient", error)
                })
            })
            .collect()
        })
    }

    fn add_escalation_recipient(
        &self,
        scope: &EscalationScope,
        address: &str,
        at: atm_storage::types::IsoTimestamp,
    ) -> Result<bool, AtmError> {
        let scope_key = scope.key();
        self.db.with_connection(|connection| {
            let inserted = connection
                .execute(
                    "INSERT OR IGNORE INTO escalation_recipients(scope_key, address, added_at)
                     VALUES (?1, ?2, ?3)",
                    params![scope_key, address, at.to_string()],
                )
                .map_err(|error| self.db.error("failed to add escalation recipient", error))?;
            Ok(inserted == 1)
        })
    }

    fn remove_escalation_recipient(
        &self,
        scope: &EscalationScope,
        address: &str,
    ) -> Result<bool, AtmError> {
        let scope_key = scope.key();
        self.db.with_connection(|connection| {
            let removed = connection
                .execute(
                    "DELETE FROM escalation_recipients WHERE scope_key = ?1 AND address = ?2",
                    params![scope_key, address],
                )
                .map_err(|error| {
                    self.db
                        .error("failed to remove escalation recipient", error)
                })?;
            Ok(removed == 1)
        })
    }
}

fn parse<T: std::str::FromStr>(value: &str, subject: &str) -> rusqlite::Result<T>
where
    T::Err: std::fmt::Display + Send + Sync + 'static,
{
    value.parse().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("invalid {subject}: {error}").into(),
        )
    })
}

fn parse_state(value: &str) -> rusqlite::Result<TaskState> {
    match value {
        "assigned" => Ok(TaskState::Assigned),
        "active" => Ok(TaskState::Active),
        "complete" => Ok(TaskState::Complete),
        _ => Err(invalid(value, "task state")),
    }
}
fn parse_event(value: &str) -> rusqlite::Result<TaskEventKind> {
    match value {
        "assigned" => Ok(TaskEventKind::Assigned),
        "acked" => Ok(TaskEventKind::Acked),
        "completed" => Ok(TaskEventKind::Completed),
        "rejected" => Ok(TaskEventKind::Rejected),
        "reminded" => Ok(TaskEventKind::Reminded),
        "lead_notified" => Ok(TaskEventKind::LeadNotified),
        _ => Err(invalid(value, "task event")),
    }
}
fn parse_outcome(value: &str) -> rusqlite::Result<ReminderOutcome> {
    match value {
        "emitted" => Ok(ReminderOutcome::Emitted),
        "unrenderable" => Ok(ReminderOutcome::Unrenderable),
        "blocked" => Ok(ReminderOutcome::Blocked),
        _ => Err(invalid(value, "reminder outcome")),
    }
}
fn parse_marker(value: &str) -> rusqlite::Result<TaskEventMarker> {
    match value {
        "resend" => Ok(TaskEventMarker::Resend),
        "assignment_missing" => Ok(TaskEventMarker::AssignmentMissing),
        _ => Err(invalid(value, "task marker")),
    }
}
fn invalid(value: &str, subject: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        format!("invalid {subject}: {value}").into(),
    )
}
const fn state_name(value: TaskState) -> &'static str {
    match value {
        TaskState::Assigned => "assigned",
        TaskState::Active => "active",
        TaskState::Complete => "complete",
    }
}
const fn event_name(value: TaskEventKind) -> &'static str {
    match value {
        TaskEventKind::Assigned => "assigned",
        TaskEventKind::Acked => "acked",
        TaskEventKind::Completed => "completed",
        TaskEventKind::Rejected => "rejected",
        TaskEventKind::Reminded => "reminded",
        TaskEventKind::LeadNotified => "lead_notified",
    }
}
const fn outcome_name(value: ReminderOutcome) -> &'static str {
    match value {
        ReminderOutcome::Emitted => "emitted",
        ReminderOutcome::Unrenderable => "unrenderable",
        ReminderOutcome::Blocked => "blocked",
    }
}
