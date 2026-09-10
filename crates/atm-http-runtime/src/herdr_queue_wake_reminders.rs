//! Task-reminder and escalation pass for the Herdr queue wake pump.

use std::collections::HashSet;
use std::sync::Arc;

use atm_core::boundary::{AsyncTaskLedgerReader, MemberKey};
use atm_core::error::AtmError;
use atm_core::types::IsoTimestamp;

use super::{HerdrQueueWakePump, HerdrQueueWakeStats, TaskCandidate};

impl HerdrQueueWakePump {
    pub(super) async fn remind_open_tasks(
        &self,
        candidates: Vec<TaskCandidate>,
        list_complete: bool,
        stats: &mut HerdrQueueWakeStats,
    ) {
        let reader = match self.service_runtime.async_task_ledger_reader() {
            Ok(reader) => Some(reader),
            Err(error) => {
                stats.task_step_skipped = true;
                self.note_task_step_availability(false, Some(&error));
                None
            }
        };
        let task_store = match self.service_runtime.task_store() {
            Ok(store) => Some(store),
            Err(error) => {
                stats.task_step_skipped = true;
                self.note_task_step_availability(false, Some(&error));
                None
            }
        };
        if reader.is_some() && task_store.is_some() {
            self.note_task_step_availability(true, None);
        }
        let now = (self.clock)();
        let blocked_members: HashSet<_> = candidates
            .iter()
            .filter(|candidate| candidate.blocked)
            .map(|candidate| candidate.member.key.clone())
            .collect();
        if list_complete {
            self.escalation_state.prune_blocked(&blocked_members);
        }

        self.escalate_blocked(
            &blocked_members,
            reader.as_ref().map(Arc::as_ref),
            task_store.as_ref(),
            now,
            stats,
        )
        .await;
    }

    async fn escalate_blocked(
        &self,
        blocked_members: &HashSet<MemberKey>,
        reader: Option<&(dyn AsyncTaskLedgerReader + Send + Sync)>,
        task_store: Option<&Arc<dyn atm_core::boundary::TaskStore + Send + Sync>>,
        now: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
    ) {
        crate::herdr_queue_wake_escalation::escalate_blocked(
            self,
            blocked_members,
            reader,
            task_store,
            now,
            stats,
        )
        .await;
    }

    fn note_task_step_availability(&self, available: bool, error: Option<&AtmError>) {
        let mut previous = self
            .task_step_available
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *previous == Some(available) {
            return;
        }
        *previous = Some(available);
        if available {
            tracing::info!(
                subsystem = "herdr_queue_wake",
                action = "task_store_resolve",
                outcome = "available",
                "Herdr task reminder step available"
            );
        } else {
            tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "task_store_resolve",
                outcome = "unavailable",
                error = ?error,
                "Herdr task reminder step skipped: task store unavailable"
            );
        }
    }
}
