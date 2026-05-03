use atm_core::ack::{
    AckCommitCommand, AckCommitOutcome, AckCommitRejection, AckCommitResult, AckStore,
};
use atm_core::mail_store::AckStateRecord;
use atm_core::store::{MessageKey, StoreError};
use rusqlite::{OptionalExtension, Transaction};

use crate::mail::{classify_message_duplicate, insert_message_row, upsert_ack_state_row};
use crate::task::{TaskBatchAcknowledgeOutcome, acknowledge_tasks_for_message_tx};
use crate::{RusqliteStore, classify_store_error, parse_required};

impl atm_core::ack::sealed::Sealed for RusqliteStore {}

impl AckStore for RusqliteStore {
    fn commit_ack_reply(
        &self,
        command: &AckCommitCommand<'_>,
    ) -> Result<AckCommitResult, StoreError> {
        self.with_transaction(|transaction| commit_ack_reply(transaction, command))
    }
}

fn commit_ack_reply(
    transaction: &Transaction<'_>,
    command: &AckCommitCommand<'_>,
) -> Result<AckCommitResult, StoreError> {
    let Some(source_message_key) = resolve_source_message_key(transaction, command)? else {
        return Ok(AckCommitResult::Rejected(
            AckCommitRejection::MessageNotFound,
        ));
    };
    let visibility_read_at = transaction
        .query_row(
            "SELECT read_at FROM message_visibility WHERE message_key = ?1",
            [source_message_key.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|error| classify_store_error(error, "failed to load visibility state"))?;
    let source_kind = transaction
        .query_row(
            "SELECT source_kind FROM messages WHERE message_key = ?1",
            [source_message_key.as_str()],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| classify_store_error(error, "failed to load source message kind"))?;
    let ack_state = transaction
        .query_row(
            "SELECT pending_ack_at, acknowledged_at FROM ack_state WHERE message_key = ?1",
            [source_message_key.as_str()],
            |row| {
                Ok(RawAckState {
                    pending_ack_at: row.get(0)?,
                    acknowledged_at: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(|error| classify_store_error(error, "failed to load ack state"))?;

    match (visibility_read_at.flatten(), ack_state.as_ref()) {
        (
            _,
            Some(RawAckState {
                acknowledged_at: Some(_),
                ..
            }),
        ) => {
            return Ok(AckCommitResult::Rejected(
                AckCommitRejection::AlreadyAcknowledged,
            ));
        }
        (
            Some(_),
            Some(RawAckState {
                pending_ack_at: Some(_),
                acknowledged_at: None,
            }),
        ) => {}
        (Some(_), None) if source_kind == "legacy" => {}
        _ => return Ok(AckCommitResult::Rejected(AckCommitRejection::NotPending)),
    }

    match insert_message_row(transaction, command.reply_message) {
        Ok(()) => {}
        Err(error) => {
            if let Some(identity) = classify_message_duplicate(&error, command.reply_message) {
                return Ok(AckCommitResult::DuplicateReply(identity));
            }
            return Err(classify_store_error(
                error,
                "failed to persist acknowledgement reply row",
            ));
        }
    }

    upsert_ack_state_row(
        transaction,
        &AckStateRecord {
            message_key: source_message_key.clone(),
            pending_ack_at: None,
            acknowledged_at: Some(command.acknowledged_at),
            ack_reply_message_key: Some(command.reply_message.message_key.clone()),
            ack_reply_team: Some(command.reply_team.clone()),
            ack_reply_agent: Some(command.reply_agent.clone()),
        },
    )
    .map_err(|error| classify_store_error(error, "failed to persist acknowledgement state"))?;

    // `NotPending` is enforced by the Rust-side visibility/ack-state guard
    // above, so the UPSERT intentionally does not add a second SQL predicate.
    let task_ids = match acknowledge_tasks_for_message_tx(
        transaction,
        &source_message_key,
        command.acknowledged_at,
    )? {
        TaskBatchAcknowledgeOutcome::NoTasks => Vec::new(),
        TaskBatchAcknowledgeOutcome::Acknowledged(task_ids) => task_ids,
    };

    Ok(AckCommitResult::Committed(AckCommitOutcome {
        acknowledged_task_ids: task_ids,
    }))
}

#[derive(Debug)]
struct RawAckState {
    pending_ack_at: Option<String>,
    acknowledged_at: Option<String>,
}

fn resolve_source_message_key(
    transaction: &Transaction<'_>,
    command: &AckCommitCommand<'_>,
) -> Result<Option<MessageKey>, StoreError> {
    let raw = match (
        command.source_legacy_message_id.as_ref(),
        command.source_atm_message_id.as_ref(),
    ) {
        (Some(legacy_id), Some(atm_id)) => transaction
            .query_row(
                r#"
                SELECT message_key
                FROM messages
                WHERE legacy_message_id = ?1 OR atm_message_id = ?2
                ORDER BY CASE
                    WHEN atm_message_id = ?2 THEN 0
                    WHEN legacy_message_id = ?1 THEN 1
                    ELSE 2
                END
                LIMIT 1
                "#,
                (legacy_id.to_string(), atm_id.to_string()),
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| {
                classify_store_error(error, "failed to resolve acknowledgement source message")
            })?,
        (Some(legacy_id), None) => transaction
            .query_row(
                "SELECT message_key FROM messages WHERE legacy_message_id = ?1",
                [legacy_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| {
                classify_store_error(error, "failed to resolve acknowledgement source message")
            })?,
        (None, Some(atm_id)) => transaction
            .query_row(
                "SELECT message_key FROM messages WHERE atm_message_id = ?1",
                [atm_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| {
                classify_store_error(error, "failed to resolve acknowledgement source message")
            })?,
        (None, None) => None,
    };

    raw.map(|value| parse_required(value, "message_key"))
        .transpose()
}
