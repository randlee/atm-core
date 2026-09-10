//! SQLite ownership of the identifier-only idle-attention cursor and reservation metadata.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use atm_storage::{
    ATTENTION_RESERVATION_LEASE, AssignmentAttempt, AsyncAttentionScheduleStore, AtmError,
    AtmErrorCode, AttentionClaimDisposition, AttentionCursor, AttentionFinalizeOutcome,
    AttentionFinalizeRequest, AttentionItem, AttentionLane, AttentionReservation,
    AttentionReservationRequest, AttentionReservationStatus, AttentionScheduleStore,
    IdleOpportunity, IdleOpportunityId, MAX_NUDGE_ATTEMPTS, MemberKey, ReadDeadline, ReadLaneError,
    RosterStateRevision,
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
    reserved_until INTEGER NOT NULL DEFAULT 0,
    lease_generation INTEGER NOT NULL DEFAULT 1 CHECK(lease_generation >= 1),
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
const ATTENTION_RESERVATION_LEASE_MS: i64 = ATTENTION_RESERVATION_LEASE.as_millis() as i64;

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
        .map_err(|error| {
            sqlite_error(target, "failed to add attention creation timestamp", error)
        })?;
    connection
        .execute_batch(
            "ALTER TABLE attention_opportunities ADD COLUMN reserved_until INTEGER NOT NULL DEFAULT 0;",
        )
        .or_else(|error| {
            if error.to_string().contains("duplicate column name") {
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(|error| sqlite_error(target, "failed to add attention ownership expiry", error))?;
    connection
        .execute_batch(
            "ALTER TABLE attention_opportunities ADD COLUMN lease_generation INTEGER NOT NULL DEFAULT 1 CHECK(lease_generation >= 1);",
        )
        .or_else(|error| {
            if error.to_string().contains("duplicate column name") { Ok(()) } else { Err(error) }
        })
        .map_err(|error| sqlite_error(target, "failed to add attention lease generation", error))?;
    // The 2.1 upgrade drops all pre-existing terminal opportunity rows on the first prune; reserved rows are exempt.
    connection
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS attention_opportunities_status_created_at
             ON attention_opportunities(status, created_at);",
        )
        .map_err(|error| sqlite_error(target, "failed to index attention retention columns", error))
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
    if let Some((mut existing, reserved_until)) = load_unfinished_reservation_for_item(
        connection,
        &request.opportunity.member,
        &request.item,
    )? {
        return refresh_existing_reservation(connection, request, &mut existing, reserved_until);
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
    let now = now_unix_ms();
    connection.execute(
        "INSERT INTO attention_opportunities(
            team, agent, opportunity_id, roster_state_revision, lane, message_id, task_id,
            assignment_attempt, assignment_message_id, status, failed_attempts, created_at,
            reserved_until, lease_generation
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'reserved', 0, ?10, ?11, 1)",
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
            now,
            now.saturating_add(ATTENTION_RESERVATION_LEASE_MS),
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
        lease_generation: 1,
        disposition: AttentionClaimDisposition::Acquired,
    })
}

fn refresh_existing_reservation(
    connection: &Connection,
    request: AttentionReservationRequest,
    existing: &mut AttentionReservation,
    reserved_until: i64,
) -> rusqlite::Result<AttentionReservation> {
    // A retry consumes a later idle observation, but retains the durable
    // reservation identity and failure counter.  Refreshing only the
    // observed roster revision lets dispatch revalidate against the
    // current idle state without minting a second attempt chain.
    if existing.status == AttentionReservationStatus::Reserved && reserved_until <= now_unix_ms() {
        // A process crash can happen after an external sink accepts a
        // prompt and before finalize commits. The AZ.4 plan therefore
        // specifies bounded at-least-once recovery, not exactly-once.
        let failed_attempts = existing.failed_attempts.saturating_add(1);
        let status = if failed_attempts >= MAX_NUDGE_ATTEMPTS {
            AttentionReservationStatus::PermanentlyFailed
        } else {
            AttentionReservationStatus::Reserved
        };
        let lease_generation = existing.lease_generation.saturating_add(1);
        let now = now_unix_ms();
        connection.execute(
            "UPDATE attention_opportunities SET roster_state_revision = ?4,
                    status = ?5, failed_attempts = ?6, reserved_until = ?7,
                    lease_generation = ?8
                 WHERE team = ?1 AND agent = ?2 AND opportunity_id = ?3
                   AND status = 'reserved' AND reserved_until <= ?9",
            params![
                request.opportunity.member.team().as_str(),
                request.opportunity.member.agent().as_str(),
                existing.opportunity.id.to_string(),
                request.opportunity.roster_state_revision.get(),
                status_name(status),
                failed_attempts,
                if status == AttentionReservationStatus::Reserved {
                    now_unix_ms().saturating_add(ATTENTION_RESERVATION_LEASE_MS)
                } else {
                    0
                },
                lease_generation,
                now,
            ],
        )?;
        if connection.changes() == 0 {
            return load_reservation(
                connection,
                &request.opportunity.member,
                existing.opportunity.id,
            )?
            .ok_or(rusqlite::Error::QueryReturnedNoRows);
        }
        existing.opportunity.roster_state_revision = request.opportunity.roster_state_revision;
        existing.failed_attempts = failed_attempts;
        existing.status = status;
        existing.lease_generation = lease_generation;
        existing.disposition = AttentionClaimDisposition::Acquired;
    } else if existing.status == AttentionReservationStatus::Reserved {
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
        existing.disposition = AttentionClaimDisposition::Observed;
    }
    Ok(existing.clone())
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
) -> rusqlite::Result<Option<(AttentionReservation, i64)>> {
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
                    assignment_attempt, assignment_message_id, status, failed_attempts,
                    reserved_until, lease_generation
             FROM attention_opportunities
             WHERE team = ?1 AND agent = ?2 AND lane = ?3
               AND (status = 'reserved' OR
                    (status = 'permanently_failed' AND failed_attempts >= ?8))
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
                MAX_NUDGE_ATTEMPTS,
            ],
            |row| {
                let id: String = row.get(0)?;
                let reservation = decode_reservation_with_offset(
                    row,
                    member.clone(),
                    IdleOpportunityId::parse(&id).map_err(|_| {
                        attention_schedule_invariant("invalid stored attention opportunity id")
                    })?,
                    1,
                )?;
                Ok((reservation, row.get(9)?))
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
        "UPDATE attention_opportunities SET status = ?4, failed_attempts = ?5,
            reserved_until = ?6
         WHERE team = ?1 AND agent = ?2 AND opportunity_id = ?3
           AND lease_generation = ?7 AND status = 'reserved'",
        params![
            request.member.team().as_str(),
            request.member.agent().as_str(),
            request.opportunity_id.to_string(),
            status_name(status),
            failed_attempts,
            if status == AttentionReservationStatus::Reserved {
                // A retryable finalize relinquishes ownership immediately;
                // the next idle opportunity must reacquire through the
                // fenced reclaim path rather than observe a live claim.
                0
            } else {
                0
            },
            request.lease_generation,
        ],
    )?;
    if connection.changes() != 1 {
        return Err(attention_schedule_invariant(
            "attention reservation lease generation is stale",
        ));
    }
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
                assignment_message_id, status, failed_attempts, reserved_until,
                lease_generation
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
    let lease_generation: u64 = row.get(offset + 9)?;
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
        lease_generation,
        disposition: AttentionClaimDisposition::Observed,
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
    fn live_reservation_reads_reserved_until_and_is_not_reclaimed() {
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
        // Regression guard for the SELECT column offset: reading
        // failed_attempts (index 8) would make this look expired and return
        // Acquired instead of the non-dispatchable Observed outcome.
        let replay = store.reserve(request).expect("replay");
        assert_eq!(first.opportunity, replay.opportunity);
        assert_eq!(first.item, replay.item);
        assert_eq!(replay.disposition, AttentionClaimDisposition::Observed);
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
                    lease_generation: first.lease_generation,
                })
                .expect("finalize")
                .status,
            AttentionReservationStatus::Delivered
        );
    }

    #[test]
    fn fifth_failed_reservation_suppresses_later_dispatch() {
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
                    lease_generation: reservation.lease_generation,
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
        assert_eq!(retry.status, AttentionReservationStatus::PermanentlyFailed);
        assert_eq!(retry.failed_attempts, MAX_NUDGE_ATTEMPTS);
        assert_eq!(retry.opportunity.id, opportunity.id);
        assert_eq!(retry.disposition, AttentionClaimDisposition::Observed);
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
                lease_generation: first.lease_generation,
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

    #[test]
    fn pruning_caps_terminal_rows_at_the_configured_limit() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let transaction = connection.transaction().map_err(|error| {
                    atm_storage::AtmError::daemon_unavailable(error.to_string())
                })?;
                {
                    let mut insert = transaction
                        .prepare(
                            "INSERT INTO attention_opportunities(
                                team, agent, opportunity_id, roster_state_revision, lane,
                                message_id, status, failed_attempts, created_at
                             ) VALUES (?1, ?2, ?3, 1, 'ephemeral', ?4, 'delivered', 0, ?5)",
                        )
                        .map_err(|error| {
                            atm_storage::AtmError::daemon_unavailable(error.to_string())
                        })?;
                    for index in 0..=ATTENTION_MAX_TERMINAL_ROWS {
                        let opportunity_id = format!("seed-{index}");
                        let message_id = format!("message-{index}");
                        insert
                            .execute(params![
                                member().team().as_str(),
                                member().agent().as_str(),
                                opportunity_id,
                                message_id,
                                now_unix_ms(),
                            ])
                            .map_err(|error| {
                                atm_storage::AtmError::daemon_unavailable(error.to_string())
                            })?;
                    }
                }
                transaction
                    .commit()
                    .map_err(|error| atm_storage::AtmError::daemon_unavailable(error.to_string()))
            })
            .expect("seed terminal rows");

        let store = backend.attention_schedule_store();
        store
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
            .expect("reserve triggers terminal-row cap pruning");

        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let terminal_count: i64 = connection
                    .query_row(
                        "SELECT COUNT(*) FROM attention_opportunities WHERE status <> 'reserved'",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|error| {
                        atm_storage::AtmError::daemon_unavailable(error.to_string())
                    })?;
                assert_eq!(terminal_count, ATTENTION_MAX_TERMINAL_ROWS);
                Ok(())
            })
            .expect("inspect capped terminal rows");
    }

    #[test]
    fn expired_reservation_reclaims_with_bounded_at_least_once_budget() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.attention_schedule_store();
        let item = AttentionItem::EphemeralMessage {
            member: member(),
            message_id: AtmMessageId::new(),
        };
        let first = store
            .reserve(AttentionReservationRequest {
                opportunity: IdleOpportunity {
                    id: IdleOpportunityId::new(),
                    member: member(),
                    roster_state_revision: RosterStateRevision::from_raw(1),
                },
                expected_cursor_revision: AttentionCursorRevision::from_raw(0),
                item: item.clone(),
            })
            .expect("reserve");
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                connection
                    .execute(
                        "UPDATE attention_opportunities SET reserved_until = 0 WHERE opportunity_id = ?1",
                        params![first.opportunity.id.to_string()],
                    )
                    .expect("expire reservation");
                Ok(())
            })
            .expect("expire reservation");
        let reclaimed = store
            .reserve(AttentionReservationRequest {
                opportunity: IdleOpportunity {
                    id: IdleOpportunityId::new(),
                    member: member(),
                    roster_state_revision: RosterStateRevision::from_raw(2),
                },
                expected_cursor_revision: AttentionCursorRevision::from_raw(1),
                item,
            })
            .expect("reclaim");
        assert_eq!(reclaimed.status, AttentionReservationStatus::Reserved);
        assert_eq!(reclaimed.failed_attempts, 1);
        assert_eq!(reclaimed.opportunity.id, first.opportunity.id);
    }

    #[test]
    fn expired_reservation_budget_exhaustion_terminalizes_without_dispatch() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.attention_schedule_store();
        assert_eq!(MAX_NUDGE_ATTEMPTS, 5);
        let item = AttentionItem::EphemeralMessage {
            member: member(),
            message_id: AtmMessageId::new(),
        };
        let first = store
            .reserve(AttentionReservationRequest {
                opportunity: IdleOpportunity {
                    id: IdleOpportunityId::new(),
                    member: member(),
                    roster_state_revision: RosterStateRevision::from_raw(1),
                },
                expected_cursor_revision: AttentionCursorRevision::from_raw(0),
                item: item.clone(),
            })
            .expect("reserve");
        for attempt in 1..=MAX_NUDGE_ATTEMPTS {
            backend
                .shared_db_for_test()
                .with_connection(|connection| {
                    connection
                        .execute(
                            "UPDATE attention_opportunities SET reserved_until = 0 WHERE opportunity_id = ?1",
                            params![first.opportunity.id.to_string()],
                        )
                        .expect("expire reservation");
                    Ok(())
                })
                .expect("expire reservation");
            let reclaimed = store
                .reserve(AttentionReservationRequest {
                    opportunity: IdleOpportunity {
                        id: IdleOpportunityId::new(),
                        member: member(),
                        roster_state_revision: RosterStateRevision::from_raw(
                            u64::from(attempt) + 1,
                        ),
                    },
                    expected_cursor_revision: AttentionCursorRevision::from_raw(1),
                    item: item.clone(),
                })
                .expect("bounded reclaim");
            assert_eq!(reclaimed.failed_attempts, attempt);
            assert_eq!(
                reclaimed.status,
                if attempt == MAX_NUDGE_ATTEMPTS {
                    AttentionReservationStatus::PermanentlyFailed
                } else {
                    AttentionReservationStatus::Reserved
                }
            );
        }
        let after_exhaustion = store
            .reserve(AttentionReservationRequest {
                opportunity: IdleOpportunity {
                    id: IdleOpportunityId::new(),
                    member: member(),
                    roster_state_revision: RosterStateRevision::from_raw(99),
                },
                expected_cursor_revision: AttentionCursorRevision::from_raw(1),
                item,
            })
            .expect("a new opportunity may observe the terminal row as complete");
        assert_eq!(
            after_exhaustion.status,
            AttentionReservationStatus::PermanentlyFailed
        );
        assert_eq!(after_exhaustion.failed_attempts, MAX_NUDGE_ATTEMPTS);
        assert_eq!(after_exhaustion.opportunity.id, first.opportunity.id);
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let status: String = connection
                    .query_row(
                        "SELECT status FROM attention_opportunities WHERE opportunity_id = ?1",
                        params![first.opportunity.id.to_string()],
                        |row| row.get(0),
                    )
                    .map_err(|error| {
                        atm_storage::AtmError::daemon_unavailable(error.to_string())
                    })?;
                assert_eq!(status, "permanently_failed");
                Ok(())
            })
            .expect("inspect terminal reservation");
    }

    #[test]
    fn stale_owner_finalize_is_rejected_after_reclaim() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.attention_schedule_store();
        let item = AttentionItem::EphemeralMessage {
            member: member(),
            message_id: AtmMessageId::new(),
        };
        let first = store
            .reserve(AttentionReservationRequest {
                opportunity: IdleOpportunity {
                    id: IdleOpportunityId::new(),
                    member: member(),
                    roster_state_revision: RosterStateRevision::from_raw(1),
                },
                expected_cursor_revision: AttentionCursorRevision::from_raw(0),
                item: item.clone(),
            })
            .expect("reserve");
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                connection
                    .execute(
                        "UPDATE attention_opportunities SET reserved_until = 0 WHERE opportunity_id = ?1",
                        params![first.opportunity.id.to_string()],
                    )
                    .expect("expire reservation");
                Ok(())
            })
            .expect("expire reservation");
        let reclaimed = store
            .reserve(AttentionReservationRequest {
                opportunity: IdleOpportunity {
                    id: IdleOpportunityId::new(),
                    member: member(),
                    roster_state_revision: RosterStateRevision::from_raw(2),
                },
                expected_cursor_revision: AttentionCursorRevision::from_raw(1),
                item,
            })
            .expect("reclaim");
        assert_eq!(reclaimed.lease_generation, first.lease_generation + 1);
        let stale = store.finalize(AttentionFinalizeRequest {
            member: member(),
            opportunity_id: first.opportunity.id,
            outcome: AttentionFinalizeOutcome::Delivered,
            lease_generation: first.lease_generation,
        });
        assert!(
            stale.is_err(),
            "stale owner must not finalize reclaimed work"
        );
        assert_eq!(
            store
                .finalize(AttentionFinalizeRequest {
                    member: member(),
                    opportunity_id: reclaimed.opportunity.id,
                    outcome: AttentionFinalizeOutcome::Delivered,
                    lease_generation: reclaimed.lease_generation,
                })
                .expect("current owner finalize")
                .status,
            AttentionReservationStatus::Delivered
        );
    }

    #[test]
    fn concurrent_expired_claims_bump_generation_once() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.attention_schedule_store();
        let item = AttentionItem::EphemeralMessage {
            member: member(),
            message_id: AtmMessageId::new(),
        };
        let first = store
            .reserve(AttentionReservationRequest {
                opportunity: IdleOpportunity {
                    id: IdleOpportunityId::new(),
                    member: member(),
                    roster_state_revision: RosterStateRevision::from_raw(1),
                },
                expected_cursor_revision: AttentionCursorRevision::from_raw(0),
                item: item.clone(),
            })
            .expect("reserve");
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                connection
                    .execute(
                        "UPDATE attention_opportunities SET reserved_until = 0 WHERE opportunity_id = ?1",
                        params![first.opportunity.id.to_string()],
                    )
                    .expect("expire reservation");
                Ok(())
            })
            .expect("expire reservation");
        let store = std::sync::Arc::new(store);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let results = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for revision in [2_u64, 3_u64] {
                let store = std::sync::Arc::clone(&store);
                let barrier = std::sync::Arc::clone(&barrier);
                let item = item.clone();
                handles.push(scope.spawn(move || {
                    barrier.wait();
                    store.reserve(AttentionReservationRequest {
                        opportunity: IdleOpportunity {
                            id: IdleOpportunityId::new(),
                            member: member(),
                            roster_state_revision: RosterStateRevision::from_raw(revision),
                        },
                        expected_cursor_revision: AttentionCursorRevision::from_raw(1),
                        item,
                    })
                }));
            }
            handles
                .into_iter()
                .map(|handle| handle.join().expect("claim thread"))
                .collect::<Vec<_>>()
        });
        let first_claim = results[0].as_ref().expect("first claim");
        let second_claim = results[1].as_ref().expect("second claim");
        assert_eq!(first_claim.opportunity.id, second_claim.opportunity.id);
        assert_eq!(first_claim.lease_generation, 2);
        assert_eq!(second_claim.lease_generation, 2);
        assert_eq!(first_claim.failed_attempts, 1);
    }

    #[test]
    fn expired_refresh_path_consumes_budget_before_returning_a_reservation() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.attention_schedule_store();
        let item = AttentionItem::PersistentTaskReminder {
            member: member(),
            task_id: "refresh-budget".parse().expect("task id"),
            attempt: AssignmentAttempt::new(1).expect("attempt"),
            assignment_message_id: AtmMessageId::new(),
        };
        let first = store
            .reserve(AttentionReservationRequest {
                opportunity: IdleOpportunity {
                    id: IdleOpportunityId::new(),
                    member: member(),
                    roster_state_revision: RosterStateRevision::from_raw(1),
                },
                expected_cursor_revision: AttentionCursorRevision::from_raw(0),
                item: item.clone(),
            })
            .expect("reserve");
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                connection
                    .execute(
                        "UPDATE attention_opportunities SET reserved_until = 0 WHERE opportunity_id = ?1",
                        params![first.opportunity.id.to_string()],
                    )
                    .expect("expire reservation");
                Ok(())
            })
            .expect("expire reservation");
        let refreshed = store
            .reserve(AttentionReservationRequest {
                opportunity: IdleOpportunity {
                    id: IdleOpportunityId::new(),
                    member: member(),
                    roster_state_revision: RosterStateRevision::from_raw(2),
                },
                expected_cursor_revision: AttentionCursorRevision::from_raw(1),
                item,
            })
            .expect("refresh reservation");
        assert_eq!(refreshed.failed_attempts, 1);
        assert_eq!(refreshed.status, AttentionReservationStatus::Reserved);
        assert_eq!(refreshed.opportunity.id, first.opportunity.id);
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
                        lease_generation: reservation.lease_generation,
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
