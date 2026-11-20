use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::router_support::BoundedBlockingBridge;
use atm_core::LocalServiceRuntime;
use atm_core::boundary::{MemberKey, PendingNudgeStore};
use tokio::task::JoinHandle;

use super::{HERDR_MAX_CONSECUTIVE_RELEASES, herdr_request_deadline};

pub(crate) struct ReleasePendingOnDrop {
    store: Arc<dyn PendingNudgeStore + Send + Sync>,
    member: MemberKey,
    claim: atm_core::boundary::NudgeClaim,
    release_streaks: Arc<Mutex<HashMap<MemberKey, u32>>>,
    release_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    blocking_bridge: BoundedBlockingBridge,
    #[cfg(test)]
    release_test_gate: Option<Arc<crate::herdr_queue_wake_test_gates::BlockingGate>>,
    #[cfg(test)]
    release_test_deadline: Option<atm_core::api::RequestDeadline>,
    /// Clears the member's ephemeral Herdr wake-pending roster flag when this
    /// claim attempt concludes (success, requeue, or release), regardless of
    /// which exit path was taken. Set alongside claiming a pending nudge in
    /// [`HerdrQueueWakePump::process_candidate`]; this is the real production
    /// transition the ephemeral roster state exists to track (FTQ-AW finding
    /// 4 on PR #1240 -- previously this state had no production caller).
    service_runtime: LocalServiceRuntime,
    armed: bool,
}

impl ReleasePendingOnDrop {
    pub(crate) fn new(
        store: Arc<dyn PendingNudgeStore + Send + Sync>,
        member: MemberKey,
        claim: atm_core::boundary::NudgeClaim,
        release_streaks: Arc<Mutex<HashMap<MemberKey, u32>>>,
        service_runtime: LocalServiceRuntime,
        release_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
        blocking_bridge: BoundedBlockingBridge,
    ) -> Self {
        Self {
            store,
            member,
            claim,
            release_streaks,
            release_handles,
            blocking_bridge,
            #[cfg(test)]
            release_test_gate: None,
            #[cfg(test)]
            release_test_deadline: None,
            service_runtime,
            armed: true,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_release_test_gate(
        mut self,
        gate: Arc<crate::herdr_queue_wake_test_gates::BlockingGate>,
    ) -> Self {
        self.release_test_gate = Some(gate);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_release_test_deadline(
        mut self,
        deadline: atm_core::api::RequestDeadline,
    ) -> Self {
        self.release_test_deadline = Some(deadline);
        self
    }

    pub(crate) async fn release_without_input(&mut self) {
        if !self.armed {
            return;
        }
        let should_requeue = self.claim_release_action();
        self.armed = false;
        let store = Arc::clone(&self.store);
        let member = self.member.clone();
        let claim = self.claim.clone();
        if let Err(error) = self
            .blocking_bridge
            .run(herdr_request_deadline(), move || {
                if should_requeue {
                    store.requeue_pending(&member, &claim)
                } else {
                    store.release_pending(&member, &claim)
                }
            })
            .await
        {
            tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "queue_claim_release",
                outcome = "failed",
                error = %error,
                member = %self.member,
                "failed to resolve Herdr queue claim"
            );
        }
    }

    pub(crate) async fn requeue(&mut self) {
        if !self.armed {
            return;
        }
        self.release_streaks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.member);
        self.armed = false;
        let store = Arc::clone(&self.store);
        let member = self.member.clone();
        let claim = self.claim.clone();
        if let Err(error) = self
            .blocking_bridge
            .run(herdr_request_deadline(), move || {
                store.requeue_pending(&member, &claim)
            })
            .await
        {
            tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "queue_claim_requeue",
                outcome = "failed",
                error = %error,
                member = %self.member,
                "failed to requeue Herdr queue claim"
            );
        }
    }

    fn claim_release_action(&self) -> bool {
        let mut streaks = self
            .release_streaks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let streak = streaks.entry(self.member.clone()).or_default();
        if *streak >= HERDR_MAX_CONSECUTIVE_RELEASES {
            streaks.remove(&self.member);
            true
        } else {
            *streak = streak.saturating_add(1);
            false
        }
    }

    fn release_in_drop(&mut self) {
        if !self.armed {
            return;
        }
        let should_requeue = self.claim_release_action();
        self.armed = false;
        let store = Arc::clone(&self.store);
        let member = self.member.clone();
        let claim = self.claim.clone();
        #[cfg(test)]
        let test_gate = self.release_test_gate.clone();
        #[cfg(test)]
        let signal_gate = test_gate.clone();
        #[cfg(test)]
        let test_deadline = self.release_test_deadline;
        let release = move || {
            #[cfg(test)]
            if let Some(gate) = test_gate {
                gate.enter_and_wait();
            }
            if should_requeue {
                store.requeue_pending(&member, &claim)
            } else {
                store.release_pending(&member, &claim)
            }
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let bridge = self.blocking_bridge.clone();
            let log_member = self.member.clone();
            let release_handle = handle.spawn(async move {
                #[cfg(test)]
                let deadline = test_deadline.unwrap_or_else(herdr_request_deadline);
                #[cfg(not(test))]
                let deadline = herdr_request_deadline();
                let outcome = bridge.run(deadline, release).await;
                #[cfg(test)]
                if outcome.is_err()
                    && let Some(gate) = signal_gate
                {
                    gate.fail_before_entry();
                }
                if let Err(error) = outcome {
                    tracing::warn!(
                        subsystem = "herdr_queue_wake",
                        action = "queue_claim_release",
                        outcome = "failed",
                        error = %error,
                        member = %log_member,
                        "failed to resolve Herdr queue claim during drop"
                    );
                }
            });
            let mut release_handles = self
                .release_handles
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            release_handles.retain(|handle| !handle.is_finished());
            release_handles.push(release_handle);
        } else {
            if let Err(error) = release() {
                tracing::warn!(
                    subsystem = "herdr_queue_wake",
                    action = "queue_claim_release",
                    outcome = "failed",
                    error = %error,
                    member = %self.member,
                    "failed to resolve Herdr queue claim during drop"
                );
            }
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ReleasePendingOnDrop {
    fn drop(&mut self) {
        self.release_in_drop();
        self.service_runtime.set_roster_herdr_wake_pending(
            self.member.team(),
            self.member.agent(),
            false,
        );
    }
}
