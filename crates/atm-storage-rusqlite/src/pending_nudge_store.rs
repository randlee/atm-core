use super::SqlitePendingNudgeStore;
use crate::shared_db::SharedDb;
use atm_storage::contract::{MessageKey, NudgeClaim};
use atm_storage::error::AtmError;
use atm_storage::schema::AtmMessageId;
use atm_storage::types::{IsoTimestamp, MemberKey};
use atm_storage::{MAX_NUDGE_ATTEMPTS, PendingNudgeStore};
use rusqlite::{OptionalExtension, params};
use std::sync::Arc;

impl SqlitePendingNudgeStore {
    pub(crate) fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }
}

impl atm_storage::contract::sealed::Sealed for SqlitePendingNudgeStore {}

impl PendingNudgeStore for SqlitePendingNudgeStore {
    fn mark_pending(
        &self,
        member: &MemberKey,
        msg: &AtmMessageId,
        at: IsoTimestamp,
    ) -> Result<bool, AtmError> {
        let message_key = MessageKey::from(*msg);
        let at_raw = at.to_string();
        // An IMMEDIATE transaction (rather than an ad-hoc connection) so
        // concurrent callers wait under the configured busy_timeout instead
        // of hitting shared-cache SQLITE_LOCKED during deferred lock
        // escalation; see SharedDb::with_transaction.
        self.db.with_transaction(|connection| {
            let changed = connection
                .execute(
                    "UPDATE mail_message_states
                     SET nudge_pending_at = ?4, nudge_attempts = 0, updated_at = ?4
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3
                       AND read = 0 AND deleted_at IS NULL;",
                    params![
                        member.team().as_str(),
                        member.agent().as_str(),
                        message_key.as_str(),
                        &at_raw,
                    ],
                )
                .map_err(|error| self.db.error("failed to mark pending nudge", error))?;
            Ok(changed == 1)
        })
    }

    fn claim_next_pending(&self, member: &MemberKey) -> Result<Option<NudgeClaim>, AtmError> {
        let at_raw = IsoTimestamp::now().to_string();
        // THE at-most-once mechanism: an IMMEDIATE transaction acquires the
        // write lock up front so a second concurrent claimant blocks under
        // busy_timeout and observes the first claimant's committed row
        // state, instead of racing on an ad-hoc connection.
        self.db.with_transaction(|connection| {
            connection
                .query_row(
                    "UPDATE mail_message_states
                     SET nudge_pending_at = NULL, updated_at = ?4
                     WHERE rowid = (
                         SELECT rowid FROM mail_message_states
                         WHERE team = ?1 AND agent = ?2 AND nudge_pending_at IS NOT NULL
                           AND read = 0 AND deleted_at IS NULL AND nudge_attempts < ?3
                         ORDER BY message_key ASC LIMIT 1
                     )
                     RETURNING message_key, nudge_attempts;",
                    params![
                        member.team().as_str(),
                        member.agent().as_str(),
                        MAX_NUDGE_ATTEMPTS,
                        &at_raw,
                    ],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)),
                )
                .optional()
                .map_err(|error| self.db.error("failed to claim next pending nudge", error))?
                .map(|(message_key, attempt)| {
                    let msg = MessageKey::new(message_key)?.as_atm_message_id()?;
                    Ok(NudgeClaim { msg, attempt })
                })
                .transpose()
        })
    }

    fn requeue_pending(&self, member: &MemberKey, claim: &NudgeClaim) -> Result<(), AtmError> {
        let message_key = MessageKey::from(claim.msg);
        let at_raw = IsoTimestamp::now().to_string();
        let next_attempt = claim.attempt + 1;
        self.db.with_transaction(|connection| {
            connection
                .execute(
                    "UPDATE mail_message_states
                     SET nudge_pending_at = ?4, nudge_attempts = ?5, updated_at = ?4
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3
                       AND nudge_pending_at IS NULL AND nudge_attempts = ?6;",
                    params![
                        member.team().as_str(),
                        member.agent().as_str(),
                        message_key.as_str(),
                        &at_raw,
                        next_attempt,
                        claim.attempt,
                    ],
                )
                .map_err(|error| self.db.error("failed to requeue pending nudge", error))?;
            Ok(())
        })
    }

    fn release_pending(&self, member: &MemberKey, claim: &NudgeClaim) -> Result<(), AtmError> {
        let message_key = MessageKey::from(claim.msg);
        let at_raw = IsoTimestamp::now().to_string();
        self.db.with_transaction(|connection| {
            connection
                .execute(
                    "UPDATE mail_message_states
                     SET nudge_pending_at = ?4, updated_at = ?4
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3
                       AND nudge_pending_at IS NULL AND nudge_attempts = ?5;",
                    params![
                        member.team().as_str(),
                        member.agent().as_str(),
                        message_key.as_str(),
                        &at_raw,
                        claim.attempt,
                    ],
                )
                .map_err(|error| self.db.error("failed to release pending nudge", error))?;
            Ok(())
        })
    }

    fn clear_pending_on_read(
        &self,
        member: &MemberKey,
        msg: &AtmMessageId,
    ) -> Result<(), AtmError> {
        self.clear_pending(member, msg, "failed to clear pending nudge on read")
    }

    fn clear_pending_on_handoff(
        &self,
        member: &MemberKey,
        msg: &AtmMessageId,
    ) -> Result<(), AtmError> {
        self.clear_pending(member, msg, "failed to clear pending nudge on handoff")
    }

    fn list_pending_members(&self) -> Result<Vec<MemberKey>, AtmError> {
        self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT DISTINCT team, agent FROM mail_message_states
                     WHERE nudge_pending_at IS NOT NULL AND read = 0 AND deleted_at IS NULL;",
                )
                .map_err(|error| self.db.error("failed to list pending members", error))?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| self.db.error("failed to list pending members", error))?;
            rows.into_iter()
                .map(|entry| {
                    let (team, agent) = entry.map_err(|error| {
                        self.db.error("failed to read pending member row", error)
                    })?;
                    let team = team.parse().map_err(|error| {
                        AtmError::validation(format!(
                            "invalid team in mail_message_states: {error}"
                        ))
                    })?;
                    let agent = agent.parse().map_err(|error| {
                        AtmError::validation(format!(
                            "invalid agent in mail_message_states: {error}"
                        ))
                    })?;
                    Ok(MemberKey::new(team, agent))
                })
                .collect()
        })
    }
}

impl SqlitePendingNudgeStore {
    fn clear_pending(
        &self,
        member: &MemberKey,
        msg: &AtmMessageId,
        error_message: &'static str,
    ) -> Result<(), AtmError> {
        let message_key = MessageKey::from(*msg);
        let at_raw = IsoTimestamp::now().to_string();
        self.db.with_transaction(|connection| {
            connection
                .execute(
                    "UPDATE mail_message_states
                     SET nudge_pending_at = NULL, updated_at = ?4
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                    params![
                        member.team().as_str(),
                        member.agent().as_str(),
                        message_key.as_str(),
                        &at_raw,
                    ],
                )
                .map_err(|error| self.db.error(error_message, error))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::SqliteStorageBackend;
    use atm_storage::MAX_NUDGE_ATTEMPTS;
    use atm_storage::contract::{Message, MessageKey};
    use atm_storage::schema::{AtmMessageId, MessageEnvelope};
    use atm_storage::types::{AgentName, IsoTimestamp, MemberKey, TeamName};
    use chrono::Utc;
    use serde_json::Map;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn team() -> TeamName {
        "test-team".parse().expect("team")
    }

    fn agent() -> AgentName {
        "test-agent".parse().expect("agent")
    }

    fn member() -> MemberKey {
        MemberKey::new(team(), agent())
    }

    /// Seeds (or re-upserts, on a repeat call with the same `id`) one
    /// message and its initial `mail_message_states` row through the real
    /// production write path (`MessageStore::save_message`), matching how
    /// `service_runtime_store` admits and later re-writes a message.
    fn seed_message(backend: &SqliteStorageBackend, id: AtmMessageId, read: bool) {
        let team = team();
        let agent = agent();
        let message = Message {
            team: team.clone(),
            agent: agent.clone(),
            message_key: MessageKey::from(id),
            envelope: MessageEnvelope {
                from: agent,
                source_chat_id: None,
                text: "hello".to_string(),
                timestamp: IsoTimestamp::from_datetime(Utc::now()),
                read,
                source_team: Some(team),
                destination_chat_id: None,
                summary: None,
                message_id: None,
                requires_ack: false,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                extra: Map::new(),
            },
        };
        backend
            .message_store()
            .save_message(&message)
            .expect("seed message");
    }

    /// Directly sets `deleted_at`, bypassing the trait surface: the current
    /// `MessageStore::delete_message` hard-deletes rather than soft-deletes,
    /// so a raw write is the only way to exercise the `deleted_at IS NULL`
    /// eligibility guard.
    fn mark_deleted(backend: &SqliteStorageBackend, id: AtmMessageId) {
        let message_key = MessageKey::from(id);
        let db = backend.shared_db_for_test();
        db.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE mail_message_states SET deleted_at = ?4
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                    rusqlite::params![
                        team().as_str(),
                        agent().as_str(),
                        message_key.as_str(),
                        IsoTimestamp::now().to_string(),
                    ],
                )
                .map_err(|error| db.error("mark deleted for test", error))?;
            Ok(())
        })
        .expect("mark deleted");
    }

    /// Directly sets `read = 1`, isolating the `list_pending_members`
    /// eligibility filter from the separate read-path upsert clearing
    /// behavior, which has its own dedicated test below.
    fn mark_read_without_clearing_marker(backend: &SqliteStorageBackend, id: AtmMessageId) {
        let message_key = MessageKey::from(id);
        let db = backend.shared_db_for_test();
        db.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE mail_message_states SET read = 1
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                    rusqlite::params![team().as_str(), agent().as_str(), message_key.as_str()],
                )
                .map_err(|error| db.error("mark read for test", error))?;
            Ok(())
        })
        .expect("mark read");
    }

    #[test]
    fn mark_then_claim_returns_the_marked_message() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, false);

        let store = backend.pending_nudge_store();
        let marked = store
            .mark_pending(&member, &msg, IsoTimestamp::now())
            .expect("mark pending");
        assert!(marked);

        let claim = store
            .claim_next_pending(&member)
            .expect("claim")
            .expect("claim present");
        assert_eq!(claim.msg, msg);
        assert_eq!(claim.attempt, 0);

        assert!(
            store
                .claim_next_pending(&member)
                .expect("claim again")
                .is_none(),
            "the marker must not be re-claimable once it has been claimed"
        );
    }

    #[test]
    fn mark_pending_is_conditional_on_unread_and_not_deleted() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let store = backend.pending_nudge_store();

        let read_msg = AtmMessageId::new();
        seed_message(&backend, read_msg, true);
        assert!(
            !store
                .mark_pending(&member, &read_msg, IsoTimestamp::now())
                .expect("mark read message")
        );

        let deleted_msg = AtmMessageId::new();
        seed_message(&backend, deleted_msg, false);
        mark_deleted(&backend, deleted_msg);
        assert!(
            !store
                .mark_pending(&member, &deleted_msg, IsoTimestamp::now())
                .expect("mark deleted message")
        );
    }

    #[test]
    fn two_concurrent_claims_race_to_exactly_one_winner() {
        // A real on-disk (WAL) database, not the in-memory shared-cache
        // fixture: shared-cache in-memory SQLite raises SQLITE_LOCKED for
        // cross-connection table contention, which busy_timeout does not
        // retry, unlike the file-locking WAL uses in production (and here).
        let tempdir = tempfile::tempdir().expect("temporary database directory");
        let backend = SqliteStorageBackend::new(tempdir.path().join("pending-nudge-claim-race.db"))
            .expect("backend");
        let member = member();
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, false);

        let store = backend.pending_nudge_store();
        store
            .mark_pending(&member, &msg, IsoTimestamp::now())
            .expect("mark pending");

        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let store = Arc::clone(&store);
                let member = member.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    store.claim_next_pending(&member).expect("claim")
                })
            })
            .collect();

        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().expect("claim thread"))
            .collect();
        let some_count = results.iter().filter(|claim| claim.is_some()).count();
        let none_count = results.iter().filter(|claim| claim.is_none()).count();
        assert_eq!(some_count, 1, "exactly one thread must win the claim");
        assert_eq!(
            none_count, 1,
            "the losing thread must observe no eligible row"
        );
    }

    #[test]
    fn requeue_pending_increments_attempts_until_max_then_claim_returns_none_with_marker_set() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, false);

        let store = backend.pending_nudge_store();
        store
            .mark_pending(&member, &msg, IsoTimestamp::now())
            .expect("mark pending");

        for expected_attempt in 0..MAX_NUDGE_ATTEMPTS {
            let claim = store
                .claim_next_pending(&member)
                .expect("claim")
                .expect("claim present");
            assert_eq!(claim.attempt, expected_attempt);
            store.requeue_pending(&member, &claim).expect("requeue");
        }

        assert!(
            store
                .claim_next_pending(&member)
                .expect("claim at max attempts")
                .is_none(),
            "a row at MAX_NUDGE_ATTEMPTS must become auto-retry ineligible"
        );

        // ADR-054 (f): the marker stays set and the member is still reported
        // stuck via list_pending_members even though it is unclaimable.
        assert_eq!(
            store.list_pending_members().expect("list pending"),
            vec![member]
        );
    }

    #[test]
    fn release_pending_restores_marker_without_incrementing_attempts() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, false);

        let store = backend.pending_nudge_store();
        store
            .mark_pending(&member, &msg, IsoTimestamp::now())
            .expect("mark pending");

        let claim = store
            .claim_next_pending(&member)
            .expect("claim")
            .expect("claim present");
        assert_eq!(claim.attempt, 0);
        store.release_pending(&member, &claim).expect("release");

        let reclaimed = store
            .claim_next_pending(&member)
            .expect("reclaim")
            .expect("reclaim present");
        assert_eq!(
            reclaimed.attempt, 0,
            "release_pending must leave nudge_attempts unchanged"
        );
    }

    #[test]
    fn clear_pending_on_handoff_clears_only_the_named_message() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let mut ids: Vec<AtmMessageId> = (0..3).map(|_| AtmMessageId::new()).collect();
        ids.sort_by_key(|id| MessageKey::from(*id).into_inner());
        for id in &ids {
            seed_message(&backend, *id, false);
        }

        let store = backend.pending_nudge_store();
        for id in &ids {
            store
                .mark_pending(&member, id, IsoTimestamp::now())
                .expect("mark pending");
        }

        // Hand off the newest (last-FIFO) message directly, leaving the
        // oldest still marked and still claimable.
        store
            .clear_pending_on_handoff(&member, &ids[2])
            .expect("clear handoff");

        let first_claim = store
            .claim_next_pending(&member)
            .expect("claim")
            .expect("claim present");
        assert_eq!(
            first_claim.msg, ids[0],
            "FIFO claim must still surface the oldest marked message"
        );
        let second_claim = store
            .claim_next_pending(&member)
            .expect("claim")
            .expect("claim present");
        assert_eq!(second_claim.msg, ids[1]);
        assert!(
            store.claim_next_pending(&member).expect("claim").is_none(),
            "the handed-off message must not itself be claimable"
        );
    }

    #[test]
    fn list_pending_members_excludes_read_and_deleted_rows() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let visible_msg = AtmMessageId::new();
        let read_msg = AtmMessageId::new();
        let deleted_msg = AtmMessageId::new();
        for id in [visible_msg, read_msg, deleted_msg] {
            seed_message(&backend, id, false);
        }

        let store = backend.pending_nudge_store();
        for id in [visible_msg, read_msg, deleted_msg] {
            store
                .mark_pending(&member, &id, IsoTimestamp::now())
                .expect("mark pending");
        }

        mark_read_without_clearing_marker(&backend, read_msg);
        mark_deleted(&backend, deleted_msg);

        assert_eq!(
            store.list_pending_members().expect("list pending"),
            vec![member.clone()],
            "read and deleted rows must not keep a member listed as pending"
        );

        store
            .clear_pending_on_handoff(&member, &visible_msg)
            .expect("clear visible marker");
        assert!(
            store
                .list_pending_members()
                .expect("list pending after clear")
                .is_empty()
        );
    }

    #[test]
    fn claim_next_pending_is_fifo_by_message_key_order() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let mut ids: Vec<AtmMessageId> = (0..3).map(|_| AtmMessageId::new()).collect();
        ids.sort_by_key(|id| MessageKey::from(*id).into_inner());

        // Seed and mark in reverse order to prove FIFO is driven by
        // message_key (ULID) order, not by call order.
        for id in ids.iter().rev() {
            seed_message(&backend, *id, false);
        }
        let store = backend.pending_nudge_store();
        for id in ids.iter().rev() {
            store
                .mark_pending(&member, id, IsoTimestamp::now())
                .expect("mark pending");
        }

        for expected in &ids {
            let claim = store
                .claim_next_pending(&member)
                .expect("claim")
                .expect("claim present");
            assert_eq!(&claim.msg, expected);
        }
        assert!(
            store
                .claim_next_pending(&member)
                .expect("claim exhausted")
                .is_none()
        );
    }

    #[test]
    fn read_path_upsert_clears_the_pending_marker() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, false);

        let store = backend.pending_nudge_store();
        store
            .mark_pending(&member, &msg, IsoTimestamp::now())
            .expect("mark pending");

        // The read transition is just another whole-row upsert with
        // read = true -- the same call the send/read pipeline performs
        // (service_runtime_store -> save_message ->
        // writer/ops.rs::insert_initial_message_state).
        seed_message(&backend, msg, true);

        let db = backend.shared_db_for_test();
        let (nudge_pending_at, nudge_attempts): (Option<String>, i64) = db
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT nudge_pending_at, nudge_attempts FROM mail_message_states
                         WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                        rusqlite::params![
                            member.team().as_str(),
                            member.agent().as_str(),
                            MessageKey::from(msg).as_str(),
                        ],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .map_err(|error| db.error("read nudge marker columns for test", error))
            })
            .expect("query nudge marker columns");

        assert!(
            nudge_pending_at.is_none(),
            "the read-path upsert must clear the pending marker"
        );
        assert_eq!(
            nudge_attempts, 0,
            "the read-path upsert must not disturb nudge_attempts"
        );
    }
}
