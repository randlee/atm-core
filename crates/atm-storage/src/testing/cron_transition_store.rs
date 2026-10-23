//! Deterministic in-memory double for [`CronTransitionStore`].

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::contract::sealed;
use crate::cron_transition_store::{
    CronCommitResult, CronCommittedNoticeRecord, CronDeliveryKey, CronLifecycleEvent,
    CronNoticeKey, CronTransitionStore, CronTransitionWrite, DeliveryReceipt, PendingCronDelivery,
};
use crate::error::AtmError;
use crate::hook_types::CronName;
use crate::types::{AgentName, TeamName};

/// Deterministic approved record double for lifecycle-intent contract tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InMemoryCommittedCronNoticeRecord {
    key: CronNoticeKey,
    team: TeamName,
    recipient: AgentName,
    name: CronName,
    event: CronLifecycleEvent,
}

impl InMemoryCommittedCronNoticeRecord {
    #[must_use]
    pub fn new(
        key: CronNoticeKey,
        team: TeamName,
        recipient: AgentName,
        name: CronName,
        event: CronLifecycleEvent,
    ) -> Self {
        Self {
            key,
            team,
            recipient,
            name,
            event,
        }
    }
}

impl sealed::Sealed for InMemoryCommittedCronNoticeRecord {}

impl CronCommittedNoticeRecord for InMemoryCommittedCronNoticeRecord {
    fn key(&self) -> CronNoticeKey {
        self.key
    }

    fn team(&self) -> &TeamName {
        &self.team
    }

    fn recipient(&self) -> &AgentName {
        &self.recipient
    }

    fn name(&self) -> &CronName {
        &self.name
    }

    fn event(&self) -> &CronLifecycleEvent {
        &self.event
    }
}

#[derive(Default)]
pub struct InMemoryCronTransitionStore {
    pub writes: Mutex<Vec<CronTransitionWrite>>,
    pub commit_results: Mutex<VecDeque<Result<CronCommitResult, AtmError>>>,
    pub pending_intents: Mutex<Vec<PendingCronDelivery>>,
    pub delivered: Mutex<Vec<(CronDeliveryKey, DeliveryReceipt)>>,
}

impl sealed::Sealed for InMemoryCronTransitionStore {}

impl CronTransitionStore for InMemoryCronTransitionStore {
    fn commit(&self, write: CronTransitionWrite) -> Result<CronCommitResult, AtmError> {
        self.writes
            .lock()
            .map_err(|_| AtmError::validation("in-memory cron transition writes lock poisoned"))?
            .push(write);
        self.commit_results
            .lock()
            .map_err(|_| AtmError::validation("in-memory cron transition results lock poisoned"))?
            .pop_front()
            .unwrap_or_else(|| Ok(CronCommitResult::Applied { intents: vec![] }))
    }

    fn pending(&self, limit: usize) -> Result<Vec<PendingCronDelivery>, AtmError> {
        Ok(self
            .pending_intents
            .lock()
            .map_err(|_| AtmError::validation("in-memory cron pending intents lock poisoned"))?
            .iter()
            .take(limit)
            .cloned()
            .collect())
    }

    fn mark_delivered(
        &self,
        key: CronDeliveryKey,
        receipt: DeliveryReceipt,
    ) -> Result<bool, AtmError> {
        self.delivered
            .lock()
            .map_err(|_| AtmError::validation("in-memory cron deliveries lock poisoned"))?
            .push((key, receipt));
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cron_schedule_store::{CronGeneration, CronScheduleId};
    use crate::cron_transition_store::{
        CronDeliveryIntent, CronDiagnosticDetail, CronNoticeIntent, DeliveryRetryCause,
    };
    use crate::hook_types::CronName;
    use crate::types::{AgentName, TeamName};

    fn reconciling_intent() -> CronDeliveryIntent {
        let record = InMemoryCommittedCronNoticeRecord::new(
            CronNoticeKey {
                schedule_id: CronScheduleId::new(7),
                generation: CronGeneration::FIRST,
                transition_seq: 3,
            },
            "ops".parse::<TeamName>().expect("team"),
            "operator".parse::<AgentName>().expect("recipient"),
            CronName::parse("nightly").expect("name"),
            CronLifecycleEvent::Stopped {
                message: CronDiagnosticDetail::parse("stop hook timeout".into())
                    .expect("diagnostic"),
            },
        );
        CronDeliveryIntent::Lifecycle(CronNoticeIntent::from_committed(&record))
    }

    #[test]
    fn unknown_receipt_replays_as_reconciliation_not_resendable_work() {
        let store = InMemoryCronTransitionStore::default();
        let intent = reconciling_intent();
        let cause = DeliveryRetryCause::parse("handoff response lost".into()).expect("cause");
        let key = match &intent {
            CronDeliveryIntent::Lifecycle(intent) => CronDeliveryKey::Lifecycle(intent.key()),
            CronDeliveryIntent::ModuleMail(intent) => intent.key,
        };

        store.pending_intents.lock().expect("pending lock").push(
            PendingCronDelivery::ReconciliationRequired {
                intent: intent.clone(),
                retry_cause: cause.clone(),
            },
        );
        assert!(
            store
                .mark_delivered(
                    key,
                    DeliveryReceipt::Unknown {
                        retry_cause: cause.clone(),
                    },
                )
                .expect("unknown receipt persists")
        );

        let replay = store.pending(10).expect("pending query");
        assert_eq!(
            replay,
            vec![PendingCronDelivery::ReconciliationRequired {
                intent,
                retry_cause: cause.clone(),
            }]
        );
        assert!(!replay[0].is_safe_to_resend());
        assert_eq!(
            store.delivered.lock().expect("delivery lock").as_slice(),
            &[(key, DeliveryReceipt::Unknown { retry_cause: cause })]
        );
    }
}
