use super::SqlitePendingNudgeStore;
use crate::shared_db::SharedDb;
use atm_storage::contract::{MessageKey, NudgeClaim};
use atm_storage::error::AtmError;
use atm_storage::schema::AtmMessageId;
use atm_storage::types::{IsoTimestamp, MemberKey};
use atm_storage::{MAX_NUDGE_ATTEMPTS, PendingNudgeStore};
use rusqlite::{OptionalExtension, params};
use std::sync::Arc;

/// A queue item is open while unread, or while an acknowledgement is owed.
const OPEN_ITEM_SQL: &str =
    "(read = 0 OR (pending_ack_at IS NOT NULL AND acknowledged_at IS NULL)) AND deleted_at IS NULL";

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
        let now = IsoTimestamp::now();
        let at_raw = now.to_string();
        let next_due = IsoTimestamp::from_datetime(
            now.into_inner()
                + chrono::Duration::milliseconds(atm_storage::TASK_REMINDER_INTERVAL_MS),
        )
        .to_string();
        // THE at-most-once mechanism: an IMMEDIATE transaction acquires the
        // write lock up front so a second concurrent claimant blocks under
        // busy_timeout and observes the first claimant's committed row
        // state, instead of racing on an ad-hoc connection.
        self.db.with_transaction(|connection| {
            connection
                .query_row(
                    &format!(
                        "UPDATE mail_message_states
                     SET nudge_pending_at = ?5, updated_at = ?4
                     WHERE rowid = (
                         SELECT rowid FROM mail_message_states
                         WHERE team = ?1 AND agent = ?2 AND nudge_pending_at IS NOT NULL
                           AND nudge_pending_at <= ?4 AND {OPEN_ITEM_SQL} AND nudge_attempts < ?3
                         ORDER BY message_key ASC LIMIT 1
                     )
                     RETURNING message_key, nudge_attempts;"
                    ),
                    params![
                        member.team().as_str(),
                        member.agent().as_str(),
                        MAX_NUDGE_ATTEMPTS,
                        &at_raw,
                        &next_due,
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
        let now = IsoTimestamp::now();
        let at_raw = now.to_string();
        let next_due = IsoTimestamp::from_datetime(
            now.into_inner()
                + chrono::Duration::milliseconds(atm_storage::TASK_REMINDER_INTERVAL_MS),
        )
        .to_string();
        let next_attempt = claim.attempt + 1;
        self.db.with_transaction(|connection| {
            connection
                .execute(
                    &format!("UPDATE mail_message_states
                     SET nudge_pending_at = CASE WHEN ?5 >= ?7 THEN ?8 ELSE ?4 END,
                         nudge_attempts = CASE WHEN ?5 >= ?7 THEN 0 ELSE ?5 END, updated_at = ?4
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3
                       AND nudge_pending_at IS NOT NULL AND nudge_attempts = ?6 AND {OPEN_ITEM_SQL};"),
                    params![
                        member.team().as_str(),
                        member.agent().as_str(),
                        message_key.as_str(),
                        &at_raw,
                        next_attempt,
                        claim.attempt,
                        MAX_NUDGE_ATTEMPTS,
                        &next_due,
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
                    &format!("UPDATE mail_message_states
                     SET nudge_pending_at = ?4, updated_at = ?4
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3
                       AND nudge_pending_at IS NOT NULL AND nudge_attempts = ?5 AND {OPEN_ITEM_SQL};"),
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

    fn rearm_pending_after_handoff(
        &self,
        member: &MemberKey,
        msg: &AtmMessageId,
        next_due: IsoTimestamp,
    ) -> Result<(), AtmError> {
        let message_key = MessageKey::from(*msg);
        let next_due_raw = next_due.to_string();
        let now_raw = IsoTimestamp::now().to_string();
        self.db.with_transaction(|connection| {
            connection
                .execute(
                    &format!(
                        "UPDATE mail_message_states
                     SET nudge_pending_at = ?4, updated_at = ?5
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3 AND {OPEN_ITEM_SQL};"
                    ),
                    params![
                        member.team().as_str(),
                        member.agent().as_str(),
                        message_key.as_str(),
                        &next_due_raw,
                        &now_raw,
                    ],
                )
                .map_err(|error| {
                    self.db
                        .error("failed to rearm pending nudge after handoff", error)
                })?;
            Ok(())
        })
    }

    fn list_pending_members(&self) -> Result<Vec<MemberKey>, AtmError> {
        let db = Arc::clone(&self.db);
        self.db.read(move |connection| {
            let mut statement = connection
                .prepare(&format!(
                    "SELECT DISTINCT team, agent FROM mail_message_states
                     WHERE nudge_pending_at IS NOT NULL AND {OPEN_ITEM_SQL};"
                ))
                .map_err(|error| db.error("failed to list pending members", error))?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| db.error("failed to list pending members", error))?;
            rows.into_iter()
                .map(|entry| {
                    let (team, agent) = entry
                        .map_err(|error| db.error("failed to read pending member row", error))?;
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

#[cfg(test)]
mod tests {
    use crate::SqliteStorageBackend;
    use atm_storage::MAX_NUDGE_ATTEMPTS;
    use atm_storage::contract::{MailboxScope, Message, MessageKey};
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
                placement: None,
                task_op: None,
                task_complete: None,
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

    fn state_row(
        backend: &SqliteStorageBackend,
        id: AtmMessageId,
    ) -> (i64, Option<String>, Option<String>, Option<String>, i64) {
        let db = backend.shared_db_for_test();
        db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT read, nudge_pending_at, pending_ack_at, acknowledged_at, nudge_attempts
                     FROM mail_message_states
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                    rusqlite::params![
                        team().as_str(),
                        agent().as_str(),
                        MessageKey::from(id).as_str(),
                    ],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .map_err(|error| db.error("read pending state for test", error))
        })
        .expect("pending state row")
    }

    fn mark_ack_pending(backend: &SqliteStorageBackend, id: AtmMessageId) {
        let db = backend.shared_db_for_test();
        db.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE mail_message_states
                     SET pending_ack_at = ?4, acknowledged_at = NULL
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                    rusqlite::params![
                        team().as_str(),
                        agent().as_str(),
                        MessageKey::from(id).as_str(),
                        IsoTimestamp::now().to_string(),
                    ],
                )
                .map_err(|error| db.error("mark ack pending for test", error))
        })
        .expect("mark ack pending");
    }

    fn set_marker(backend: &SqliteStorageBackend, id: AtmMessageId, at: IsoTimestamp) {
        let db = backend.shared_db_for_test();
        db.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE mail_message_states SET nudge_pending_at = ?4
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                    rusqlite::params![
                        team().as_str(),
                        agent().as_str(),
                        MessageKey::from(id).as_str(),
                        at.to_string(),
                    ],
                )
                .map_err(|error| db.error("set pending marker for test", error))
        })
        .expect("set pending marker");
    }

    #[test]
    fn claim_skips_items_not_yet_due() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, false);
        let store = backend.pending_nudge_store();
        set_marker(
            &backend,
            msg,
            IsoTimestamp::from_datetime(Utc::now() + chrono::Duration::seconds(30)),
        );
        assert!(
            store
                .claim_next_pending(&member)
                .expect("early claim")
                .is_none()
        );
        set_marker(&backend, msg, IsoTimestamp::now());
        assert_eq!(
            store
                .claim_next_pending(&member)
                .expect("due claim")
                .map(|claim| claim.msg),
            Some(msg)
        );
        assert!(
            store
                .claim_next_pending(&member)
                .expect("leased claim")
                .is_none()
        );
    }

    #[test]
    fn restart_after_claim_reexposes_unread_item() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, false);
        let store = backend.pending_nudge_store();
        store
            .mark_pending(&member, &msg, IsoTimestamp::now())
            .expect("mark");
        let claim = store
            .claim_next_pending(&member)
            .expect("claim")
            .expect("claimed");
        assert_eq!(claim.attempt, 0);
        set_marker(&backend, msg, IsoTimestamp::now());
        assert_eq!(
            store.list_pending_members().expect("list"),
            vec![member.clone()]
        );
        let reclaimed = store
            .claim_next_pending(&member)
            .expect("reclaim")
            .expect("reclaimed");
        assert_eq!(reclaimed.msg, msg);
        assert_eq!(reclaimed.attempt, 0);
    }

    #[test]
    fn rearm_after_handoff_sets_next_due_and_keeps_attempts() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, false);
        let store = backend.pending_nudge_store();
        store
            .mark_pending(&member, &msg, IsoTimestamp::now())
            .expect("mark");
        let claim = store
            .claim_next_pending(&member)
            .expect("claim")
            .expect("claimed");
        store.requeue_pending(&member, &claim).expect("retry");
        let retry = store
            .claim_next_pending(&member)
            .expect("retry claim")
            .expect("retry claimed");
        let next_due = IsoTimestamp::from_datetime(Utc::now() + chrono::Duration::seconds(60));
        store
            .rearm_pending_after_handoff(&member, &msg, next_due)
            .expect("rearm");
        let (_, marker, _, _, attempts) = state_row(&backend, msg);
        assert_eq!(marker, Some(next_due.to_string()));
        assert_eq!(attempts, i64::from(retry.attempt));
    }

    #[tokio::test]
    async fn rearm_after_handoff_is_noop_when_read_meanwhile() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, false);
        let store = backend.pending_nudge_store();
        store
            .mark_pending(&member, &msg, IsoTimestamp::now())
            .expect("mark");
        let _ = store
            .claim_next_pending(&member)
            .expect("claim")
            .expect("claimed");
        backend
            .async_message_store()
            .apply_read_display_state_async(
                MailboxScope::new(team(), agent()),
                vec![MessageKey::from(msg)],
                None,
            )
            .await
            .expect("read");
        store
            .rearm_pending_after_handoff(&member, &msg, IsoTimestamp::now())
            .expect("rearm");
        assert!(state_row(&backend, msg).1.is_none());
    }

    #[tokio::test]
    async fn mark_message_read_closes_item_without_ack_requirement() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, false);
        backend
            .pending_nudge_store()
            .mark_pending(&member, &msg, IsoTimestamp::now())
            .expect("mark");
        backend
            .async_message_store()
            .apply_read_display_state_async(
                MailboxScope::new(team(), agent()),
                vec![MessageKey::from(msg)],
                None,
            )
            .await
            .expect("read");
        assert!(state_row(&backend, msg).1.is_none());
    }

    #[tokio::test]
    async fn mark_message_read_rearms_item_when_ack_owed() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, false);
        mark_ack_pending(&backend, msg);
        backend
            .pending_nudge_store()
            .mark_pending(&member, &msg, IsoTimestamp::now())
            .expect("mark");
        backend
            .async_message_store()
            .apply_read_display_state_async(
                MailboxScope::new(team(), agent()),
                vec![MessageKey::from(msg)],
                None,
            )
            .await
            .expect("read");
        let (read, marker, pending_ack, acknowledged, _) = state_row(&backend, msg);
        assert_eq!(read, 1);
        assert!(marker.is_some());
        assert!(pending_ack.is_some());
        assert!(acknowledged.is_none());
    }

    #[tokio::test]
    async fn mark_message_read_leaves_unmarked_message_null() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, false);
        backend
            .async_message_store()
            .apply_read_display_state_async(
                MailboxScope::new(team(), agent()),
                vec![MessageKey::from(msg)],
                None,
            )
            .await
            .expect("read");
        assert!(state_row(&backend, msg).1.is_none());
    }

    #[test]
    fn claim_selects_read_but_unacked_item() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, false);
        mark_ack_pending(&backend, msg);
        backend
            .pending_nudge_store()
            .mark_pending(&member, &msg, IsoTimestamp::now())
            .expect("mark");
        mark_read_without_clearing_marker(&backend, msg);
        assert_eq!(
            backend
                .pending_nudge_store()
                .claim_next_pending(&member)
                .expect("claim")
                .expect("claim")
                .msg,
            msg
        );
    }

    #[test]
    fn list_pending_members_includes_read_but_unacked_member() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, false);
        mark_ack_pending(&backend, msg);
        backend
            .pending_nudge_store()
            .mark_pending(&member, &msg, IsoTimestamp::now())
            .expect("mark");
        mark_read_without_clearing_marker(&backend, msg);
        assert_eq!(
            backend
                .pending_nudge_store()
                .list_pending_members()
                .expect("list"),
            vec![member]
        );
    }

    #[test]
    fn requeue_and_release_keep_closed_items_closed() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        let msg = AtmMessageId::new();
        seed_message(&backend, msg, true);
        set_marker(&backend, msg, IsoTimestamp::now());
        let claim = atm_storage::NudgeClaim { msg, attempt: 0 };
        let store = backend.pending_nudge_store();
        store
            .requeue_pending(&member, &claim)
            .expect("closed requeue");
        store
            .release_pending(&member, &claim)
            .expect("closed release");
        assert!(
            store
                .list_pending_members()
                .expect("closed list")
                .is_empty()
        );
    }

    #[test]
    fn immediate_send_never_carries_marker() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let member = member();
        for _ in 0..20 {
            let msg = AtmMessageId::new();
            seed_message(&backend, msg, false);
            assert!(
                backend
                    .pending_nudge_store()
                    .claim_next_pending(&member)
                    .expect("claim")
                    .is_none()
            );
        }
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
    fn requeue_at_max_attempts_backs_off_to_interval_and_resets() {
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
        let (_, marker, _, _, attempts) = state_row(&backend, msg);
        assert!(marker.is_some());
        assert_eq!(attempts, 0);
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
    fn rearm_after_handoff_keeps_only_open_items_pending() {
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
        let next_due = IsoTimestamp::from_datetime(
            Utc::now() + chrono::Duration::milliseconds(atm_storage::TASK_REMINDER_INTERVAL_MS),
        );
        store
            .rearm_pending_after_handoff(&member, &ids[2], next_due)
            .expect("rearm handoff");

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
            "the handed-off message must wait for its next due time"
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

        assert_eq!(
            store
                .list_pending_members()
                .expect("list pending after rearm"),
            vec![member]
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
    fn ack_clears_marker_via_existing_upsert() {
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
