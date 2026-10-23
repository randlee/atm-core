//! Deterministic in-memory double for [`CronTransitionStore`].

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::contract::sealed;
use crate::cron_transition_store::{
    CronCommitResult, CronDeliveryKey, CronTransitionStore, CronTransitionWrite, DeliveryReceipt,
    PendingCronDelivery,
};
use crate::error::AtmError;

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
        CronDeliveryIntent, CronDiagnosticDetail, CronLifecycleEvent, CronNoticeIntent,
        CronNoticeKey, DeliveryRetryCause,
    };
    use crate::hook_types::CronName;
    use crate::types::{AgentName, TeamName};

    fn reconciling_intent() -> CronDeliveryIntent {
        CronDeliveryIntent::Lifecycle(CronNoticeIntent {
            key: CronNoticeKey {
                schedule_id: CronScheduleId::new(7),
                generation: CronGeneration::FIRST,
                transition_seq: 3,
            },
            team: "ops".parse::<TeamName>().expect("team"),
            recipient: "operator".parse::<AgentName>().expect("recipient"),
            name: CronName::parse("nightly").expect("name"),
            event: CronLifecycleEvent::Stopped {
                message: CronDiagnosticDetail::parse("stop hook timeout".into())
                    .expect("diagnostic"),
            },
        })
    }

    #[test]
    fn unknown_receipt_replays_as_reconciliation_not_resendable_work() {
        let store = InMemoryCronTransitionStore::default();
        let intent = reconciling_intent();
        let cause = DeliveryRetryCause::parse("handoff response lost".into()).expect("cause");
        let key = match &intent {
            CronDeliveryIntent::Lifecycle(intent) => CronDeliveryKey::Lifecycle(intent.key),
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
