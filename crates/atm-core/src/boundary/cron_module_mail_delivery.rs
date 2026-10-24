//! Boundary for delivering persisted cron module output as ordinary mail.

use async_trait::async_trait;

use super::{CronModuleMailIntent, CronRunPermit, sealed};
use crate::error::AtmError;
use atm_storage::DeliveryReceipt;

/// Delivers a committed module-mail intent through canonical write admission.
///
/// The scheduler permit is separate from the persisted intent: it proves the
/// active schedule generation at the canonical writer, where the generation
/// fence and immutable message id are checked atomically with mailbox state.
#[async_trait]
pub trait CronModuleMailDelivery: sealed::Sealed + Send + Sync {
    async fn deliver(
        &self,
        intent: &CronModuleMailIntent,
        permit: &CronRunPermit,
    ) -> Result<DeliveryReceipt, AtmError>;
}
