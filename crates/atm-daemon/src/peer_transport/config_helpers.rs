use atm_core::error::AtmError;

#[allow(
    dead_code,
    reason = "moved to crate::remote_replay during AG.21 cleanup"
)]
pub(super) fn remote_replay_store_not_configured_error() -> AtmError {
    AtmError::daemon_unavailable("remote replay store is not configured").with_recovery(
        "Restore the host-scoped ATM durable replay store before retrying remote delivery so atm-daemon can resume unknown peer handoffs safely.",
    )
}

#[allow(
    dead_code,
    reason = "moved to crate::remote_replay during AG.21 cleanup"
)]
pub(super) fn remote_retry_budget_expiry_error(
    source: impl std::error::Error + Send + Sync + 'static,
) -> AtmError {
    AtmError::daemon_unavailable("failed to convert remote retry budget into a replay expiry")
        .with_recovery(
            "Fix the daemon remote retry budget configuration and restart atm-daemon before retrying remote delivery so replay expiry can be computed deterministically.",
        )
        .with_source(source)
}
