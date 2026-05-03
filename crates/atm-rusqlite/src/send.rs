use atm_core::mail_store::{AckStateRecord, StoredMessageRecord};
use atm_core::send::SendStore;
use atm_core::store::{InsertOutcome, StoreError};
use atm_core::task_store::TaskRecord;

use crate::mail::{classify_message_duplicate, insert_message_row, upsert_ack_state_row};
use crate::task::upsert_task_row;
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
                upsert_ack_state_row(transaction, ack_state).map_err(|error| {
                    classify_store_error(error, "failed to persist outbound ack state")
                })?;
            }

            if let Some(task) = task {
                upsert_task_row(transaction, task).map_err(|error| {
                    classify_store_error(error, "failed to persist outbound task row")
                })?;
            }

            Ok(InsertOutcome::Inserted(message.clone()))
        })
    }
}
