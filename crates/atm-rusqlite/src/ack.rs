use atm_core::ack::{
    AckCommitCommand, AckCommitOutcome, AckCommitRejection, AckCommitResult, AckStore,
};
use atm_core::store::{MessageKey, StoreError};
use rusqlite::{OptionalExtension, Transaction};

use crate::mail::{classify_message_duplicate, insert_message_row};
use crate::task::acknowledge_tasks_for_message_tx;
use crate::{RusqliteStore, classify_store_error, parse_required};

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
    let visibility = transaction
        .query_row(
            "SELECT read_at FROM message_visibility WHERE message_key = ?1",
            [source_message_key.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|error| classify_store_error(error, "failed to load visibility state"))?;
    let ack_state = transaction
        .query_row(
            "SELECT pending_ack_at, acknowledged_at FROM ack_state WHERE message_key = ?1",
            [source_message_key.as_str()],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()
        .map_err(|error| classify_store_error(error, "failed to load ack state"))?;

    match (visibility, ack_state) {
        (_, Some((_, Some(_)))) => {
            return Ok(AckCommitResult::Rejected(
                AckCommitRejection::AlreadyAcknowledged,
            ));
        }
        (Some(Some(_)), Some((Some(_), None))) => {}
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

    transaction
        .execute(
            r#"
            INSERT INTO ack_state (
                message_key,
                pending_ack_at,
                acknowledged_at,
                ack_reply_message_key,
                ack_reply_team,
                ack_reply_agent
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(message_key) DO UPDATE SET
                pending_ack_at = excluded.pending_ack_at,
                acknowledged_at = excluded.acknowledged_at,
                ack_reply_message_key = excluded.ack_reply_message_key,
                ack_reply_team = excluded.ack_reply_team,
                ack_reply_agent = excluded.ack_reply_agent
            "#,
            (
                source_message_key.as_str(),
                Option::<String>::None,
                Some(command.acknowledged_at.to_string()),
                Some(command.reply_message.message_key.to_string()),
                Some(command.reply_team.to_string()),
                Some(command.reply_agent.to_string()),
            ),
        )
        .map_err(|error| classify_store_error(error, "failed to persist acknowledgement state"))?;

    // `NotPending` is enforced by the Rust-side visibility/ack-state guard
    // above, so the UPSERT intentionally does not add a second SQL predicate.
    let task_ids = acknowledge_tasks_for_message_tx(
        transaction,
        &source_message_key,
        command.acknowledged_at,
    )?;

    Ok(AckCommitResult::Committed(AckCommitOutcome {
        acknowledged_task_ids: task_ids,
    }))
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
