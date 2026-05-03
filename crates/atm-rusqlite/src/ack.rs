use atm_core::ack::{
    AckCommitCommand, AckCommitOutcome, AckCommitRejection, AckCommitResult, AckStore,
};
use atm_core::store::StoreError;
use atm_core::task_store::TaskStatus;
use atm_core::types::TaskId;
use rusqlite::{OptionalExtension, Transaction};

use crate::mail::{classify_message_duplicate, insert_message_row};
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
    let visibility = transaction
        .query_row(
            "SELECT read_at FROM message_visibility WHERE message_key = ?1",
            [command.source_message_key.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|error| classify_store_error(error, "failed to load visibility state"))?;
    let ack_state = transaction
        .query_row(
            "SELECT pending_ack_at, acknowledged_at FROM ack_state WHERE message_key = ?1",
            [command.source_message_key.as_str()],
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
                command.source_message_key.as_str(),
                Option::<String>::None,
                Some(command.acknowledged_at.to_string()),
                Some(command.reply_message.message_key.to_string()),
                Some(command.reply_team.to_string()),
                Some(command.reply_agent.to_string()),
            ),
        )
        .map_err(|error| classify_store_error(error, "failed to persist acknowledgement state"))?;

    let mut statement = transaction
        .prepare("SELECT task_id FROM tasks WHERE message_key = ?1 ORDER BY task_id")
        .map_err(|error| classify_store_error(error, "failed to prepare task lookup"))?;
    let rows = statement
        .query_map([command.source_message_key.as_str()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| classify_store_error(error, "failed to query linked task rows"))?;
    let mut task_ids = Vec::new();
    for row in rows {
        let raw_task_id =
            row.map_err(|error| classify_store_error(error, "failed to read linked task row"))?;
        task_ids.push(parse_required::<TaskId>(raw_task_id, "task_id")?);
    }

    transaction
        .execute(
            "UPDATE tasks SET status = ?1, acknowledged_at = ?2 WHERE message_key = ?3",
            (
                TaskStatus::Acknowledged.as_str(),
                command.acknowledged_at.to_string(),
                command.source_message_key.as_str(),
            ),
        )
        .map_err(|error| classify_store_error(error, "failed to persist task acknowledgement"))?;

    Ok(AckCommitResult::Committed(AckCommitOutcome {
        acknowledged_task_ids: task_ids,
    }))
}
