use atm_core::error::AtmError;

pub(super) fn remote_replay_store_not_configured_error() -> AtmError {
    AtmError::daemon_unavailable("remote replay store is not configured").with_recovery(
        "Restore the host-scoped ATM durable replay store before retrying remote delivery so atm-daemon can resume unknown peer handoffs safely.",
    )
}

pub(super) fn remote_peer_endpoint_not_configured_error() -> AtmError {
    AtmError::daemon_unavailable("remote peer endpoint is not configured").with_recovery(
        "Use a concrete resolved peer endpoint from the cross-host dispatch path before retrying remote delivery or replay persistence.",
    )
}

pub(super) fn remote_retry_budget_expiry_error(
    source: impl std::error::Error + Send + Sync + 'static,
) -> AtmError {
    AtmError::daemon_unavailable("failed to convert remote retry budget into a replay expiry")
        .with_recovery(
            "Fix the daemon remote retry budget configuration and restart atm-daemon before retrying remote delivery so replay expiry can be computed deterministically.",
        )
        .with_source(source)
}
