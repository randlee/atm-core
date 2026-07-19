use std::sync::Arc;

use atm_core::boundary::{MessageKey, RemoteReplayStateRecord, RemoteReplayStore};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::protocol::RequestEnvelope;
use atm_core::send::RemoteTargetHost;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};

use crate::SubsystemObservability;

pub(crate) fn persist_replay_request(
    retry_budget: std::time::Duration,
    replay_store: Option<&Arc<dyn RemoteReplayStore>>,
    remote_host: RemoteTargetHost,
    team: TeamName,
    agent: AgentName,
    message_key: MessageKey,
    request: RequestEnvelope,
) -> Result<(), AtmError> {
    let Some(replay_store) = replay_store else {
        return Err(remote_replay_store_not_configured_error());
    };
    let recorded_at = IsoTimestamp::now();
    let expires_at = IsoTimestamp::from_datetime(
        recorded_at.into_inner()
            + chrono::Duration::from_std(retry_budget).map_err(remote_retry_budget_expiry_error)?,
    );
    replay_store.enqueue(RemoteReplayStateRecord {
        team,
        agent,
        message_key,
        remote_host,
        request,
        recorded_at,
        expires_at,
        attempt_count: 0,
        last_attempt_at: None,
        last_error: None,
    })
}

pub(crate) fn replay_error_is_terminal(error: &AtmError) -> bool {
    !matches!(
        error.code,
        AtmErrorCode::DaemonUnavailable | AtmErrorCode::RemoteDeliveryOutcomeUnknown
    )
}

pub(crate) fn expire_replay_record(
    replay_store: &dyn RemoteReplayStore,
    record: &RemoteReplayStateRecord,
) -> Result<(), AtmError> {
    replay_store.delete(&record.team, &record.agent, &record.message_key)?;
    Ok(())
}

pub(crate) fn complete_replay_record(
    replay_store: &dyn RemoteReplayStore,
    observability: &SubsystemObservability,
    record: &RemoteReplayStateRecord,
) -> Result<(), AtmError> {
    replay_store.delete(&record.team, &record.agent, &record.message_key)?;
    tracing::info!(
        message_key = %record.message_key,
        remote_host = %record.remote_host.as_str(),
        replay_attempt_count = record.attempt_count,
        "daemon remote replay delivered successfully"
    );
    observability.emit_or_warn(
        "resume_pending_replay",
        "ok",
        "daemon remote replay delivered a retained record",
    );
    Ok(())
}

pub(crate) fn fail_replay_record_terminal(
    replay_store: &dyn RemoteReplayStore,
    record: &RemoteReplayStateRecord,
) -> Result<(), AtmError> {
    replay_store.delete(&record.team, &record.agent, &record.message_key)?;
    Ok(())
}

pub(crate) fn retain_replay_record(
    replay_store: &dyn RemoteReplayStore,
    observability: &SubsystemObservability,
    record: &mut RemoteReplayStateRecord,
    error: &AtmError,
) -> Result<(), AtmError> {
    record.attempt_count = record.attempt_count.saturating_add(1);
    record.last_attempt_at = Some(IsoTimestamp::now());
    record.last_error = Some(error.code);
    tracing::warn!(
        subsystem = "peer_transport",
        action = "resume_replay",
        outcome = "skipped",
        message_key = %record.message_key,
        remote_host = %record.remote_host.as_str(),
        replay_attempt_count = record.attempt_count,
        error_code = %error.code,
        error_message = %error.message,
        "daemon remote replay delivery attempt failed; retaining record"
    );
    observability.emit_or_warn(
        "resume_pending_replay",
        "degraded",
        "daemon remote replay delivery failed and retained the record for retry",
    );
    replay_store.enqueue(record.clone())
}

fn remote_replay_store_not_configured_error() -> AtmError {
    AtmError::daemon_unavailable(
        "remote replay persistence is unavailable because no replay store is configured",
    )
    .with_recovery(
        "Repair the daemon runtime assembly so a replay store is available before retrying remote delivery persistence.",
    )
}

fn remote_retry_budget_expiry_error(
    source: impl std::error::Error + Send + Sync + 'static,
) -> AtmError {
    AtmError::daemon_unavailable("failed to convert remote retry budget into a replay expiry")
        .with_recovery(
            "Repair the bounded retry duration configuration or its conversion path before retrying remote delivery persistence.",
        )
        .with_source(source)
}
