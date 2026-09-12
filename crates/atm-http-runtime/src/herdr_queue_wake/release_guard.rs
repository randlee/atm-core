use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use atm_core::LocalServiceRuntime;
use atm_core::boundary::{MemberKey, PendingNudgeStore};
use tokio::task::JoinHandle;

use super::{HERDR_MAX_CONSECUTIVE_RELEASES, run_blocking};

pub(crate) struct ReleasePendingOnDrop {
    store: Arc<dyn PendingNudgeStore + Send + Sync>,
    member: MemberKey,
    claim: atm_core::boundary::NudgeClaim,
    release_streaks: Arc<Mutex<HashMap<MemberKey, u32>>>,
    release_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
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
    ) -> Self {
        Self {
            store,
            member,
            claim,
            release_streaks,
            release_handles,
            service_runtime,
            armed: true,
        }
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
        if let Err(error) = run_blocking(move || {
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
        if let Err(error) = run_blocking(move || store.requeue_pending(&member, &claim)).await {
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
        let release = move || {
            let result = if should_requeue {
                store.requeue_pending(&member, &claim)
            } else {
                store.release_pending(&member, &claim)
            };
            if let Err(error) = result {
                tracing::warn!(
                    subsystem = "herdr_queue_wake",
                    action = "queue_claim_release",
                    outcome = "failed",
                    error = %error,
                    member = %member,
                    "failed to resolve Herdr queue claim during drop"
                );
            }
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let release_handle = handle.spawn_blocking(release);
            let mut release_handles = self
                .release_handles
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            release_handles.retain(|handle| !handle.is_finished());
            release_handles.push(release_handle);
        } else {
            release();
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
