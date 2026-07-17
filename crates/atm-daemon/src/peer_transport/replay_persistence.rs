use std::net::SocketAddr;
use std::sync::Arc;

use atm_core::boundary::{MessageKey, RemoteReplayStateRecord, RemoteReplayStore};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::protocol::{RequestEnvelope, SendRequestEnvelope};
use atm_core::schema::AtmMessageId;
use atm_core::send::{RemoteDeliveryReceiptStatus, finalize_remote_delivery_receipt_with_runtime};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_core::with_default_local_service_runtime;

use super::{PeerTransportConfig, remote_retry_budget_expiry_error};
use crate::peer_transport::client_helpers::replay_metadata_for_request;

pub(super) fn persist_outcome_unknown_request(
    client: &super::PeerClientTransport,
    request: &RequestEnvelope,
) -> Result<(), AtmError> {
    let Some((team, agent, message_key)) = replay_metadata_for_request(request) else {
        tracing::warn!(
            subsystem = "peer_transport",
            action = "persist",
            outcome = "unknown",
            request = ?request,
            "remote delivery outcome is unknown but this request family does not support durable replay persistence",
        );
        return Ok(());
    };
    let endpoint = client
        .endpoint
        .ok_or_else(super::remote_peer_endpoint_not_configured_error)?;
    client.persist_replay_request_to_endpoint(
        endpoint,
        team,
        agent,
        message_key,
        request.clone(),
        None,
        None,
        None,
        None,
        None,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "cross-host retry persistence needs explicit sender and receipt metadata at the peer-transport boundary"
)]
pub(super) fn persist_replay_request(
    config: &PeerTransportConfig,
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
            + chrono::Duration::from_std(config.remote_retry_budget)
                .map_err(remote_retry_budget_expiry_error)?,
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
    let RequestEnvelope::Send(SendRequestEnvelope::Compose(send_request)) = &record.request else {
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
