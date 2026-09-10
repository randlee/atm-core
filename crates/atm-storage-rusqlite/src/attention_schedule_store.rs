//! SQLite ownership of the identifier-only idle-attention cursor and reservation metadata.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use atm_storage::{
    AssignmentAttempt, AsyncAttentionScheduleStore, AtmError, AtmErrorCode, AttentionCursor,
    AttentionFinalizeOutcome, AttentionFinalizeRequest, AttentionItem, AttentionLane,
    AttentionReservation, AttentionReservationRequest, AttentionReservationStatus,
    AttentionScheduleStore, IdleOpportunity, IdleOpportunityId, MAX_NUDGE_ATTEMPTS, MemberKey,
    ReadDeadline, ReadLaneError, RosterStateRevision,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::SqliteAttentionScheduleStore;
use crate::shared_db::{SharedDb, SharedDbTarget, SqliteConnection, sqlite_error};

#[derive(Debug)]
struct AttentionScheduleInvariant(&'static str);

impl std::fmt::Display for AttentionScheduleInvariant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for AttentionScheduleInvariant {}

fn attention_schedule_invariant(reason: &'static str) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(AttentionScheduleInvariant(reason)))
}

const ATTENTION_SCHEMA_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS attention_lane_cursors (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    next_lane TEXT NOT NULL CHECK(next_lane IN ('ephemeral', 'persistent_task')),
    revision INTEGER NOT NULL CHECK(revision >= 0),
    PRIMARY KEY (team, agent)
);

CREATE TABLE IF NOT EXISTS attention_opportunities (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    opportunity_id TEXT NOT NULL,
    roster_state_revision INTEGER NOT NULL CHECK(roster_state_revision >= 0),
    lane TEXT NOT NULL CHECK(lane IN ('ephemeral', 'persistent_task')),
    message_id TEXT NULL,
    task_id TEXT NULL,
    assignment_attempt INTEGER NULL CHECK(assignment_attempt >= 1),
    assignment_message_id TEXT NULL,
    status TEXT NOT NULL CHECK(status IN ('reserved', 'delivered', 'stale', 'permanently_failed')),
    failed_attempts INTEGER NOT NULL DEFAULT 0 CHECK(failed_attempts >= 0),
    created_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (team, agent, opportunity_id),
    CHECK((lane = 'ephemeral' AND message_id IS NOT NULL AND task_id IS NULL
           AND assignment_attempt IS NULL AND assignment_message_id IS NULL)
       OR (lane = 'persistent_task' AND message_id IS NULL AND task_id IS NOT NULL
           AND assignment_attempt IS NOT NULL AND assignment_message_id IS NOT NULL))
);

CREATE INDEX IF NOT EXISTS attention_opportunities_member_revision
    ON attention_opportunities(team, agent, roster_state_revision);
"#;

const ATTENTION_MAX_AGE_DAYS: i64 = 30;
const ATTENTION_MAX_TERMINAL_ROWS: i64 = 10_000;

pub(crate) fn ensure_schema(
    connection: &mut SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    connection
        .execute_batch(ATTENTION_SCHEMA_DDL)
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to initialize attention scheduler schema",
                error,
            )
        })?;
    connection
        .execute_batch("ALTER TABLE attention_opportunities ADD COLUMN failed_attempts INTEGER NOT NULL DEFAULT 0 CHECK(failed_attempts >= 0);")
        .or_else(|error| {
            if error.to_string().contains("duplicate column name") { Ok(()) } else { Err(error) }
        })
        .map_err(|error| sqlite_error(target, "failed to add attention retry counter", error))?;
    connection
        .execute_batch(
            "ALTER TABLE attention_opportunities ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;",
        )
        .or_else(|error| {
            if error.to_string().contains("duplicate column name") {
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(|error| sqlite_error(target, "failed to add attention creation timestamp", error))
}

impl SqliteAttentionScheduleStore {
    pub(crate) fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }
}

impl atm_storage::contract::sealed::Sealed for SqliteAttentionScheduleStore {}

impl AttentionScheduleStore for SqliteAttentionScheduleStore {
    fn load_cursor(&self, member: &MemberKey) -> Result<AttentionCursor, AtmError> {
        let member = member.clone();
        let db = Arc::clone(&self.db);
        self.db.read(move |connection| {
            load_cursor(connection, &member)
                .map_err(|error| db.error("failed to read attention cursor", error))
        })
    }

    fn reserve(
        &self,
        request: AttentionReservationRequest,
    ) -> Result<AttentionReservation, AtmError> {
        self.db.with_transaction(|connection| {
            reserve_writer(connection, request).map_err(|error| {
                self.db
                    .error("failed to reserve attention opportunity", error)
            })
        })
    }

    fn finalize(
        &self,
        request: AttentionFinalizeRequest,
    ) -> Result<AttentionReservation, AtmError> {
        self.db.with_transaction(|connection| {
            finalize_writer(connection, request).map_err(|error| {
                self.db
                    .error("failed to finalize attention opportunity", error)
            })
        })
    }
}

#[async_trait::async_trait]
impl AsyncAttentionScheduleStore for SqliteAttentionScheduleStore {
    async fn load_cursor(
        &self,
        member: MemberKey,
        deadline: ReadDeadline,
    ) -> Result<AttentionCursor, ReadLaneError> {
        let db = Arc::clone(&self.db);
        self.db
            .read_with_deadline_async(deadline.remaining(), move |connection| {
                load_cursor(connection, &member)
                    .map_err(|error| db.error("failed to read attention cursor", error))
            })
            .await
            .map_err(crate::mailbox_reader::read_lane_storage_error)
    }

    async fn reserve(
        &self,
        request: AttentionReservationRequest,
        deadline: ReadDeadline,
    ) -> Result<AttentionReservation, AtmError> {
        let db = Arc::clone(&self.db);
        execute_schedule_write(
            move || {
                db.with_transaction(|connection| {
                    reserve_writer(connection, request)
                        .map_err(|error| db.error("failed to reserve attention opportunity", error))
                })
            },
            deadline,
        )
        .await
    }

    async fn finalize(
        &self,
        request: AttentionFinalizeRequest,
        deadline: ReadDeadline,
    ) -> Result<AttentionReservation, AtmError> {
        let db = Arc::clone(&self.db);
        execute_schedule_write(
            move || {
                db.with_transaction(|connection| {
                    finalize_writer(connection, request).map_err(|error| {
                        db.error("failed to finalize attention opportunity", error)
                    })
                })
            },
            deadline,
        )
        .await
    }
}

async fn execute_schedule_write<T>(
    operation: impl FnOnce() -> Result<T, AtmError> + Send + 'static,
    deadline: ReadDeadline,
) -> Result<T, AtmError>
where
    T: Send + 'static,
{
    tokio::time::timeout(deadline.remaining(), tokio::task::spawn_blocking(operation))
        .await
        .map_err(|_| {
            AtmError::new(
                AtmErrorCode::WaitTimeout,
                "attention schedule storage write exceeded its request deadline",
            )
        })?
        .map_err(|source| {
            AtmError::daemon_unavailable("attention schedule storage worker ended unexpectedly")
                .with_cause(source)
        })?
}

fn load_cursor(connection: &Connection, member: &MemberKey) -> rusqlite::Result<AttentionCursor> {
    let row = connection
        .query_row(
            "SELECT next_lane, revision FROM attention_lane_cursors WHERE team = ?1 AND agent = ?2",
            params![member.team().as_str(), member.agent().as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
        )
        .optional()?;
    Ok(match row {
        None => AttentionCursor {
            next_lane: AttentionLane::Ephemeral,
            revision: atm_storage::AttentionCursorRevision::from_raw(0),
        },
        Some((lane, revision)) => AttentionCursor {
            next_lane: parse_lane(&lane)
                .map_err(|_| attention_schedule_invariant("invalid attention cursor lane"))?,
            revision: atm_storage::AttentionCursorRevision::from_raw(revision),
        },
    })
}

pub(crate) fn reserve_writer(
    connection: &Connection,
    request: AttentionReservationRequest,
) -> rusqlite::Result<AttentionReservation> {
    prune_terminal_rows(connection)?;
    if let Some(existing) = load_reservation(
        connection,
        &request.opportunity.member,
        request.opportunity.id,
    )? {
        return Ok(existing);
    }
    if let Some(mut existing) = load_unfinished_reservation_for_item(
        connection,
        &request.opportunity.member,
        &request.item,
    )? {
        // A retry consumes a later idle observation, but retains the durable
        // reservation identity and failure counter.  Refreshing only the
        // observed roster revision lets dispatch revalidate against the
        // current idle state without minting a second attempt chain.
        if existing.status == AttentionReservationStatus::Reserved {
            connection.execute(
                "UPDATE attention_opportunities SET roster_state_revision = ?4
                 WHERE team = ?1 AND agent = ?2 AND opportunity_id = ?3",
                params![
                    request.opportunity.member.team().as_str(),
                    request.opportunity.member.agent().as_str(),
                    existing.opportunity.id.to_string(),
                    request.opportunity.roster_state_revision.get(),
                ],
            )?;
            existing.opportunity.roster_state_revision = request.opportunity.roster_state_revision;
        }
        return Ok(existing);
    }
    let cursor = load_cursor(connection, &request.opportunity.member)?;
    if cursor.revision != request.expected_cursor_revision {
        return Err(attention_schedule_invariant(
            "attention cursor revision mismatch",
        ));
    }
    if request_item_member(&request.item) != &request.opportunity.member {
        return Err(attention_schedule_invariant(
            "attention item member mismatch",
        ));
    }
    let columns = item_columns(&request.item);
    connection.execute(
        "INSERT INTO attention_opportunities(
            team, agent, opportunity_id, roster_state_revision, lane, message_id, task_id,
            assignment_attempt, assignment_message_id, status, failed_attempts, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'reserved', 0, ?10)",
        params![
            request.opportunity.member.team().as_str(),
            request.opportunity.member.agent().as_str(),
            request.opportunity.id.to_string(),
            request.opportunity.roster_state_revision.get(),
            request.item.lane().as_str(),
            columns.message_id,
            columns.task_id,
            columns.attempt,
            columns.assignment_message_id,
            now_unix_ms(),
        ],
    )?;
    let next_lane = request.item.lane().other();
    connection.execute(
        "INSERT INTO attention_lane_cursors(team, agent, next_lane, revision) VALUES (?1, ?2, ?3, 1)
         ON CONFLICT(team, agent) DO UPDATE SET next_lane = excluded.next_lane, revision = attention_lane_cursors.revision + 1",
        params![request.opportunity.member.team().as_str(), request.opportunity.member.agent().as_str(), next_lane.as_str()],
    )?;
    Ok(AttentionReservation {
        opportunity: request.opportunity,
        item: request.item,
        status: AttentionReservationStatus::Reserved,
        failed_attempts: 0,
    })
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

fn prune_terminal_rows(connection: &Connection) -> rusqlite::Result<()> {
    let cutoff =
        now_unix_ms().saturating_sub(ATTENTION_MAX_AGE_DAYS.saturating_mul(24 * 60 * 60 * 1_000));
    connection.execute(
        "DELETE FROM attention_opportunities
         WHERE status <> 'reserved' AND created_at < ?1",
        params![cutoff],
    )?;
    connection.execute(
        "DELETE FROM attention_opportunities
         WHERE rowid IN (
             SELECT rowid FROM attention_opportunities
             WHERE status <> 'reserved'
             ORDER BY created_at DESC, rowid DESC
             LIMIT -1 OFFSET ?1
         )",
        params![ATTENTION_MAX_TERMINAL_ROWS],
    )?;
    Ok(())
}

fn load_unfinished_reservation_for_item(
    connection: &Connection,
    member: &MemberKey,
    item: &AttentionItem,
) -> rusqlite::Result<Option<AttentionReservation>> {
    let filter = match item {
        AttentionItem::EphemeralMessage { message_id, .. } => UnfinishedReservationFilter {
            lane: AttentionLane::Ephemeral,
            columns: ItemColumns {
                message_id: Some(message_id.to_string()),
                task_id: None,
                attempt: None,
                assignment_message_id: None,
            },
        },
        AttentionItem::PersistentTaskReminder {
            task_id,
            attempt,
            assignment_message_id,
            ..
        } => UnfinishedReservationFilter {
            lane: AttentionLane::PersistentTask,
            columns: ItemColumns {
                message_id: None,
                task_id: Some(task_id.to_string()),
                attempt: Some(attempt.get()),
                assignment_message_id: Some(assignment_message_id.to_string()),
            },
        },
    };
    connection
        .query_row(
            "SELECT opportunity_id, roster_state_revision, lane, message_id, task_id,
                    assignment_attempt, assignment_message_id, status, failed_attempts
             FROM attention_opportunities
             WHERE team = ?1 AND agent = ?2 AND lane = ?3
               AND status = 'reserved'
               AND message_id IS ?4 AND task_id IS ?5 AND assignment_attempt IS ?6
               AND assignment_message_id IS ?7
             ORDER BY rowid DESC LIMIT 1",
            params![
                member.team().as_str(),
                member.agent().as_str(),
                filter.lane.as_str(),
                filter.columns.message_id,
                filter.columns.task_id,
                filter.columns.attempt,
                filter.columns.assignment_message_id,
            ],
            |row| {
                let id: String = row.get(0)?;
                decode_reservation_with_offset(
                    row,
                    member.clone(),
                    IdleOpportunityId::parse(&id).map_err(|_| {
                        attention_schedule_invariant("invalid stored attention opportunity id")
                    })?,
                    1,
                )
            },
        )
        .optional()
}

pub(crate) fn finalize_writer(
    connection: &Connection,
    request: AttentionFinalizeRequest,
) -> rusqlite::Result<AttentionReservation> {
    let existing = load_reservation(connection, &request.member, request.opportunity_id)?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    if existing.status != AttentionReservationStatus::Reserved {
        return Err(attention_schedule_invariant(
            "attention reservation is already finalized",
        ));
    }
    let (status, failed_attempts) = match request.outcome {
        AttentionFinalizeOutcome::Delivered => (
            AttentionReservationStatus::Delivered,
            existing.failed_attempts,
        ),
        AttentionFinalizeOutcome::Stale => {
            (AttentionReservationStatus::Stale, existing.failed_attempts)
        }
        AttentionFinalizeOutcome::RetryableFailure => {
            let failed_attempts = existing.failed_attempts.saturating_add(1);
            (
                if failed_attempts >= MAX_NUDGE_ATTEMPTS {
                    AttentionReservationStatus::PermanentlyFailed
                } else {
                    AttentionReservationStatus::Reserved
                },
                failed_attempts,
            )
        }
    };
    connection.execute(
        "UPDATE attention_opportunities SET status = ?4, failed_attempts = ?5
         WHERE team = ?1 AND agent = ?2 AND opportunity_id = ?3 AND status = 'reserved'",
        params![
            request.member.team().as_str(),
            request.member.agent().as_str(),
            request.opportunity_id.to_string(),
            status_name(status),
            failed_attempts,
        ],
    )?;
    load_reservation(connection, &request.member, request.opportunity_id)?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)
}

fn load_reservation(
    connection: &Connection,
    member: &MemberKey,
    id: IdleOpportunityId,
) -> rusqlite::Result<Option<AttentionReservation>> {
    connection
        .query_row(
            "SELECT roster_state_revision, lane, message_id, task_id, assignment_attempt,
                assignment_message_id, status, failed_attempts
         FROM attention_opportunities
         WHERE team = ?1 AND agent = ?2 AND opportunity_id = ?3",
            params![
                member.team().as_str(),
                member.agent().as_str(),
                id.to_string()
            ],
            |row| decode_reservation(row, member.clone(), id),
        )
        .optional()
}

fn request_item_member(item: &AttentionItem) -> &MemberKey {
    match item {
        AttentionItem::EphemeralMessage { member, .. }
        | AttentionItem::PersistentTaskReminder { member, .. } => member,
    }
}

struct ItemColumns {
    message_id: Option<String>,
    task_id: Option<String>,
    attempt: Option<u32>,
    assignment_message_id: Option<String>,
}

struct UnfinishedReservationFilter {
    lane: AttentionLane,
    columns: ItemColumns,
}

fn item_columns(item: &AttentionItem) -> ItemColumns {
    match item {
        AttentionItem::EphemeralMessage { message_id, .. } => ItemColumns {
            message_id: Some(message_id.to_string()),
            task_id: None,
            attempt: None,
            assignment_message_id: None,
        },
        AttentionItem::PersistentTaskReminder {
            task_id,
            attempt,
            assignment_message_id,
            ..
        } => ItemColumns {
            message_id: None,
            task_id: Some(task_id.to_string()),
            attempt: Some(attempt.get()),
            assignment_message_id: Some(assignment_message_id.to_string()),
        },
    }
}
fn parse_lane(value: &str) -> Result<AttentionLane, ()> {
    match value {
        "ephemeral" => Ok(AttentionLane::Ephemeral),
        "persistent_task" => Ok(AttentionLane::PersistentTask),
        _ => Err(()),
    }
}
fn status_name(value: AttentionReservationStatus) -> &'static str {
    match value {
        AttentionReservationStatus::Reserved => "reserved",
        AttentionReservationStatus::Delivered => "delivered",
        AttentionReservationStatus::Stale => "stale",
        AttentionReservationStatus::PermanentlyFailed => "permanently_failed",
    }
}

fn decode_reservation(
    row: &Row<'_>,
    member: MemberKey,
    id: IdleOpportunityId,
) -> rusqlite::Result<AttentionReservation> {
    decode_reservation_with_offset(row, member, id, 0)
}

fn decode_reservation_with_offset(
    row: &Row<'_>,
    member: MemberKey,
    id: IdleOpportunityId,
    offset: usize,
) -> rusqlite::Result<AttentionReservation> {
    let revision: u64 = row.get(offset)?;
    let lane: String = row.get(offset + 1)?;
    let status: String = row.get(offset + 6)?;
    let failed_attempts: u32 = row.get(offset + 7)?;
    let item = match lane.as_str() {
        "ephemeral" => AttentionItem::EphemeralMessage {
            member: member.clone(),
            message_id: row
                .get::<_, String>(offset + 2)?
                .parse()
                .map_err(|_| attention_schedule_invariant("invalid stored ephemeral message id"))?,
        },
        "persistent_task" => AttentionItem::PersistentTaskReminder {
            member: member.clone(),
            task_id: row
                .get::<_, String>(offset + 3)?
                .parse()
                .map_err(|_| attention_schedule_invariant("invalid stored persistent task id"))?,
            attempt: AssignmentAttempt::new(row.get(offset + 4)?)
                .map_err(|_| attention_schedule_invariant("invalid stored assignment attempt"))?,
            assignment_message_id: row.get::<_, String>(offset + 5)?.parse().map_err(|_| {
                attention_schedule_invariant("invalid stored assignment message id")
            })?,
        },
        _ => {
            return Err(attention_schedule_invariant(
                "unknown stored attention lane",
            ));
        }
    };
    let status = match status.as_str() {
        "reserved" => AttentionReservationStatus::Reserved,
        "delivered" => AttentionReservationStatus::Delivered,
        "stale" => AttentionReservationStatus::Stale,
        "permanently_failed" => AttentionReservationStatus::PermanentlyFailed,
        _ => {
            return Err(attention_schedule_invariant(
                "unknown stored attention reservation status",
            ));
        }
    };
    Ok(AttentionReservation {
        opportunity: IdleOpportunity {
            id,
            member,
            roster_state_revision: RosterStateRevision::from_raw(revision),
        },
        item,
        status,
        failed_attempts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SqliteStorageBackend;
    use atm_storage::AttentionCursorRevision;
    use atm_storage::{AgentName, AtmMessageId, ReadDeadline, TeamName};
    use std::time::Duration;

    fn member() -> MemberKey {
        MemberKey::new(
            "attention-team".parse::<TeamName>().expect("team"),
            "attention-agent".parse::<AgentName>().expect("agent"),
        )
    }

    #[test]
    fn reservation_replay_is_idempotent_and_persists_the_fair_cursor() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.attention_schedule_store();
        let opportunity = IdleOpportunity {
            id: IdleOpportunityId::new(),
            member: member(),
            roster_state_revision: RosterStateRevision::from_raw(7),
        };
        let item = AttentionItem::EphemeralMessage {
            member: member(),
            message_id: AtmMessageId::new(),
        };
        let request = AttentionReservationRequest {
            opportunity: opportunity.clone(),
            expected_cursor_revision: AttentionCursorRevision::from_raw(0),
            item: item.clone(),
        };
        let first = store.reserve(request.clone()).expect("reserve");
        let replay = store.reserve(request).expect("replay");
        assert_eq!(first, replay);
        assert_eq!(
            store.load_cursor(&member()).expect("cursor"),
            AttentionCursor {
                next_lane: AttentionLane::PersistentTask,
                revision: AttentionCursorRevision::from_raw(1),
            }
        );
        assert_eq!(
            store
                .finalize(AttentionFinalizeRequest {
                    member: member(),
                    opportunity_id: opportunity.id,
                    outcome: AttentionFinalizeOutcome::Delivered,
                })
                .expect("finalize")
                .status,
            AttentionReservationStatus::Delivered
        );
    }

    #[test]
    fn fifth_failed_reservation_allows_a_later_idle_opportunity() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.attention_schedule_store();
        let opportunity = IdleOpportunity {
            id: IdleOpportunityId::new(),
            member: member(),
            roster_state_revision: RosterStateRevision::from_raw(7),
        };
        let reservation = store
            .reserve(AttentionReservationRequest {
                opportunity: opportunity.clone(),
                expected_cursor_revision: AttentionCursorRevision::from_raw(0),
                item: AttentionItem::EphemeralMessage {
                    member: member(),
                    message_id: AtmMessageId::new(),
                },
            })
            .expect("reserve");
        for attempt in 1..=5 {
            let updated = store
                .finalize(AttentionFinalizeRequest {
                    member: member(),
                    opportunity_id: opportunity.id,
                    outcome: AttentionFinalizeOutcome::RetryableFailure,
                })
                .expect("record retryable failure");
            assert_eq!(updated.item, reservation.item);
            assert_eq!(updated.failed_attempts, attempt);
            assert_eq!(
                updated.status,
                if attempt < 5 {
                    AttentionReservationStatus::Reserved
                } else {
                    AttentionReservationStatus::PermanentlyFailed
                }
            );
        }

        let later_opportunity = IdleOpportunity {
            id: IdleOpportunityId::new(),
            member: member(),
            roster_state_revision: RosterStateRevision::from_raw(8),
        };
        let retry = store
            .reserve(AttentionReservationRequest {
                opportunity: later_opportunity.clone(),
                expected_cursor_revision: AttentionCursorRevision::from_raw(1),
                item: reservation.item.clone(),
            })
            .expect("later opportunity reserves the still-eligible item");
        assert_eq!(retry.status, AttentionReservationStatus::Reserved);
        assert_eq!(retry.failed_attempts, 0);
        assert_eq!(retry.opportunity, later_opportunity);
        assert_ne!(retry.opportunity.id, opportunity.id);
        assert_eq!(
            store
                .reserve(AttentionReservationRequest {
                    opportunity: later_opportunity,
                    expected_cursor_revision: AttentionCursorRevision::from_raw(1),
                    item: reservation.item,
                })
                .expect("same opportunity replay")
                .opportunity
                .id,
            retry.opportunity.id,
            "the later opportunity stays idempotent while a terminal prior reservation does not suppress it"
        );
    }

    #[test]
    fn pruning_removes_old_terminal_rows_but_retains_reserved_rows() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.attention_schedule_store();
        let first = store
            .reserve(AttentionReservationRequest {
                opportunity: IdleOpportunity {
                    id: IdleOpportunityId::new(),
                    member: member(),
                    roster_state_revision: RosterStateRevision::from_raw(1),
                },
                expected_cursor_revision: AttentionCursorRevision::from_raw(0),
                item: AttentionItem::EphemeralMessage {
                    member: member(),
                    message_id: AtmMessageId::new(),
                },
            })
            .expect("first reserve");
        store
            .finalize(AttentionFinalizeRequest {
                member: member(),
                opportunity_id: first.opportunity.id,
                outcome: AttentionFinalizeOutcome::Delivered,
            })
            .expect("terminalize first");
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                connection
                    .execute(
                        "UPDATE attention_opportunities SET created_at = 0 WHERE opportunity_id = ?1",
                        params![first.opportunity.id.to_string()],
                    )
                    .expect("age terminal row");
                Ok(())
            })
            .expect("age terminal row");

        let reserved = store
            .reserve(AttentionReservationRequest {
                opportunity: IdleOpportunity {
                    id: IdleOpportunityId::new(),
                    member: member(),
                    roster_state_revision: RosterStateRevision::from_raw(2),
                },
                expected_cursor_revision: AttentionCursorRevision::from_raw(1),
                item: AttentionItem::EphemeralMessage {
                    member: member(),
                    message_id: AtmMessageId::new(),
                },
            })
            .expect("reserved row");
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                connection
                    .execute(
                        "UPDATE attention_opportunities SET created_at = 0 WHERE opportunity_id = ?1",
                        params![reserved.opportunity.id.to_string()],
                    )
                    .expect("age reserved row");
                Ok(())
            })
            .expect("age reserved row");

        let later = store
            .reserve(AttentionReservationRequest {
                opportunity: IdleOpportunity {
                    id: IdleOpportunityId::new(),
                    member: member(),
                    roster_state_revision: RosterStateRevision::from_raw(3),
                },
                expected_cursor_revision: AttentionCursorRevision::from_raw(2),
                item: AttentionItem::EphemeralMessage {
                    member: member(),
                    message_id: AtmMessageId::new(),
                },
            })
            .expect("later reserve triggers pruning");
        assert!(store.load_cursor(&member()).expect("cursor").revision.get() >= 3);
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let terminal_count: u64 = connection.query_row(
                    "SELECT COUNT(*) FROM attention_opportunities WHERE opportunity_id = ?1",
                    params![first.opportunity.id.to_string()],
                    |row| row.get(0),
                )
                .map_err(|error| atm_storage::AtmError::daemon_unavailable(error.to_string()))?;
                let reserved_count: u64 = connection.query_row(
                    "SELECT COUNT(*) FROM attention_opportunities WHERE opportunity_id = ?1 AND status = 'reserved'",
                    params![reserved.opportunity.id.to_string()],
                    |row| row.get(0),
                )
                .map_err(|error| atm_storage::AtmError::daemon_unavailable(error.to_string()))?;
                assert_eq!(terminal_count, 0);
                assert_eq!(reserved_count, 1);
                let _ = later;
                Ok(())
            })
            .expect("inspect pruned rows");
    }

    #[tokio::test]
    async fn async_store_uses_bounded_read_and_off_executor_write_paths() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_attention_schedule_store();
        let opportunity = IdleOpportunity {
            id: IdleOpportunityId::new(),
            member: member(),
            roster_state_revision: RosterStateRevision::from_raw(7),
        };
        let deadline = || ReadDeadline::new(Duration::from_secs(1)).expect("deadline");

        assert_eq!(
            store
                .load_cursor(member(), deadline())
                .await
                .expect("cursor"),
            AttentionCursor {
                next_lane: AttentionLane::Ephemeral,
                revision: AttentionCursorRevision::from_raw(0),
            }
        );
        let reservation = store
            .reserve(
                AttentionReservationRequest {
                    opportunity: opportunity.clone(),
                    expected_cursor_revision: AttentionCursorRevision::from_raw(0),
                    item: AttentionItem::EphemeralMessage {
                        member: member(),
                        message_id: AtmMessageId::new(),
                    },
                },
                deadline(),
            )
            .await
            .expect("reserve");
        assert_eq!(reservation.status, AttentionReservationStatus::Reserved);
        assert_eq!(
            store
                .finalize(
                    AttentionFinalizeRequest {
                        member: member(),
                        opportunity_id: opportunity.id,
                        outcome: AttentionFinalizeOutcome::Delivered,
                    },
                    deadline()
                )
                .await
                .expect("finalize")
                .status,
            AttentionReservationStatus::Delivered
        );
    }
}
