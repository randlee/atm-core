//! Daemon-lease lifecycle for a bound graft receiver.
//!
//! This submodule of [`super`] owns the narrow interface the receive loop
//! uses to talk to a daemon's graft-receiver lease store
//! ([`GraftReceiverLeaseClient`]), the receiver-owned lease state machine
//! that drives announce/refresh/unregister over that interface
//! ([`RegisteredGraftReceiver`]), and the backoff schedule that paces the
//! periodic refresh tick ([`LeaseRefreshBackoff`]). It was split out of the
//! parent `runtime` module to keep that file under the repository's
//! RULE-003 per-file line cap; the receive loop itself (bind, accept,
//! recovery) remains in `runtime`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::graft::GraftReceiverListener;
use atm_core::local_http::LocalCapability;
use atm_core::protocol::OwnerGeneration;
use atm_core::types::{AgentName, TeamName};

use crate::GraftClient;

use super::{GraftReceiverLoopContext, read_snapshot, warn_runtime_error};

/// The narrow daemon-lease surface the receive loop needs from a graft
/// client: announce/refresh/unregister for one loopback endpoint.
///
/// This trait exists so `GraftReceiverLoopContext`/`RegisteredGraftReceiver`
/// depend on an interface owned by this module, not on the concrete
/// `GraftClient` type from `crate::lib` — `GraftClient` in turn depends on
/// `GraftSession` (via `GraftClient::activate_session`), so a direct
/// `GraftReceiverLoopContext: Option<GraftClient>` field would create a
/// `GraftClient` <-> `GraftSession` architectural cycle (sc-boundary
/// SCB-CYCLE-001). `GraftClient` implements this trait in `crate::lib`; the
/// receive loop consumes only `Arc<dyn GraftReceiverLeaseClient>`.
pub(crate) trait GraftReceiverLeaseClient: Send + Sync {
    fn register_receiver_sync(
        &self,
        team: TeamName,
        agent: AgentName,
        endpoint: SocketAddr,
        capability: LocalCapability,
        owner_generation: OwnerGeneration,
    ) -> Result<(), AtmError>;

    fn refresh_receiver_sync(
        &self,
        team: TeamName,
        agent: AgentName,
        owner_generation: OwnerGeneration,
    ) -> Result<(), AtmError>;

    fn unregister_receiver_sync(
        &self,
        team: TeamName,
        agent: AgentName,
        owner_generation: OwnerGeneration,
    ) -> Result<(), AtmError>;
}

pub(crate) const GRAFT_LEASE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
#[expect(
    dead_code,
    reason = "ADR-056-reserved lease TTL window; retained as documentation for \
              the intended lease staleness bound until a consumer (e.g. a \
              window-gated fallback path) is implemented"
)]
pub(crate) const ACTIVE_LEASE_WINDOW: Duration = Duration::from_secs(15);
/// Caps how long one blocking lease announce/refresh/unregister client call is
/// allowed to hold up the caller before this loop gives up waiting on it and
/// moves on. A stalled (not merely slow) daemon must not compound across the
/// per-iteration refresh check and `Drop`'s unregister call to approach or
/// exceed `RECEIVE_LOOP_JOIN_DEADLINE` (rust-service-hardening RSH-001); the
/// helper thread that actually runs the call is detached and its late result
/// discarded on timeout, which is safe because every lease call this loop
/// makes is already best-effort and retried on the next tick.
const GRAFT_LEASE_CALL_DEADLINE: Duration = Duration::from_secs(1);
/// Initial and per-failure cadence for the periodic lease refresh, growing
/// with each consecutive failure (rust-service-hardening RSH-002) so a
/// sustained daemon outage does not drive a reconnect attempt every single
/// tick. A successful refresh resets the cadence back to this base interval,
/// so the healthy-daemon refresh rate required by deliverable 2 / AC6 is
/// unaffected.
const GRAFT_LEASE_REFRESH_BACKOFF_INITIAL: Duration = GRAFT_LEASE_REFRESH_INTERVAL;
/// Caps the refresh backoff so a long daemon outage still retries at a
/// bounded, actionable cadence instead of backing off indefinitely.
const GRAFT_LEASE_REFRESH_BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Bounds reconnect pressure from the periodic lease refresh during a
/// sustained daemon outage (rust-service-hardening RSH-002), instead of
/// retrying every fixed `GRAFT_LEASE_REFRESH_INTERVAL` tick. Mirrors
/// `ReceiverRecoveryCircuit`'s growing delay shape. A successful refresh
/// resets the cadence to the base interval immediately, so AC6's "still
/// refreshes on cadence while busy" requirement holds whenever the daemon is
/// healthy.
#[derive(Debug)]
pub(super) struct LeaseRefreshBackoff {
    next_attempt_at: Instant,
    delay: Duration,
}

impl LeaseRefreshBackoff {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            next_attempt_at: now + GRAFT_LEASE_REFRESH_BACKOFF_INITIAL,
            delay: GRAFT_LEASE_REFRESH_BACKOFF_INITIAL,
        }
    }

    fn due(&self, now: Instant) -> bool {
        now >= self.next_attempt_at
    }

    fn record_success(&mut self, now: Instant) {
        self.delay = GRAFT_LEASE_REFRESH_BACKOFF_INITIAL;
        self.next_attempt_at = now + self.delay;
    }

    fn record_failure(&mut self, now: Instant) {
        self.next_attempt_at = now + self.delay;
        self.delay = self
            .delay
            .saturating_mul(2)
            .min(GRAFT_LEASE_REFRESH_BACKOFF_MAX);
    }

    #[cfg(test)]
    fn current_delay(&self) -> Duration {
        self.delay
    }
}

/// The listener already stores its owner generation as the validated
/// `OwnerGeneration` newtype (RBP-F002), so this is a cheap clone rather
/// than a re-parse of a raw string on every ~1s refresh tick.
fn owner_generation(listener: &GraftReceiverListener) -> OwnerGeneration {
    listener.owner_generation().clone()
}

/// Runs a possibly-blocking daemon client call on a helper thread and gives up
/// waiting on it after `deadline` instead of blocking the caller indefinitely.
///
/// This bounds the receive loop's per-iteration lease call and `Drop`'s
/// unregister call so a stalled (not merely slow) daemon cannot silently push
/// shutdown latency toward or past `RECEIVE_LOOP_JOIN_DEADLINE`
/// (rust-service-hardening RSH-001). On timeout the helper thread is left
/// detached and its late result discarded — every call this loop makes
/// through this helper is already a best-effort lease operation retried on
/// the next tick, so a stranded attempt causes no correctness issue.
fn run_bounded_lease_call<F>(
    action: &'static str,
    deadline: Duration,
    call: F,
) -> Result<(), AtmError>
where
    F: FnOnce() -> Result<(), AtmError> + Send + 'static,
{
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let spawned = thread::Builder::new()
        .name("atm-graft-lease-call".to_string())
        .spawn(move || {
            let _ = result_tx.send(call());
        });
    if spawned.is_err() {
        return Err(AtmError::daemon_unavailable(format!(
            "failed to spawn graft {action} helper"
        )));
    }
    match result_rx.recv_timeout(deadline) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => Err(AtmError::new(
            AtmErrorCode::WaitTimeout,
            format!("graft {action} exceeded the {deadline:?} bounded deadline"),
        )),
        Err(RecvTimeoutError::Disconnected) => Err(AtmError::daemon_unavailable(format!(
            "graft {action} helper disconnected before returning a result"
        ))),
    }
}

/// Owns a listener together with its best-effort daemon lease lifecycle.
pub(super) struct RegisteredGraftReceiver {
    pub(super) listener: GraftReceiverListener,
    team: TeamName,
    agent: AgentName,
    client: Option<Arc<dyn GraftReceiverLeaseClient>>,
    /// Set once [`Self::announce`] has observed a successful register call.
    /// While `false`, the periodic loop keeps retrying `announce` (an
    /// idempotent upsert) instead of the owner-checked `refresh`, so a
    /// receiver that bound while the daemon was down still self-heals once
    /// the daemon returns (AC2) instead of mistaking "never registered" for
    /// "displaced".
    announced: bool,
    /// Set once a `refresh` call observes `AtmErrorCode::GraftReceiverNotOwner`
    /// for an already-`announced` lease — a genuine displacement (closes the
    /// RBQA-F001 dead-refresh-path finding). Once set, `refresh` becomes a
    /// permanent no-op for this generation: the receiver logs the transition
    /// once (via the returned `Err`), never crashes its accept loop, and
    /// never fights to reclaim a lease another generation now legitimately
    /// owns (AC5/AC8).
    displaced: bool,
}

impl RegisteredGraftReceiver {
    pub(super) fn new(listener: GraftReceiverListener, ctx: &GraftReceiverLoopContext) -> Self {
        Self {
            listener,
            team: ctx.team.clone(),
            agent: ctx.agent.clone(),
            client: ctx.client.clone(),
            announced: false,
            displaced: false,
        }
    }

    /// Returns the daemon client, establishing (and permanently caching) one
    /// via `GraftClient::connect_existing()` on first use if `ctx.client` did
    /// not already provide one. A transient announce/refresh failure never
    /// discards this client — the underlying transport already handles
    /// per-request reconnection, and discarding it here would make retries
    /// unable to reach a daemon reachable only through a test-injected or
    /// otherwise non-default-resolvable client.
    fn client(&mut self) -> Result<Arc<dyn GraftReceiverLeaseClient>, AtmError> {
        if let Some(client) = &self.client {
            return Ok(Arc::clone(client));
        }
        let client: Arc<dyn GraftReceiverLeaseClient> = Arc::new(GraftClient::connect_existing()?);
        self.client = Some(Arc::clone(&client));
        Ok(client)
    }

    /// Unconditionally announces this receiver's endpoint (ADR-056's
    /// `register`: an upsert), used once at bind/rebind time and retried by
    /// the periodic loop on every tick until it first succeeds.
    pub(super) fn announce(&mut self) -> Result<(), AtmError> {
        let client = self.client()?;
        let team = self.team.clone();
        let agent = self.agent.clone();
        let endpoint = self.listener.local_addr()?;
        let capability = self.listener.capability().clone();
        let generation = owner_generation(&self.listener);
        let result =
            run_bounded_lease_call("lease_announce", GRAFT_LEASE_CALL_DEADLINE, move || {
                client.register_receiver_sync(team, agent, endpoint, capability, generation)
            });
        if result.is_ok() {
            self.announced = true;
        }
        result
    }

    /// Owner-checked keepalive (ADR-056's `refresh`); see the `displaced`
    /// field doc for the NotOwner handling this method implements.
    fn refresh(&mut self) -> Result<(), AtmError> {
        if self.displaced {
            return Ok(());
        }
        if !self.announced {
            return self.announce();
        }
        let client = self.client()?;
        let team = self.team.clone();
        let agent = self.agent.clone();
        let generation = owner_generation(&self.listener);
        let result =
            run_bounded_lease_call("lease_refresh", GRAFT_LEASE_CALL_DEADLINE, move || {
                client.refresh_receiver_sync(team, agent, generation)
            });
        if let Err(error) = &result
            && error.code() == AtmErrorCode::GraftReceiverNotOwner
        {
            self.displaced = true;
        }
        result
    }
}

impl Drop for RegisteredGraftReceiver {
    fn drop(&mut self) {
        // ATM-QA-007: attempt a best-effort reconnect via `connect_existing()`
        // when no client was ever established (mirrors `client()`'s own
        // fallback), instead of unconditionally skipping the unregister
        // attempt. A transient hiccup that prevented every prior
        // announce/refresh from ever populating `self.client` must not also
        // silently lose the final unregister attempt at clean shutdown.
        let client = match self.client.take() {
            Some(client) => client,
            None => match GraftClient::connect_existing() {
                Ok(client) => Arc::new(client) as Arc<dyn GraftReceiverLeaseClient>,
                Err(error) => {
                    tracing::debug!(
                        subsystem = "atm_graft.receiver_loop",
                        action = "unregister_graft_receiver",
                        outcome = "best_effort_reconnect_failure",
                        error_code = %error.code(),
                        error_message = %error.message(),
                        "graft receiver could not reconnect to attempt a best-effort lease unregister"
                    );
                    return;
                }
            },
        };
        let team = self.team.clone();
        let agent = self.agent.clone();
        let generation = owner_generation(&self.listener);
        if let Err(error) =
            run_bounded_lease_call("lease_unregister", GRAFT_LEASE_CALL_DEADLINE, move || {
                client.unregister_receiver_sync(team, agent, generation)
            })
        {
            tracing::debug!(
                subsystem = "atm_graft.receiver_loop",
                action = "unregister_graft_receiver",
                outcome = "best_effort_failure",
                error_code = %error.code(),
                error_message = %error.message(),
                "graft receiver lease unregister did not complete"
            );
        }
    }
}

/// Runs the per-iteration lease refresh check unconditionally (deliverable
/// 2): unlike the pre-AQ1.6 idle-only `handle_idle_graft_receiver` path, this
/// runs regardless of whether the same iteration also accepted a connection,
/// so a continuously busy receiver still refreshes on cadence (AC6).
pub(super) fn tick_lease_refresh(
    ctx: &GraftReceiverLoopContext,
    listener: &mut RegisteredGraftReceiver,
    lease_backoff: &mut LeaseRefreshBackoff,
) {
    let now = Instant::now();
    if !lease_backoff.due(now) {
        return;
    }
    match listener.refresh() {
        Ok(()) => {
            lease_backoff.record_success(Instant::now());
            if let Ok(snapshot) = read_snapshot(&ctx.snapshot) {
                ctx.observability
                    .receiver_ownership(&snapshot, "refresh_receiver_lease", "ok");
            }
        }
        Err(error) => {
            lease_backoff.record_failure(Instant::now());
            warn_runtime_error("refresh_graft_receiver", Some(&ctx.graft_root), &error);
            let outcome = if error.code() == AtmErrorCode::GraftReceiverNotOwner {
                "displaced"
            } else {
                "error"
            };
            if let Ok(snapshot) = read_snapshot(&ctx.snapshot) {
                ctx.observability
                    .receiver_ownership(&snapshot, "refresh_receiver_lease", outcome);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{GRAFT_LEASE_REFRESH_INTERVAL, LeaseRefreshBackoff};

    // rust-service-hardening RSH-002: the refresh backoff grows on repeated
    // failure and resets to the base cadence on the next success, so a
    // sustained outage does not retry every fixed tick while a healthy
    // daemon keeps the AC6 cadence unaffected. Deterministic per ADR-008:
    // pure `Instant` arithmetic, no real waiting.
    #[test]
    fn rsh002_refresh_backoff_grows_on_failure_and_resets_on_success() {
        let now = Instant::now();
        let mut backoff = LeaseRefreshBackoff::new(now);
        assert_eq!(backoff.current_delay(), GRAFT_LEASE_REFRESH_INTERVAL);
        assert!(
            !backoff.due(now),
            "the first tick must not fire immediately at t=0"
        );
        assert!(backoff.due(now + GRAFT_LEASE_REFRESH_INTERVAL));

        backoff.record_failure(now + GRAFT_LEASE_REFRESH_INTERVAL);
        let grown_delay = backoff.current_delay();
        assert!(
            grown_delay > GRAFT_LEASE_REFRESH_INTERVAL,
            "a failure must grow the backoff delay beyond the base interval"
        );
        let next_due_at = backoff.next_attempt_at;
        assert!(
            !backoff.due(next_due_at - Duration::from_millis(1)),
            "the grown delay must not be satisfied one millisecond early"
        );
        assert!(
            backoff.due(next_due_at),
            "the next attempt must become due exactly at the grown delay boundary"
        );

        let mut previous_delay = grown_delay;
        let mut when = now + GRAFT_LEASE_REFRESH_INTERVAL;
        for _ in 0..10 {
            when += previous_delay;
            backoff.record_failure(when);
            previous_delay = backoff.current_delay();
        }
        assert!(
            previous_delay <= Duration::from_secs(30),
            "repeated failures must not grow the backoff without bound"
        );

        backoff.record_success(when);
        assert_eq!(
            backoff.current_delay(),
            GRAFT_LEASE_REFRESH_INTERVAL,
            "a success must reset the cadence back to the base interval"
        );
    }
}
