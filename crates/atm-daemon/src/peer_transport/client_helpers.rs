use super::*;

pub(super) fn peer_read_deadline_error(source: std::io::Error) -> Box<AttemptFailure> {
    Box::new(AttemptFailure {
        kind: AttemptFailureKind::Retryable,
        error: AtmError::daemon_unavailable("failed to apply remote peer read deadline")
            .with_recovery(
                "Restart atm-daemon and retry after the peer socket can accept bounded read deadlines again.",
            )
            .with_source(source),
    })
}

pub(super) fn peer_write_deadline_error(source: std::io::Error) -> Box<AttemptFailure> {
    Box::new(AttemptFailure {
        kind: AttemptFailureKind::Retryable,
        error: AtmError::daemon_unavailable("failed to apply remote peer write deadline")
            .with_recovery(
                "Restart atm-daemon and retry after the peer socket can accept bounded write deadlines again.",
            )
            .with_source(source),
    })
}

pub(super) fn peer_flush_error(source: std::io::Error) -> Box<AttemptFailure> {
    Box::new(AttemptFailure {
        kind: AttemptFailureKind::OutcomeUnknown,
        error: AtmError::remote_delivery_outcome_unknown(
            "failed to flush the remote peer request frame before waiting for a response",
        )
        .with_source(source),
    })
}

pub(super) fn peer_closed_before_response_error() -> Box<AttemptFailure> {
    Box::new(AttemptFailure {
        kind: AttemptFailureKind::OutcomeUnknown,
        error: AtmError::remote_delivery_outcome_unknown(
            "remote peer closed the connection before returning a response frame",
        )
        .with_recovery(
            "Check the destination daemon or mailbox before retrying. If local durable replay is enabled, let the daemon resume the pending handoff rather than guessing success.",
        ),
    })
}

pub(super) fn peer_response_decode_error(error: AtmError) -> Box<AttemptFailure> {
    Box::new(AttemptFailure {
        kind: AttemptFailureKind::NonRetryable,
        error: AtmError::daemon_unavailable("failed to decode remote peer response frame")
            .with_recovery(
                "Align the peer daemon builds so both sides speak the same ATM daemon protocol before retrying the remote delivery.",
            )
            .with_source(error),
    })
}

pub(super) fn peer_response_id_mismatch_error(
    response_id: atm_core::protocol::RequestId,
    request_id: atm_core::protocol::RequestId,
) -> Box<AttemptFailure> {
    Box::new(AttemptFailure {
        kind: AttemptFailureKind::NonRetryable,
        error: AtmError::daemon_unavailable(format!(
            "remote peer response request_id {} did not match request_id {}",
            response_id, request_id
        ))
        .with_recovery(
            "Align the peer daemon builds so both sides use the same ATM daemon protocol contract before retrying.",
        ),
    })
}

pub(super) fn wait_for_retry_backoff(terminate: &Arc<AtomicBool>, sleep_for: Duration) -> bool {
    const RETRY_POLL_INTERVAL: Duration = Duration::from_millis(25);

    let started = Instant::now();
    loop {
        if terminate.load(Ordering::SeqCst) {
            return true;
        }
        let elapsed = started.elapsed();
        if elapsed >= sleep_for {
            return false;
        }
        let remaining = sleep_for.saturating_sub(elapsed);
        std::thread::sleep(remaining.min(RETRY_POLL_INTERVAL));
    }
}

pub(super) fn daemon_terminate_flag() -> Result<Arc<AtomicBool>, AtmError> {
    static DAEMON_TERMINATE_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();

    if let Some(flag) = DAEMON_TERMINATE_FLAG.get() {
        return Ok(Arc::clone(flag));
    }

    let flag = crate::lifecycle_control::LifecycleControlSourceAdapter::install()?.terminate_flag();
    match DAEMON_TERMINATE_FLAG.set(Arc::clone(&flag)) {
        Ok(()) => Ok(flag),
        Err(existing) => Ok(existing),
    }
}

pub(super) fn classify_io_error(error: &std::io::Error) -> AttemptFailureKind {
    match error.kind() {
        std::io::ErrorKind::TimedOut
        | std::io::ErrorKind::Interrupted
        | std::io::ErrorKind::ConnectionRefused
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::BrokenPipe
        | std::io::ErrorKind::AddrNotAvailable
        | std::io::ErrorKind::NotConnected
        | std::io::ErrorKind::HostUnreachable
        | std::io::ErrorKind::NetworkUnreachable => AttemptFailureKind::Retryable,
        _ => AttemptFailureKind::NonRetryable,
    }
}

pub(super) fn replay_metadata_for_request(
    request: &RequestEnvelope,
) -> Option<(TeamName, AgentName, MessageKey)> {
    match request {
        RequestEnvelope::Heartbeat(heartbeat) => Some((
            heartbeat.team.clone(),
            heartbeat.member.clone(),
            heartbeat_message_key(heartbeat),
        )),
        _ => None,
    }
}

fn heartbeat_message_key(request: &TeamMemberHeartbeatRequest) -> MessageKey {
    MessageKey::new(format!(
        "remote-heartbeat:{}:{}:{}:{}",
        request.team.as_str(),
        request.member.as_str(),
        request.pid,
        request.observed_at.into_inner().to_rfc3339(),
    ))
    .expect("validated heartbeat replay keys are never blank")
}

pub(super) fn jittered_backoff(base: Duration, seed: u64) -> Duration {
    let base_nanos = base.as_nanos();
    if base_nanos == 0 {
        return Duration::ZERO;
    }
    let offset_basis_points = (seed % 4001) as i128 - 2000;
    let scaled = (base_nanos as i128 * (10_000 + offset_basis_points))
        .div_euclid(10_000)
        .max(1);
    Duration::from_nanos(u64::try_from(scaled).unwrap_or(u64::MAX))
}

pub(super) fn jitter_seed(endpoint: SocketAddr, attempt: u32) -> u64 {
    let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos() as u64,
        Err(error) => {
            tracing::warn!(
                subsystem = "peer_transport",
                action = "jitter_seed",
                outcome = "default",
                %error,
                "system clock is before the unix epoch; using deterministic jitter fallback"
            );
            0
        }
    };
    now ^ u64::from(endpoint.port()) ^ (u64::from(attempt) << 32)
}

pub(super) fn parse_peer_endpoint(raw: &str) -> Option<SocketAddr> {
    match raw.parse::<SocketAddr>() {
        Ok(endpoint) => Some(endpoint),
        Err(error) => {
            tracing::warn!(
                subsystem = "peer_transport",
                action = "parse_endpoint",
                outcome = "skipped",
                %raw,
                %error,
                "parse_peer_endpoint: invalid address format"
            );
            None
        }
    }
}

impl boundary::sealed::Sealed for PeerClientTransport {}

impl ClientTransport for PeerClientTransport {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let endpoint = self
            .endpoint
            .ok_or_else(remote_peer_endpoint_not_configured_error)?;
        self.send_with_outcome_persistence(endpoint, request)
    }
}
