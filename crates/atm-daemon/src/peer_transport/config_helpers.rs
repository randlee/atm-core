use std::net::SocketAddr;

use atm_core::error::AtmError;

use super::parse_peer_endpoint;

pub(super) fn remote_replay_store_not_configured_error() -> AtmError {
    AtmError::daemon_unavailable("remote replay store is not configured").with_recovery(
        "Restore the host-scoped ATM durable replay store before retrying remote delivery so atm-daemon can resume unknown peer handoffs safely.",
    )
}

pub(super) fn remote_peer_endpoint_not_configured_error() -> AtmError {
    AtmError::daemon_unavailable("remote peer endpoint is not configured").with_recovery(
        "Set ATM_DAEMON_PEER_ADDR or configure the daemon peer transport before retrying remote delivery or replay persistence.",
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

pub(super) fn remote_replay_persistence_failed_error(source: AtmError) -> AtmError {
    AtmError::remote_delivery_outcome_unknown(
        "remote peer delivery outcome is unknown and replay persistence failed",
    )
    .with_source(source)
}

pub(super) fn daemon_peer_endpoint_from_env() -> Option<SocketAddr> {
    match std::env::var("ATM_DAEMON_PEER_ADDR") {
        Ok(raw) => parse_peer_endpoint(&raw),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            tracing::warn!(
                subsystem = "peer_transport",
                action = "env_parse",
                outcome = "ignored",
                "ignoring non-unicode ATM_DAEMON_PEER_ADDR value"
            );
            None
        }
    }
}
