use std::path::Path;

use tracing::{error, info};

use crate::boundary;
use crate::delivery_policy::DeliveryRecipientSnapshot;
use crate::error::AtmError;
use crate::schema::{InboxMessage, clear_transport_delivery_metadata, peer_delivery_target};
use crate::service_runtime::LocalServiceRuntime;
use crate::service_runtime::RetainedServiceRuntime;
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::types::{AgentName, HostName, TeamName};

use super::{DeliveryPersistenceResult, DuplicateWriteDisposition, prepare_threaded_message};

#[cfg(test)]
pub(crate) fn persist_message(
    runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
    home_dir: &Path,
    recipient: &DeliveryRecipientSnapshot,
    inbox_path: &Path,
    envelope: &InboxMessage,
    require_existing_inbox: bool,
    same_store_peer_receipt: Option<(&HostName, &HostName)>,
) -> Result<DeliveryPersistenceResult, AtmError> {
    persist_message_with_ack_update(
        runtime,
        home_dir,
        recipient,
        inbox_path,
        envelope,
        require_existing_inbox,
        same_store_peer_receipt,
        None,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the acknowledgement source replacement is part of the one durable admission commit"
)]
pub(crate) fn persist_message_with_ack_update(
    runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
    home_dir: &Path,
    recipient: &DeliveryRecipientSnapshot,
    inbox_path: &Path,
    envelope: &InboxMessage,
    require_existing_inbox: bool,
    same_store_peer_receipt: Option<(&HostName, &HostName)>,
    acknowledgement_source_update: Option<boundary::Message>,
) -> Result<DeliveryPersistenceResult, AtmError> {
    if require_existing_inbox && !inbox_path.exists() {
        return Ok(DeliveryPersistenceResult::persisted(envelope.clone()));
    }

    let mut prepared = envelope.clone();
    // Ordinary immutable messages have no thread relation to validate. Do not
    // enumerate the recipient mailbox on their admission path; that makes a
    // simple send grow with mailbox size and opens avoidable SQLite readers.
    let inbox_messages = if prepared.parent_message_id.is_some() && prepared.thread_mode.is_some() {
        load_store_backed_mailbox_projection(runtime, home_dir, &recipient.team, &recipient.agent)?
    } else {
        Vec::new()
    };
    prepare_threaded_message(&mut prepared, &inbox_messages)?;

    match mirror_message_to_store(
        runtime,
        home_dir,
        &recipient.team,
        &recipient.agent,
        &prepared,
        same_store_peer_receipt,
        acknowledgement_source_update,
    ) {
        Ok(DuplicateWriteDisposition::NotDuplicate) => {
            Ok(DeliveryPersistenceResult::persisted(prepared))
        }
        Ok(DuplicateWriteDisposition::AlreadyDeliveredRemote) => {
            Ok(DeliveryPersistenceResult::already_persisted(prepared))
        }
        Ok(DuplicateWriteDisposition::SameStorePeerReceipt) => {
            Ok(DeliveryPersistenceResult::same_store_peer_receipt(prepared))
        }
        Err(error) => Err(error),
    }
}

async fn load_store_backed_mailbox_projection_async(
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<InboxMessage>, AtmError> {
    let mut records = runtime
        .list_messages_async(atm_storage::MessageQuery {
            team: team.clone(),
            agent: agent.clone(),
            sender: None,
            task_id: None,
            limit: None,
        })
        .await?;
    records.sort_by(|left, right| {
        left.envelope
            .timestamp
            .cmp(&right.envelope.timestamp)
            .then_with(|| left.message_key.as_ref().cmp(right.message_key.as_ref()))
    });
    Ok(records.into_iter().map(|record| record.envelope).collect())
}

/// Tokio-owned durable admission for ordinary immutable messages.
///
/// Validation and message construction remain in the canonical core path;
/// only the storage transition is asynchronous. The future enqueues exactly
/// one ordered record in the backend-owned write lane and awaits its durable
/// reply, so no Tokio worker waits on SQLite or a blocking bridge.
pub(crate) async fn persist_message_with_async_admission(
    runtime: &LocalServiceRuntime,
    _home_dir: &Path,
    recipient: &DeliveryRecipientSnapshot,
    inbox_path: &Path,
    envelope: &InboxMessage,
    require_existing_inbox: bool,
    same_store_peer_receipt: Option<(&HostName, &HostName)>,
) -> Result<DeliveryPersistenceResult, AtmError> {
    if require_existing_inbox && !inbox_path.exists() {
        return Ok(DeliveryPersistenceResult::persisted(envelope.clone()));
    }

    let mut prepared = envelope.clone();
    let inbox_messages = if prepared.parent_message_id.is_some() && prepared.thread_mode.is_some() {
        load_store_backed_mailbox_projection_async(runtime, &recipient.team, &recipient.agent)
            .await?
    } else {
        Vec::new()
    };
    prepare_threaded_message(&mut prepared, &inbox_messages)?;

    match mirror_message_to_store_async(
        runtime,
        &recipient.team,
        &recipient.agent,
        &prepared,
        same_store_peer_receipt,
    )
    .await
    {
        Ok(DuplicateWriteDisposition::NotDuplicate) => {
            Ok(DeliveryPersistenceResult::persisted(prepared))
        }
        Ok(DuplicateWriteDisposition::AlreadyDeliveredRemote) => {
            Ok(DeliveryPersistenceResult::already_persisted(prepared))
        }
        Ok(DuplicateWriteDisposition::SameStorePeerReceipt) => {
            Ok(DeliveryPersistenceResult::same_store_peer_receipt(prepared))
        }
        Err(error) => Err(error),
    }
}

fn load_store_backed_mailbox_projection(
    runtime: &(impl RetainedMailboxRuntime + ?Sized),
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<InboxMessage>, AtmError> {
    let mut metadata_rows = runtime.query_mailbox_metadata_rows(home_dir, team, agent, None)?;
    metadata_rows.sort_by(|left, right| {
        left.message_at
            .cmp(&right.message_at)
            .then_with(|| left.message_key.as_ref().cmp(right.message_key.as_ref()))
    });

    let mut messages = Vec::with_capacity(metadata_rows.len());
    for row in metadata_rows {
        // Concurrent clear/ack paths can legally delete a row after metadata
        // enumeration but before the compatibility export reloads it. Skip the
        // vanished row and export the current mailbox contents instead of
        // failing the whole send after the write already committed.
        let Some(record) = runtime.load_message_record(home_dir, team, agent, &row.message_key)?
        else {
            continue;
        };
        messages.push(record.envelope);
    }
    Ok(messages)
}

fn mirror_message_to_store(
    runtime: &(impl RetainedMailboxRuntime + ?Sized),
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    envelope: &InboxMessage,
    same_store_peer_receipt: Option<(&HostName, &HostName)>,
    acknowledgement_source_update: Option<boundary::Message>,
) -> Result<DuplicateWriteDisposition, AtmError> {
    let Some(message_id) = envelope.message_id else {
        return Ok(DuplicateWriteDisposition::NotDuplicate);
    };
    let message_key = boundary::MessageKey::from(message_id);
    let record = boundary::Message {
        team: team.clone(),
        agent: agent.clone(),
        message_key: message_key.clone(),
        envelope: envelope.clone(),
    };
    if let Some(source_update) = acknowledgement_source_update {
        if let Some(existing) = runtime.load_message_record(home_dir, team, agent, &message_key)? {
            return classify_existing_message(
                existing,
                envelope,
                message_id,
                team,
                agent,
                same_store_peer_receipt,
            );
        }
        runtime.persist_message_records_atomically(vec![record, source_update])?;
    } else {
        if let Some(existing) = runtime.admit_message_record(home_dir, record)? {
            return classify_existing_message(
                existing,
                envelope,
                message_id,
                team,
                agent,
                same_store_peer_receipt,
            );
        }
    }
    Ok(DuplicateWriteDisposition::NotDuplicate)
}

async fn mirror_message_to_store_async(
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &AgentName,
    envelope: &InboxMessage,
    same_store_peer_receipt: Option<(&HostName, &HostName)>,
) -> Result<DuplicateWriteDisposition, AtmError> {
    let Some(message_id) = envelope.message_id else {
        return Ok(DuplicateWriteDisposition::NotDuplicate);
    };
    let message_key = boundary::MessageKey::from(message_id);
    let record = boundary::Message {
        team: team.clone(),
        agent: agent.clone(),
        message_key,
        envelope: envelope.clone(),
    };
    if let Some(existing) = runtime.save_message_if_absent_async(record).await? {
        return classify_existing_message(
            existing,
            envelope,
            message_id,
            team,
            agent,
            same_store_peer_receipt,
        );
    }
    Ok(DuplicateWriteDisposition::NotDuplicate)
}

fn classify_existing_message(
    existing: boundary::Message,
    envelope: &InboxMessage,
    message_id: crate::schema::AtmMessageId,
    team: &TeamName,
    agent: &AgentName,
    same_store_peer_receipt: Option<(&HostName, &HostName)>,
) -> Result<DuplicateWriteDisposition, AtmError> {
    if immutable_envelopes_match(&existing.envelope, envelope) {
        if let Some((source_host, destination_host)) = same_store_peer_receipt
            && peer_delivery_target(&existing.envelope)?.as_ref() == Some(destination_host)
        {
            info!(
                event = "peer_duplicate_write_skipped",
                message_id = %message_id,
                source_host = %source_host,
                destination_host = %destination_host,
                same_store_peer_receipt = true,
                database_write = "skipped",
                delivery = "continued",
                "peer_duplicate_write_skipped"
            );
            return Ok(DuplicateWriteDisposition::SameStorePeerReceipt);
        }
        info!(
            message_id = %message_id,
            team = %team,
            agent = %agent,
            "duplicate message ULID write matched immutable data; retaining existing record"
        );
        return Ok(DuplicateWriteDisposition::AlreadyDeliveredRemote);
    }
    error!(
        code = %crate::error_codes::AtmErrorCode::MessageIdConflict,
        message_id = %message_id,
        team = %team,
        agent = %agent,
        "duplicate message ULID carried different immutable data; retaining original record"
    );
    Err(AtmError::message_id_conflict(format!(
        "message {message_id} already exists for {agent}@{team} with different immutable data"
    )))
}

fn immutable_envelopes_match(left: &InboxMessage, right: &InboxMessage) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.read = false;
    left.pending_ack_at = None;
    left.acknowledged_at = None;
    right.read = false;
    right.pending_ack_at = None;
    right.acknowledged_at = None;
    clear_transport_delivery_metadata(&mut left);
    clear_transport_delivery_metadata(&mut right);
    left == right
}
