use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use atm_core::error::AtmError;

use super::PeerTransportRuntime;

pub(super) struct DeliveryRetryState<'a> {
    pub(super) deadline: Instant,
    pub(super) terminate: &'a Arc<AtomicBool>,
    pub(super) backoff: &'a mut Duration,
    pub(super) next_attempt: &'a mut u32,
}

pub(super) enum DeliveryLoopDecision {
    Retry,
    Return(AtmError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReplayResumeSummary {
    pub(crate) delivered: usize,
    pub(crate) retained: usize,
    pub(crate) purged_expired: usize,
    pub(crate) receipt_updates: usize,
}

impl Default for PeerTransportRuntime {
    fn default() -> Self {
        Self::new(None)
    }
}
