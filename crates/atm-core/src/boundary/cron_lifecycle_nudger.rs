//! Boundary for delivering persisted cron lifecycle notices.

use async_trait::async_trait;

use super::sealed;
use crate::error::AtmError;
use atm_storage::{CronNoticeIntent, DeliveryReceipt};

/// Delivers a single durable lifecycle notice after its transition commits.
///
/// The input is the persisted intent, so this boundary cannot synthesize a
/// notice or alter the creator-selected recipient. Implementations return an
/// explicit receipt and leave unknown handoff outcomes to reconciliation.
#[async_trait]
pub trait CronLifecycleNudger: sealed::Sealed + Send + Sync {
    async fn deliver(&self, intent: &CronNoticeIntent) -> Result<DeliveryReceipt, AtmError>;
}
