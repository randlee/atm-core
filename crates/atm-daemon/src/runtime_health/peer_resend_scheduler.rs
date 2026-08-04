//! Bounded, daemon-lifetime peer resend aggregate.
//!
//! This is deliberately not a worker. The local ingress serve loop owns the
//! one due callback and calls `poll_due_peer_resends`; admissions use the same
//! synchronous `send_peer_http_frames` function as the AK.4 direct path.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroU16;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use atm_core::api::RequestDeadline;
use atm_core::doctor::{DoctorFinding, DoctorSeverity};
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::send::WriteRequest;
use atm_storage::{AtmMessageId, MessageStore, OutboundMessageQuery, PeerDirectory, PeerEndpoint};

use crate::peer_http_listener::{PeerHttpRuntimeConfig, send_peer_http_frames_and_confirm};

/// One bounded recovery pass never carries more than this many immutable
/// durable frames. It is a page size, not an in-memory queue capacity.
pub(crate) const PEER_RESEND_BATCH_LIMIT: u16 = 64;
pub(crate) const PEER_RESEND_DUE_CALLBACK_BUDGET: Duration = Duration::from_millis(250);
pub(crate) const PEER_RESEND_RETRY_DELAY: Duration = Duration::from_secs(60);
const PEER_RESEND_RETRY_JITTER_MAX: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeerConnectionState {
    Connected,
    /// A due callback or immediate send owns the one connection attempt.
    /// This is an in-progress guard, never a persisted health state.
    Disconnected,
    Queued {
        due_at: Instant,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PeerResendAggregate {
    state: PeerConnectionState,
}

#[derive(Debug, Default)]
struct PeerResendState {
    aggregates: HashMap<PeerEndpoint, PeerResendAggregate>,
    earliest_due: Option<Instant>,
}

/// The sole interior-mutable AK.5 object. The mutex is required to make an
/// admission's state transition and coalesced deadline update atomic; it is
/// never held across SQLite or network I/O.
pub(crate) struct PeerResendScheduler {
    state: Mutex<PeerResendState>,
    http: PeerHttpRuntimeConfig,
    directory: PeerDirectory,
    outbound: Arc<dyn OutboundMessageQuery + Send + Sync>,
    messages: Arc<dyn MessageStore + Send + Sync>,
}

impl PeerResendScheduler {
    pub(crate) fn new(
        http: PeerHttpRuntimeConfig,
        directory: PeerDirectory,
        outbound: Arc<dyn OutboundMessageQuery + Send + Sync>,
        messages: Arc<dyn MessageStore + Send + Sync>,
    ) -> Self {
        Self {
            state: Mutex::new(PeerResendState::default()),
            http,
            directory,
            outbound,
            messages,
        }
    }

    /// Attempts one persisted frame immediately unless its endpoint is
    /// already queued or in progress. An initial failure remains visible to
    /// the originating caller and only schedules a future aggregate retry.
    pub(crate) fn deliver_or_queue(
        &self,
        endpoint: PeerEndpoint,
        message_id: AtmMessageId,
        write: &WriteRequest,
        deadline: RequestDeadline,
    ) -> Result<(), AtmError> {
        {
            let mut state = self.lock_state()?;
            match state
                .aggregates
                .get(&endpoint)
                .map(|aggregate| aggregate.state)
                .unwrap_or(PeerConnectionState::Connected)
            {
                PeerConnectionState::Connected => {
                    state.aggregates.insert(
                        endpoint.clone(),
                        PeerResendAggregate {
                            state: PeerConnectionState::Disconnected,
                        },
                    );
                }
                PeerConnectionState::Queued { .. } | PeerConnectionState::Disconnected => {
                    return Err(AtmError::remote_delivery_unconfirmed(format!(
                        "local persistence succeeded; configured peer `{}` delivery is pending its scheduled retry",
                        endpoint.canonical_host
                    )));
                }
            }
        }

        let result = self.send_and_confirm(
            &endpoint,
            std::slice::from_ref(write),
            &[message_id],
            deadline,
        );
        let mut state = self.lock_state()?;
        match result {
            Ok(()) => {
                state.aggregates.insert(
                    endpoint,
                    PeerResendAggregate {
                        state: PeerConnectionState::Connected,
                    },
                );
                Self::recompute_earliest_due(&mut state);
                Ok(())
            }
            Err(error) => {
                Self::queue_after_failure(&mut state, endpoint, Instant::now());
                Err(error)
            }
        }
    }

    /// Schedules only existing durable peer hosts. The directory is the only
    /// alias/port source; an absent host remains durable but untouched.
    pub(crate) fn bootstrap_pending_peer_resends(&self) -> Result<(), AtmError> {
        let hosts = self
            .outbound
            .pending_peer_hosts(PEER_RESEND_DUE_CALLBACK_BUDGET)?;
        let mut state = self.lock_state()?;
        for host in hosts {
            let Some(endpoint) = self.directory.endpoint_for_canonical_host(&host) else {
                tracing::warn!(
                    subsystem = "peer_resend",
                    action = "bootstrap",
                    outcome = "unconfigured_host",
                    canonical_host = %host,
                    "retaining an undelivered peer message whose configured endpoint no longer exists"
                );
                continue;
            };
            let due_at = retry_due_at(&endpoint, Instant::now());
            state
                .aggregates
                .entry(endpoint)
                .or_insert(PeerResendAggregate {
                    state: PeerConnectionState::Queued { due_at },
                });
        }
        Self::recompute_earliest_due(&mut state);
        Ok(())
    }

    pub(crate) fn next_due(&self) -> Result<Option<Instant>, AtmError> {
        next_due_from_state_lock(&self.state)
    }

    pub(crate) fn doctor_findings(&self) -> Result<Vec<DoctorFinding>, AtmError> {
        let state = self.lock_state()?;
        Ok(peer_resend_doctor_findings(&state))
    }

    /// Runs exactly one due endpoint. It releases the state mutex before both
    /// the bounded SQLite read and the shared HTTP call, so local admissions
    /// can only observe the `Disconnected` guard and never block on I/O.
    pub(crate) fn poll_due_peer_resends(&self, now: Instant) -> Result<(), AtmError> {
        let endpoint = {
            let mut state = self.lock_state()?;
            let selected = select_due_endpoint(&state, now);
            if let Some(endpoint) = &selected {
                state.aggregates.insert(
                    endpoint.clone(),
                    PeerResendAggregate {
                        state: PeerConnectionState::Disconnected,
                    },
                );
            }
            selected
        };
        let Some(endpoint) = endpoint else {
            return Ok(());
        };

        let result = self.send_due_page(&endpoint);
        let mut state = self.lock_state()?;
        match result {
            Ok(has_possible_more) => {
                // A page can contain at most 64 records, but the durable
                // backlog is unbounded. Re-arm immediately after a full page
                // so the existing serve loop drains the next bounded page
                // rather than leaving older persisted messages stranded.
                let next_state = if has_possible_more {
                    PeerConnectionState::Queued {
                        due_at: Instant::now(),
                    }
                } else {
                    PeerConnectionState::Connected
                };
                state
                    .aggregates
                    .insert(endpoint, PeerResendAggregate { state: next_state });
                Self::recompute_earliest_due(&mut state);
                Ok(())
            }
            Err(error) => {
                Self::queue_after_failure(&mut state, endpoint, Instant::now());
                Err(error)
            }
        }
    }

    /// Returns whether the bounded page may be followed by another page.
    fn send_due_page(&self, endpoint: &PeerEndpoint) -> Result<bool, AtmError> {
        let limit = NonZeroU16::new(PEER_RESEND_BATCH_LIMIT)
            .expect("AK.5 fixed resend batch limit is non-zero");
        let stored = self.outbound.page_for_peer(
            &endpoint.canonical_host,
            None,
            limit,
            PEER_RESEND_DUE_CALLBACK_BUDGET,
        )?;
        if stored.is_empty() {
            return Ok(false);
        }
        let has_possible_more = stored.len() == usize::from(PEER_RESEND_BATCH_LIMIT);
        let mut writes = Vec::with_capacity(stored.len());
        let mut message_ids = Vec::with_capacity(stored.len());
        for record in stored {
            let write = serde_json::from_str(&record.request_json).map_err(|source| {
                AtmError::remote_delivery_unconfirmed(format!(
                    "retained peer write `{}` is invalid and remains undelivered: {source}",
                    record.message_id
                ))
            })?;
            writes.push(write);
            message_ids.push(record.message_id);
        }
        self.send_and_confirm(
            endpoint,
            &writes,
            &message_ids,
            RequestDeadline::after(PEER_RESEND_DUE_CALLBACK_BUDGET),
        )?;
        Ok(has_possible_more)
    }

    fn send_and_confirm(
        &self,
        endpoint: &PeerEndpoint,
        writes: &[WriteRequest],
        message_ids: &[AtmMessageId],
        deadline: RequestDeadline,
    ) -> Result<(), AtmError> {
        debug_assert_eq!(writes.len(), message_ids.len());
        send_peer_http_frames_and_confirm(
            &self.http,
            endpoint,
            writes,
            message_ids,
            self.messages.as_ref(),
            deadline,
        )
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, PeerResendState>, AtmError> {
        self.state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("peer resend state lock poisoned"))
    }

    fn queue_after_failure(state: &mut PeerResendState, endpoint: PeerEndpoint, now: Instant) {
        let due_at = retry_due_at(&endpoint, now);
        state.aggregates.insert(
            endpoint,
            PeerResendAggregate {
                state: PeerConnectionState::Queued { due_at },
            },
        );
        Self::recompute_earliest_due(state);
    }

    fn recompute_earliest_due(state: &mut PeerResendState) {
        state.earliest_due = state
            .aggregates
            .values()
            .filter_map(|aggregate| match aggregate.state {
                PeerConnectionState::Queued { due_at } => Some(due_at),
                PeerConnectionState::Connected | PeerConnectionState::Disconnected => None,
            })
            .min();
    }
}

fn retry_due_at(endpoint: &PeerEndpoint, now: Instant) -> Instant {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    endpoint.hash(&mut hasher);
    let max_jitter_millis = u64::try_from(PEER_RESEND_RETRY_JITTER_MAX.as_millis())
        .expect("fixed resend jitter fits in u64 milliseconds");
    let jitter = Duration::from_millis(hasher.finish() % (max_jitter_millis + 1));
    now + PEER_RESEND_RETRY_DELAY + jitter
}

fn next_due_from_state_lock(state: &Mutex<PeerResendState>) -> Result<Option<Instant>, AtmError> {
    Ok(state
        .lock()
        .map_err(|_| AtmError::daemon_unavailable("peer resend state lock poisoned"))?
        .earliest_due)
}

fn select_due_endpoint(state: &PeerResendState, now: Instant) -> Option<PeerEndpoint> {
    state
        .aggregates
        .iter()
        .filter_map(|(endpoint, aggregate)| match aggregate.state {
            PeerConnectionState::Queued { due_at } if due_at <= now => {
                Some((due_at, endpoint.clone()))
            }
            _ => None,
        })
        .min_by(|(left_due, left_endpoint), (right_due, right_endpoint)| {
            left_due
                .cmp(right_due)
                .then_with(|| {
                    left_endpoint
                        .canonical_host
                        .cmp(&right_endpoint.canonical_host)
                })
                .then_with(|| left_endpoint.port.cmp(&right_endpoint.port))
        })
        .map(|(_, endpoint)| endpoint)
}

fn peer_resend_doctor_findings(state: &PeerResendState) -> Vec<DoctorFinding> {
    let queued = state
        .aggregates
        .values()
        .filter(|aggregate| matches!(aggregate.state, PeerConnectionState::Queued { .. }))
        .count();
    let disconnected = state
        .aggregates
        .values()
        .filter(|aggregate| matches!(aggregate.state, PeerConnectionState::Disconnected))
        .count();
    if queued == 0 && disconnected == 0 {
        return Vec::new();
    }
    let severity = if disconnected > 0 {
        DoctorSeverity::Warning
    } else {
        DoctorSeverity::Info
    };
    vec![DoctorFinding {
        severity,
        code: AtmErrorCode::WarningSendAlertStateDegraded,
        message: format!(
            "peer resend state: queued_endpoints={queued}, in_progress_endpoints={disconnected}"
        ),
        remediation: (disconnected > 0).then(|| {
            "If an in-progress endpoint does not clear after its bounded callback, inspect the peer transport and retained daemon log, then rerun `atm doctor`.".to_owned()
        }),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU16;

    fn endpoint(host: &str, port: u16) -> PeerEndpoint {
        PeerEndpoint {
            canonical_host: host.parse().expect("host"),
            port: NonZeroU16::new(port).expect("port"),
        }
    }

    #[test]
    fn queued_and_disconnected_transition_matrix_preserves_one_admission_guard() {
        let now = Instant::now();
        let queued = endpoint("queued.example.test", 443);
        let disconnected = endpoint("in-progress.example.test", 443);
        let mut state = PeerResendState::default();
        state.aggregates.insert(
            queued.clone(),
            PeerResendAggregate {
                state: PeerConnectionState::Queued { due_at: now },
            },
        );
        state.aggregates.insert(
            disconnected,
            PeerResendAggregate {
                state: PeerConnectionState::Disconnected,
            },
        );

        assert_eq!(select_due_endpoint(&state, now), Some(queued.clone()));
        state.aggregates.insert(
            queued,
            PeerResendAggregate {
                state: PeerConnectionState::Disconnected,
            },
        );
        assert_eq!(select_due_endpoint(&state, now), None);
    }

    #[test]
    fn disabled_caching_has_no_aggregate_or_due_event() {
        let state = PeerResendState::default();
        assert!(state.aggregates.is_empty());
        assert_eq!(state.earliest_due, None);
        assert_eq!(select_due_endpoint(&state, Instant::now()), None);
    }

    #[test]
    fn restart_bootstrap_state_is_queued_without_payload_or_connection_state() {
        let now = Instant::now();
        let endpoint = endpoint("restart.example.test", 443);
        let due_at = retry_due_at(&endpoint, now);
        let aggregate = PeerResendAggregate {
            state: PeerConnectionState::Queued { due_at },
        };

        assert!(due_at >= now + PEER_RESEND_RETRY_DELAY);
        assert!(due_at <= now + PEER_RESEND_RETRY_DELAY + PEER_RESEND_RETRY_JITTER_MAX);
        assert!(matches!(
            aggregate.state,
            PeerConnectionState::Queued { .. }
        ));
    }

    #[test]
    fn timer_callback_selects_one_deterministic_due_endpoint() {
        let now = Instant::now();
        let first = endpoint("a.example.test", 443);
        let second = endpoint("b.example.test", 443);
        let mut state = PeerResendState::default();
        for endpoint in [second.clone(), first.clone()] {
            state.aggregates.insert(
                endpoint,
                PeerResendAggregate {
                    state: PeerConnectionState::Queued { due_at: now },
                },
            );
        }

        assert_eq!(select_due_endpoint(&state, now), Some(first));
    }

    #[test]
    fn next_due_propagates_a_poisoned_mutex() {
        let state = Mutex::new(PeerResendState::default());
        let _ = std::panic::catch_unwind(|| {
            let _guard = state.lock().expect("lock");
            panic!("poison peer resend state");
        });

        let error = next_due_from_state_lock(&state).expect_err("poison is not an empty schedule");
        assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
    }

    #[test]
    fn doctor_reports_queued_and_in_progress_endpoints() {
        let now = Instant::now();
        let mut state = PeerResendState::default();
        state.aggregates.insert(
            endpoint("queued.example.test", 443),
            PeerResendAggregate {
                state: PeerConnectionState::Queued { due_at: now },
            },
        );
        state.aggregates.insert(
            endpoint("in-progress.example.test", 443),
            PeerResendAggregate {
                state: PeerConnectionState::Disconnected,
            },
        );

        let findings = peer_resend_doctor_findings(&state);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, DoctorSeverity::Warning);
        assert!(findings[0].message.contains("queued_endpoints=1"));
        assert!(findings[0].message.contains("in_progress_endpoints=1"));
    }
}
