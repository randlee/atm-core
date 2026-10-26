//! Direct transport boundary for a cron lifecycle nudge.

use async_trait::async_trait;

use super::sealed;
use crate::types::{AgentName, TeamName};
use atm_storage::{CronNoticeKey, DeliveryReceipt};

/// Current creator transport target selected by canonical roster policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CronLifecycleNudgeTarget {
    Tmux(crate::boundary::LocalTmuxNudgeTarget),
    Herdr(crate::boundary::HerdrNudgeTarget),
    Graft(crate::boundary::GraftNudgeTarget),
}

/// Rendered, committed lifecycle notice for a direct prompt transport.
///
/// This payload is deliberately independent of post-send events and mailbox
/// records. The key lets transports deduplicate a logical notice or report an
/// unknown outcome when they cannot establish durable deduplication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CronLifecycleNudge {
    pub key: CronNoticeKey,
    pub team: TeamName,
    pub recipient: AgentName,
    pub rendered_nudge: String,
}

/// Emits one direct cron lifecycle prompt without creating ordinary mail.
#[async_trait]
pub trait CronLifecycleNudgeEmitter: sealed::Sealed + Send + Sync {
    async fn emit(
        &self,
        nudge: &CronLifecycleNudge,
        target: &CronLifecycleNudgeTarget,
    ) -> Result<DeliveryReceipt, crate::error::AtmError>;
}
