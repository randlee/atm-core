use std::net::SocketAddr;
use std::sync::Arc;

use atm_core::boundary::{MessageKey, RemoteReplayStateRecord, RemoteReplayStore};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::protocol::RequestEnvelope;
use atm_core::schema::AtmMessageId;
use atm_core::send::{RemoteDeliveryReceiptStatus, finalize_remote_delivery_receipt_with_runtime};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_core::with_default_local_service_runtime;

use super::remote_retry_budget_expiry_error;
use crate::SubsystemObservability;

#[expect(
    clippy::too_many_arguments,
    reason = "cross-host retry persistence needs explicit sender and receipt metadata at the peer-transport boundary"
)]
pub(super) fn persist_replay_request(
    retry_budget: std::time::Duration,
    replay_store: Option<&Arc<dyn RemoteReplayStore>>,
    endpoint: SocketAddr,
    team: TeamName,
    agent: AgentName,
    message_key: MessageKey,
    request: RequestEnvelope,
    receipt_sender_team: Option<TeamName>,
    receipt_sender_agent: Option<AgentName>,
    receipt_message_id: Option<AtmMessageId>,
    receipt_target: Option<String>,
    receipt_remote_host: Option<String>,
) -> Result<(), AtmError> {
    let Some(replay_store) = replay_store else {
        return Err(super::remote_replay_store_not_configured_error());
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
        peer_addr: endpoint,
        request,
        recorded_at,
        expires_at,
        attempt_count: 0,
        last_attempt_at: None,
        last_error: None,
        receipt_sender_team,
        receipt_sender_agent,
        receipt_message_id,
        receipt_target,
        receipt_remote_host,
    })
}

pub(super) fn replay_error_is_terminal(error: &AtmError) -> bool {
    !matches!(
        error.code,
        AtmErrorCode::DaemonUnavailable | AtmErrorCode::RemoteDeliveryOutcomeUnknown
    )
}

pub(super) fn finalize_replay_receipt(
    record: &RemoteReplayStateRecord,
    status: RemoteDeliveryReceiptStatus,
    body: &str,
) -> Result<bool, AtmError> {
    let (
        Some(sender_team),
        Some(sender_agent),
        Some(receipt_message_id),
        Some(target),
        Some(remote_host),
    ) = (
        record.receipt_sender_team.as_ref(),
        record.receipt_sender_agent.as_ref(),
        record.receipt_message_id,
        record.receipt_target.as_deref(),
        record.receipt_remote_host.as_deref(),
    )
    else {
        return Ok(false);
    };
    let RequestEnvelope::Send(send_request) = &record.request else {
        return Ok(false);
    };
    let target = target.parse()?;
    with_default_local_service_runtime(|runtime| {
        finalize_remote_delivery_receipt_with_runtime(
            runtime,
            &send_request.home_dir,
            sender_team,
            sender_agent,
            receipt_message_id,
            &target,
            remote_host,
            send_request.task_id.clone(),
            body,
            status,
        )
        .map(|_| true)
    })
}

pub(super) fn expire_replay_record(
    replay_store: &dyn RemoteReplayStore,
    record: &RemoteReplayStateRecord,
) -> Result<bool, AtmError> {
    let updated = finalize_replay_receipt(
        record,
        RemoteDeliveryReceiptStatus::Failed,
        "ATM failed deferred remote delivery because the bounded retry window expired before the remote daemon accepted the message.",
    )?;
    replay_store.delete(&record.team, &record.agent, &record.message_key)?;
    Ok(updated)
}

pub(super) fn complete_replay_record(
    replay_store: &dyn RemoteReplayStore,
    observability: &SubsystemObservability,
    record: &RemoteReplayStateRecord,
) -> Result<bool, AtmError> {
    let updated = finalize_replay_receipt(
        record,
        RemoteDeliveryReceiptStatus::Delivered,
        "ATM delivered the deferred remote message after replay resumed and the remote daemon accepted it.",
    )?;
    replay_store.delete(&record.team, &record.agent, &record.message_key)?;
    tracing::info!(
        message_key = %record.message_key,
        peer_addr = %record.peer_addr,
        replay_attempt_count = record.attempt_count,
        "daemon remote replay delivered successfully"
    );
    observability.emit_or_warn(
        "resume_pending_replay",
        "ok",
        "daemon remote replay delivered a retained record",
    );
    Ok(updated)
}

pub(super) fn fail_replay_record_terminal(
    replay_store: &dyn RemoteReplayStore,
    record: &RemoteReplayStateRecord,
    error_message: &str,
) -> Result<bool, AtmError> {
    let updated = finalize_replay_receipt(
        record,
        RemoteDeliveryReceiptStatus::Failed,
        &format!(
            "ATM failed deferred remote delivery because the remote peer returned a terminal error: {error_message}"
        ),
    )?;
    replay_store.delete(&record.team, &record.agent, &record.message_key)?;
    Ok(updated)
}

pub(super) fn retain_replay_record(
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
        peer_addr = %record.peer_addr,
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
