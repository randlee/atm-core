//! Frozen cron-only hook reply contract (BK-9).
//!
//! Task hook replies remain represented by [`super::HookReply`].  BK-12 owns
//! parsing this contract; this module intentionally exposes only typed reply
//! shapes so the cron engine can compile without depending on parser work.

use atm_storage::{CronDiagnosticDetail, CronModuleOutput};

use super::HookAction;

pub const CRON_REPLY_SCHEMA_VERSION: &str = "1.1.0";
pub const CRON_OUTPUT_MAX_ITEMS: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub enum CronTickReply {
    PassThrough {
        output: Vec<CronModuleOutput>,
    },
    Handled {
        actions: Vec<HookAction>,
        output: Vec<CronModuleOutput>,
    },
    NotApplicable,
    Refused {
        message: CronDiagnosticDetail,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CronStopReply {
    pub stop: bool,
    pub reason: Option<CronDiagnosticDetail>,
    pub output: Vec<CronModuleOutput>,
}

impl CronStopReply {
    /// Validates the stop/reason invariant before a parser exposes a reply.
    pub fn new(
        stop: bool,
        reason: Option<CronDiagnosticDetail>,
        output: Vec<CronModuleOutput>,
    ) -> Result<Self, crate::error::AtmError> {
        if stop != reason.is_some() {
            return Err(crate::error::AtmError::hook_malformed(
                "cron stop reason is required exactly when stop is true",
            ));
        }
        if output.len() > CRON_OUTPUT_MAX_ITEMS {
            return Err(crate::error::AtmError::hook_malformed(&format!(
                "cron output must contain at most {CRON_OUTPUT_MAX_ITEMS} entries"
            )));
        }
        Ok(Self {
            stop,
            reason,
            output,
        })
    }
}

#[derive(Debug)]
pub struct CronHookOutcome {
    pub reply: CronTickReply,
    pub elapsed_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_requires_reason_only_for_terminal_reply() {
        assert!(CronStopReply::new(false, None, vec![]).is_ok());
        assert!(
            CronStopReply::new(
                false,
                Some(CronDiagnosticDetail::parse("x".into()).unwrap()),
                vec![]
            )
            .is_err()
        );
        assert!(CronStopReply::new(true, None, vec![]).is_err());
    }
}
