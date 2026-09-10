//! SQLite ownership of the identifier-only idle-attention cursor and reservation metadata.

use std::sync::Arc;

use atm_storage::{
    AssignmentAttempt, AsyncAttentionScheduleStore, AtmError, AttentionCursor,
    AttentionFinalizeRequest, AttentionItem, AttentionLane, AttentionReservation,
    AttentionReservationRequest, AttentionReservationStatus, AttentionScheduleStore,
    IdleOpportunity, IdleOpportunityId, MemberKey, ReadDeadline, ReadLaneError,
    RosterStateRevision,
};
use rusqlite::{OptionalExtension, Row, params};

use crate::SqliteAttentionScheduleStore;
use crate::shared_db::{SharedDb, SharedDbTarget, SqliteConnection, sqlite_error};

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
    PRIMARY KEY (team, agent, opportunity_id),
    CHECK((lane = 'ephemeral' AND message_id IS NOT NULL AND task_id IS NULL
           AND assignment_attempt IS NULL AND assignment_message_id IS NULL)
       OR (lane = 'persistent_task' AND message_id IS NULL AND task_id IS NOT NULL
           AND assignment_attempt IS NOT NULL AND assignment_message_id IS NOT NULL))
);

CREATE INDEX IF NOT EXISTS attention_opportunities_member_revision
    ON attention_opportunities(team, agent, roster_state_revision);
"#;

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
        })
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
        self.db
            .read_with_deadline_async(deadline.remaining(), move |connection| {
                load_cursor(connection, &member).map_err(|error| {
                    AtmError::new(
                        atm_storage::AtmErrorCode::InternalError,
                        "failed to read attention cursor",
                    )
                    .with_cause(error)
                })
            })
            .await
            .map_err(read_lane_error)
    }

    async fn reserve(
        &self,
        request: AttentionReservationRequest,
    ) -> Result<AttentionReservation, AtmError> {
        self.db.submit_attention_reservation_async(request).await
    }

    async fn finalize(
        &self,
        request: AttentionFinalizeRequest,
    ) -> Result<AttentionReservation, AtmError> {
        self.db.submit_attention_finalize_async(request).await
    }
}

fn read_lane_error(error: AtmError) -> ReadLaneError {
    ReadLaneError::Storage {
        code: error.code(),
        message: error.message().to_owned(),
        cause: error.cause().map(str::to_owned),
    }
}

fn load_cursor(
    connection: &rusqlite::Connection,
    member: &MemberKey,
) -> rusqlite::Result<AttentionCursor> {
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
            revision: 0,
        },
        Some((lane, revision)) => AttentionCursor {
            next_lane: parse_lane(&lane).map_err(|_| rusqlite::Error::InvalidQuery)?,
            revision,
        },
    })
}

pub(crate) fn reserve_writer(
    connection: &rusqlite::Connection,
    request: AttentionReservationRequest,
) -> rusqlite::Result<AttentionReservation> {
    if let Some(existing) = load_reservation(
        connection,
        &request.opportunity.member,
        request.opportunity.id,
    )? {
        return Ok(existing);
    }
    let cursor = load_cursor(connection, &request.opportunity.member)?;
    if cursor.revision != request.expected_cursor_revision {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if request_item_member(&request.item) != &request.opportunity.member {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let (message_id, task_id, attempt, assignment_message_id) = item_columns(&request.item);
    connection.execute(
        "INSERT INTO attention_opportunities(
            team, agent, opportunity_id, roster_state_revision, lane, message_id, task_id,
            assignment_attempt, assignment_message_id, status
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'reserved')",
        params![
            request.opportunity.member.team().as_str(),
            request.opportunity.member.agent().as_str(),
            request.opportunity.id.to_string(),
            request.opportunity.roster_state_revision.get(),
            request.item.lane().as_str(),
            message_id,
            task_id,
            attempt,
            assignment_message_id,
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
    })
}

pub(crate) fn finalize_writer(
    connection: &rusqlite::Connection,
    request: AttentionFinalizeRequest,
) -> rusqlite::Result<AttentionReservation> {
    let existing = load_reservation(connection, &request.member, request.opportunity_id)?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    if existing.status != AttentionReservationStatus::Reserved && existing.status != request.status
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    connection.execute(
        "UPDATE attention_opportunities SET status = ?4
         WHERE team = ?1 AND agent = ?2 AND opportunity_id = ?3 AND status = 'reserved'",
        params![
            request.member.team().as_str(),
            request.member.agent().as_str(),
            request.opportunity_id.to_string(),
            status_name(request.status)
        ],
    )?;
    load_reservation(connection, &request.member, request.opportunity_id)?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)
}

fn load_reservation(
    connection: &rusqlite::Connection,
    member: &MemberKey,
    id: IdleOpportunityId,
) -> rusqlite::Result<Option<AttentionReservation>> {
    connection
        .query_row(
            "SELECT roster_state_revision, lane, message_id, task_id, assignment_attempt,
                assignment_message_id, status
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
fn item_columns(
    item: &AttentionItem,
) -> (Option<String>, Option<String>, Option<u32>, Option<String>) {
    match item {
        AttentionItem::EphemeralMessage { message_id, .. } => {
            (Some(message_id.to_string()), None, None, None)
        }
        AttentionItem::PersistentTaskReminder {
            task_id,
            attempt,
            assignment_message_id,
            ..
        } => (
            None,
            Some(task_id.to_string()),
            Some(attempt.get()),
            Some(assignment_message_id.to_string()),
        ),
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
    let revision: u64 = row.get(0)?;
    let lane: String = row.get(1)?;
    let status: String = row.get(6)?;
    let item = match lane.as_str() {
        "ephemeral" => AttentionItem::EphemeralMessage {
            member: member.clone(),
            message_id: row
                .get::<_, String>(2)?
                .parse()
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        },
        "persistent_task" => AttentionItem::PersistentTaskReminder {
            member: member.clone(),
            task_id: row
                .get::<_, String>(3)?
                .parse()
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            attempt: AssignmentAttempt::new(row.get(4)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            assignment_message_id: row
                .get::<_, String>(5)?
                .parse()
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        },
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    let status = match status.as_str() {
        "reserved" => AttentionReservationStatus::Reserved,
        "delivered" => AttentionReservationStatus::Delivered,
        "stale" => AttentionReservationStatus::Stale,
        "permanently_failed" => AttentionReservationStatus::PermanentlyFailed,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(AttentionReservation {
        opportunity: IdleOpportunity {
            id,
            member,
            roster_state_revision: RosterStateRevision::from_raw(revision),
        },
        item,
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SqliteStorageBackend;
    use atm_storage::{AgentName, AtmMessageId, TeamName};

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
            expected_cursor_revision: 0,
            item: item.clone(),
        };
        let first = store.reserve(request.clone()).expect("reserve");
        let replay = store.reserve(request).expect("replay");
        assert_eq!(first, replay);
        assert_eq!(
            store.load_cursor(&member()).expect("cursor"),
            AttentionCursor {
                next_lane: AttentionLane::PersistentTask,
                revision: 1,
            }
        );
        assert_eq!(
            store
                .finalize(AttentionFinalizeRequest {
                    member: member(),
                    opportunity_id: opportunity.id,
                    status: AttentionReservationStatus::Delivered,
                })
                .expect("finalize")
                .status,
            AttentionReservationStatus::Delivered
        );
    }
}
