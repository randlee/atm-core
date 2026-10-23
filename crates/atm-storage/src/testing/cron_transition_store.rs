//! Deterministic in-memory double for [`CronTransitionStore`].

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::contract::sealed;
use crate::cron_transition_store::{
    CronCommitResult, CronDeliveryIntent, CronDeliveryKey, CronTransitionStore,
    CronTransitionWrite, DeliveryReceipt,
};
use crate::error::AtmError;

#[derive(Default)]
pub struct InMemoryCronTransitionStore {
    pub writes: Mutex<Vec<CronTransitionWrite>>,
    pub commit_results: Mutex<VecDeque<Result<CronCommitResult, AtmError>>>,
    pub pending_intents: Mutex<Vec<CronDeliveryIntent>>,
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

    fn pending(&self, limit: usize) -> Result<Vec<CronDeliveryIntent>, AtmError> {
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
