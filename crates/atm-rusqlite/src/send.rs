use atm_core::mail_store::{AckStateRecord, StoredMessageRecord};
use atm_core::send::SendStore;
use atm_core::store::{InsertOutcome, StoreError};
use atm_core::task_store::TaskRecord;

use crate::mail::{classify_message_duplicate, insert_message_row};
use crate::{RusqliteStore, classify_store_error};

impl SendStore for RusqliteStore {
    fn commit_outbound_message(
        &self,
        message: &StoredMessageRecord,
        ack_state: Option<&AckStateRecord>,
        task: Option<&TaskRecord>,
    ) -> Result<InsertOutcome<StoredMessageRecord>, StoreError> {
        self.with_transaction(|transaction| {
            match insert_message_row(transaction, message) {
                Ok(()) => {}
                Err(error) => {
                    if let Some(identity) = classify_message_duplicate(&error, message) {
                        return Ok(InsertOutcome::Duplicate(identity));
                    }
                    return Err(classify_store_error(
                        error,
                        "failed to insert outbound message row",
                    ));
                }
            }

            if let Some(ack_state) = ack_state {
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
                            ack_state.message_key.as_str(),
                            ack_state.pending_ack_at.as_ref().map(ToString::to_string),
                            ack_state.acknowledged_at.as_ref().map(ToString::to_string),
                            ack_state
                                .ack_reply_message_key
                                .as_ref()
                                .map(ToString::to_string),
                            ack_state.ack_reply_team.as_ref().map(ToString::to_string),
                            ack_state.ack_reply_agent.as_ref().map(ToString::to_string),
                        ),
                    )
                    .map_err(|error| {
                        classify_store_error(error, "failed to persist outbound ack state")
                    })?;
            }

            if let Some(task) = task {
                transaction
                    .execute(
                        r#"
                        INSERT INTO tasks (
                            task_id,
                            message_key,
                            status,
                            created_at,
                            acknowledged_at,
                            metadata_json
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                        ON CONFLICT(task_id) DO UPDATE SET
                            message_key = excluded.message_key,
                            status = excluded.status,
                            created_at = excluded.created_at,
                            acknowledged_at = excluded.acknowledged_at,
                            metadata_json = excluded.metadata_json
                        "#,
                        (
                            task.task_id.as_str(),
                            task.message_key.as_str(),
                            task.status.as_str(),
                            task.created_at.to_string(),
                            task.acknowledged_at.as_ref().map(ToString::to_string),
                            task.metadata_json.as_deref(),
                        ),
                    )
                    .map_err(|error| {
                        classify_store_error(error, "failed to persist outbound task row")
                    })?;
            }

            Ok(InsertOutcome::Inserted(message.clone()))
        })
    }
}
