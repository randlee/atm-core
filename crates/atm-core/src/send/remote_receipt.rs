use std::path::Path;

use serde_json::Map;

use crate::address::AgentAddress;
use crate::boundary;
use crate::error::AtmError;
use crate::schema::{AckIntentFields, AtmMessageId, InboxMessage};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteDeliveryReceiptStatus {
    Deferred,
    Delivered,
    Failed,
}

#[expect(
    clippy::too_many_arguments,
    reason = "remote-delivery receipt persistence keeps explicit sender/target metadata at the boundary seam"
)]
pub fn persist_remote_delivery_receipt_with_runtime(
    runtime: &LocalServiceRuntime,
    home_dir: &Path,
    sender_team: &TeamName,
    sender_agent: &AgentName,
    receipt_message_id: AtmMessageId,
    target: &AgentAddress,
    remote_host: &str,
    task_id: Option<TaskId>,
    body: &str,
) -> Result<InboxMessage, AtmError> {
    let envelope = build_receipt_message(
        receipt_message_id,
        target,
        remote_host,
        task_id,
        body,
        RemoteDeliveryReceiptStatus::Deferred,
    );
    persist_receipt_message(runtime, home_dir, sender_team, sender_agent, &envelope)?;
    Ok(envelope)
}

#[expect(
    clippy::too_many_arguments,
    reason = "remote-delivery receipt updates need explicit sender/target/result metadata for final replay resolution"
)]
pub fn finalize_remote_delivery_receipt_with_runtime(
    runtime: &LocalServiceRuntime,
    home_dir: &Path,
    sender_team: &TeamName,
    sender_agent: &AgentName,
    receipt_message_id: AtmMessageId,
    target: &AgentAddress,
    remote_host: &str,
    task_id: Option<TaskId>,
    body: &str,
    status: RemoteDeliveryReceiptStatus,
) -> Result<InboxMessage, AtmError> {
    let envelope = build_receipt_message(
        receipt_message_id,
        target,
        remote_host,
        task_id,
        body,
        status,
    );
    persist_receipt_message(runtime, home_dir, sender_team, sender_agent, &envelope)?;
    Ok(envelope)
}

fn build_receipt_message(
    receipt_message_id: AtmMessageId,
    target: &AgentAddress,
    remote_host: &str,
    task_id: Option<TaskId>,
    body: &str,
    status: RemoteDeliveryReceiptStatus,
) -> InboxMessage {
    let summary = match status {
        RemoteDeliveryReceiptStatus::Deferred => {
            format!(
                "ATM deferred remote delivery to {} via {}",
                target, remote_host
            )
        }
        RemoteDeliveryReceiptStatus::Delivered => {
            format!(
                "ATM delivered deferred remote message to {} via {}",
                target, remote_host
            )
        }
        RemoteDeliveryReceiptStatus::Failed => {
            format!(
                "ATM failed deferred remote delivery to {} via {}",
                target, remote_host
            )
        }
    };
    let ack = AckIntentFields::not_required();
    InboxMessage {
        from: AgentName::from_validated("atm-system"),
        text: body.to_string(),
        timestamp: IsoTimestamp::now(),
        read: false,
        source_team: target.team.clone(),
        summary: Some(summary),
        message_id: Some(receipt_message_id),
        requires_ack: ack.requires_ack,
        pending_ack_at: ack.pending_ack_at,
        acknowledged_at: ack.acknowledged_at,
        acknowledges_message_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id,
        extra: Map::new(),
    }
}

fn persist_receipt_message(
    runtime: &LocalServiceRuntime,
    home_dir: &Path,
    sender_team: &TeamName,
    sender_agent: &AgentName,
    envelope: &InboxMessage,
) -> Result<(), AtmError> {
    let message_id = envelope.message_id.ok_or_else(|| {
        AtmError::mailbox_write("remote delivery receipt is missing a message id")
    })?;
    let inbox_path = runtime.inbox_path(home_dir, sender_team, sender_agent)?;
    let _ = inbox_path;
    let message_key = boundary::MessageKey::from(message_id);
    runtime.persist_message_record(boundary::Message {
        team: sender_team.clone(),
        agent: sender_agent.clone(),
        message_key: message_key.clone(),
        envelope: envelope.clone(),
    })?;
    runtime.persist_message_state(boundary::MailMessageState {
        team: sender_team.clone(),
        agent: sender_agent.clone(),
        actor: sender_agent.clone(),
        message_key,
        read: envelope.read,
        pending_ack_at: envelope.pending_ack_at,
        acknowledged_at: envelope.acknowledged_at,
        expires_at: envelope.expires_at,
        deleted_at: None,
        updated_at: Some(IsoTimestamp::now()),
    })
}
