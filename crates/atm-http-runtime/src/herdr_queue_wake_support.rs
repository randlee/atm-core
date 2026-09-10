//! Pure helpers shared by the Herdr queue wake pump and its tests.

use atm_core::boundary::MemberKey;
use atm_core::error::AtmError;
use atm_core::protocol::RuntimeMemberState;
use atm_herdr::HerdrAgentStatus;
use std::sync::Arc;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::HerdrQueueWakePump;
use atm_core::delivery_channel::HerdrSession;

impl crate::RuntimeMaintenance for HerdrQueueWakePump {
    fn start(&self, shutdown: watch::Receiver<()>) -> JoinHandle<()> {
        Arc::new(self.clone()).start(shutdown)
    }
}

pub(super) fn log_herdr_list_failure(
    session: &Option<HerdrSession>,
    error: &atm_herdr::HerdrError,
) {
    let code = AtmError::from(error.clone()).code();
    tracing::warn!(
        subsystem = "herdr_queue_wake",
        action = "herdr_list",
        outcome = "failed",
        session = ?session,
        code = %code,
        failure_class = error.diagnostic_name(),
        error = ?error,
        "Herdr queue wake list failed: {code}"
    );
}

pub(super) fn member_order(left: &MemberKey, right: &MemberKey) -> std::cmp::Ordering {
    left.team()
        .as_str()
        .cmp(right.team().as_str())
        .then_with(|| left.agent().as_str().cmp(right.agent().as_str()))
}

pub(super) fn runtime_state(status: HerdrAgentStatus) -> RuntimeMemberState {
    match status {
        HerdrAgentStatus::Idle | HerdrAgentStatus::Done => RuntimeMemberState::Idle,
        HerdrAgentStatus::Working => RuntimeMemberState::Active,
        HerdrAgentStatus::Blocked => RuntimeMemberState::Blocked,
        HerdrAgentStatus::Unknown => RuntimeMemberState::Unknown,
    }
}
