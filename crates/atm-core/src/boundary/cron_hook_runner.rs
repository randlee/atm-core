use async_trait::async_trait;

use super::sealed;
use crate::api::RequestDeadline;
use crate::error::AtmError;
use crate::hook::{CronHookOutcome, CronStopReply, HookRequest, HookTarget};

/// Cron-only daemon runner.  It intentionally does not widen `HookRunner`,
/// preserving the exact task-hook reply grammar.
#[async_trait]
pub trait CronHookRunner: sealed::Sealed + Send + Sync {
    async fn run_cron(
        &self,
        target: &HookTarget,
        request: &HookRequest,
        deadline: RequestDeadline,
    ) -> Result<CronHookOutcome, AtmError>;

    async fn stop_cron(
        &self,
        target: &HookTarget,
        request: &HookRequest,
        deadline: RequestDeadline,
    ) -> Result<CronStopReply, AtmError>;
}
