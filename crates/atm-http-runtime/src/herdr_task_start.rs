//! Durable reminder handoff after a task prompt reaches its delivery backend.

use std::sync::Arc;

use atm_core::boundary::{MemberKey, ReminderOutcome, TaskRow, TaskStore};
use atm_core::types::IsoTimestamp;

use crate::herdr_queue_wake::{HerdrQueueWakePump, herdr_request_deadline};

/// Records an emitted task prompt without changing task state.
pub(crate) async fn complete_task_handoff(
    pump: &HerdrQueueWakePump,
    task_store: &Arc<dyn TaskStore + Send + Sync>,
    member: &MemberKey,
    row: &TaskRow,
    now: IsoTimestamp,
) -> Result<TaskRow, atm_core::error::AtmError> {
    let store = Arc::clone(task_store);
    let member = member.clone();
    let task_id = row.task_id.clone();
    pump.blocking_bridge
        .run(herdr_request_deadline(), move || {
            store.record_reminder(&member, &task_id, now, ReminderOutcome::Emitted)
        })
        .await
}
