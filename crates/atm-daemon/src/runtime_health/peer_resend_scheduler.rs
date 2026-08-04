//! Bounded, daemon-lifetime peer resend aggregate.
//!
//! This is deliberately not a worker. The local ingress serve loop owns the
//! one due callback and calls `poll_due_peer_resends`; admissions use the same
//! synchronous `send_peer_http_frames` function as the AK.4 direct path.

use std::collections::HashMap;
use std::num::NonZeroU16;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use atm_core::api::RequestDeadline;
use atm_core::error::AtmError;
use atm_core::send::WriteRequest;
use atm_storage::{
    AtmMessageId, MessageStore, OutboundMessageQuery, PeerDeliveryConfirmation, PeerDirectory,
    PeerEndpoint,
};

use crate::peer_http_listener::{PeerHttpRuntimeConfig, send_peer_http_frames};

/// One bounded recovery pass never carries more than this many immutable
/// durable frames. It is a page size, not an in-memory queue capacity.
pub(crate) const PEER_RESEND_BATCH_LIMIT: u16 = 64;
pub(crate) const PEER_RESEND_DUE_CALLBACK_BUDGET: Duration = Duration::from_millis(250);
pub(crate) const PEER_RESEND_RETRY_DELAY: Duration = Duration::from_secs(60);

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
        let due_at = Instant::now() + PEER_RESEND_RETRY_DELAY;
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

    pub(crate) fn next_due(&self) -> Option<Instant> {
        self.lock_state().ok().and_then(|state| state.earliest_due)
    }

    /// Runs exactly one due endpoint. It releases the state mutex before both
    /// the bounded SQLite read and the shared HTTP call, so local admissions
    /// can only observe the `Disconnected` guard and never block on I/O.
    pub(crate) fn poll_due_peer_resends(&self, now: Instant) -> Result<(), AtmError> {
        let endpoint = {
            let mut state = self.lock_state()?;
            let selected = state
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
                .map(|(_, endpoint)| endpoint);
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
        let responses = send_peer_http_frames(&self.http, endpoint, writes, deadline)?;
        if responses.len() != message_ids.len() {
            return Err(AtmError::remote_delivery_unconfirmed(format!(
                "configured peer `{}` returned an incomplete response set for retained delivery",
                endpoint.canonical_host
            )));
        }
        for message_id in message_ids {
            self.messages
                .confirm_peer_delivery(PeerDeliveryConfirmation {
                    message_id: *message_id,
                    canonical_host: endpoint.canonical_host.clone(),
                })?;
        }
        Ok(())
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, PeerResendState>, AtmError> {
        self.state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("peer resend state lock poisoned"))
    }

    fn queue_after_failure(state: &mut PeerResendState, endpoint: PeerEndpoint, now: Instant) {
        state.aggregates.insert(
            endpoint,
            PeerResendAggregate {
                state: PeerConnectionState::Queued {
                    due_at: now + PEER_RESEND_RETRY_DELAY,
                },
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
