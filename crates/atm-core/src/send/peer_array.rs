//! Atomic preparation for authenticated inbound peer message arrays.

use std::collections::HashSet;

use crate::boundary;
use crate::error::AtmError;
use crate::observability::ObservabilityPort;
use crate::provenance::{ValidatedWriteProvenance, WriteIngress, validate_write_provenance};
use crate::schema::AtmMessageId;
use crate::service_runtime::RetainedServiceRuntime;
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::types::{IsoTimestamp, TaskId};

#[cfg(test)]
use super::outcome::post_send_messages_from_persistence;
use super::outcome::{SendExecutionContext, build_send_envelope};
use super::{
    DeliveryExecutionMode, DeliveryPersistenceResult, DuplicateWriteDisposition, PreparedWrite,
    SendMessageSource, WriteRequest, finalize_send_outcome, prepare_send_context,
    prepare_threaded_message, resolve_message_body,
};

struct PendingPeerWrite {
    request: WriteRequest,
    context: SendExecutionContext,
    body: String,
    summary: String,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
    requires_ack: bool,
    task_id: Option<TaskId>,
    persistence: DeliveryPersistenceResult,
}

struct ValidatedArrayRequest {
    message_id: AtmMessageId,
    provenance: ValidatedWriteProvenance,
}

pub(super) fn prepare_with_runtime<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    requests: Vec<WriteRequest>,
    observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<Vec<PreparedWrite>, AtmError> {
    if requests.is_empty() {
        return Err(AtmError::validation_with_recovery(
            "peer message array must contain at least one write",
            "submit a non-empty messages[] array through authenticated peer ingress",
        ));
    }

    let mut seen_origin_ids = HashSet::with_capacity(requests.len());
    let mut pending = Vec::with_capacity(requests.len());
    let mut records = Vec::with_capacity(requests.len());
    for request in requests {
        pending.push(prepare_one(
            runtime,
            request,
            &mut seen_origin_ids,
            &mut records,
        )?);
    }

    // This is the only durable operation for new records in the accepted
    // array. Validation and duplicate conflict checks complete before it
    // begins, so an error cannot leave a newly accepted subset behind.
    runtime.admit_message_records_atomically(records)?;
    pending
        .into_iter()
        .map(|pending| finalize_pending(runtime, observability, pending))
        .collect()
}

fn prepare_one<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: WriteRequest,
    seen_origin_ids: &mut HashSet<AtmMessageId>,
    records: &mut Vec<boundary::Message>,
) -> Result<PendingPeerWrite, AtmError> {
    let validated = validate_array_request(&request, seen_origin_ids)?;
    let context = prepare_send_context(runtime, &request, validated.provenance)?;
    let task_id = request.task_id.clone();
    let requires_ack = request.requires_ack
        || task_id.is_some()
        || matches!(
            &request.message_source,
            SendMessageSource::File { path, .. } if super::file_policy::is_task_envelope(path)
        );
    let body = resolve_message_body(
        &request.message_source,
        &request.current_dir,
        &request.home_dir,
        &context.recipient.team,
    )?;
    let summary = super::summary::build_summary(&body, request.summary_override.clone());
    let timestamp = request.origin_timestamp.unwrap_or_else(IsoTimestamp::now);
    let envelope = build_array_envelope(
        runtime,
        &request,
        &context,
        &body,
        &summary,
        validated.message_id,
        timestamp,
        requires_ack,
        task_id.clone(),
    )?;
    let persistence = classify_or_stage(
        runtime,
        &request,
        &context,
        envelope,
        validated.message_id,
        records,
    )?;
    Ok(PendingPeerWrite {
        request,
        context,
        body,
        summary,
        message_id: validated.message_id,
        timestamp,
        requires_ack,
        task_id,
        persistence,
    })
}

fn validate_array_request(
    request: &WriteRequest,
    seen_origin_ids: &mut HashSet<AtmMessageId>,
) -> Result<ValidatedArrayRequest, AtmError> {
    let provenance = validate_write_provenance(WriteIngress::Peer, request.provenance())?;
    if request.acknowledges_message_id.is_some() {
        return Err(AtmError::validation_with_recovery(
            "peer message arrays cannot contain acknowledgement writes",
            "send a direct one-item acknowledgement request instead of placing it in messages[]",
        ));
    }
    if request.dry_run {
        return Err(AtmError::validation_with_recovery(
            "peer message arrays cannot contain dry-run writes",
            "submit a durable peer write with dry_run set to false",
        ));
    }
    let message_id = request.origin_message_id.ok_or_else(|| {
        AtmError::validation_with_recovery(
            "authenticated peer write is missing an origin message ID",
            "preserve the immutable origin_message_id and origin_timestamp supplied by the origin writer",
        )
    })?;
    if !seen_origin_ids.insert(message_id) {
        return Err(AtmError::validation_with_recovery(
            format!("peer message array contains duplicate origin message ID {message_id}"),
            "remove duplicate origin_message_id values and resubmit the complete array",
        ));
    }
    Ok(ValidatedArrayRequest {
        message_id,
        provenance,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "array admission keeps the canonical immutable envelope fields explicit"
)]
fn build_array_envelope<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: &WriteRequest,
    context: &SendExecutionContext,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
    requires_ack: bool,
    task_id: Option<TaskId>,
) -> Result<crate::schema::InboxMessage, AtmError> {
    let mut envelope = build_send_envelope(
        request,
        context,
        body,
        summary,
        message_id,
        timestamp,
        requires_ack,
        task_id,
    );
    if envelope.parent_message_id.is_some() && envelope.thread_mode.is_some() {
        let inbox_messages = super::persistence::load_store_backed_mailbox_projection(
            runtime,
            &request.home_dir,
            &context.recipient.team,
            &context.recipient.agent,
        )?;
        prepare_threaded_message(&mut envelope, &inbox_messages)?;
    }
    Ok(envelope)
}

fn classify_or_stage<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: &WriteRequest,
    context: &SendExecutionContext,
    envelope: crate::schema::InboxMessage,
    message_id: AtmMessageId,
    records: &mut Vec<boundary::Message>,
) -> Result<DeliveryPersistenceResult, AtmError> {
    let record = boundary::Message {
        team: context.recipient.team.clone(),
        agent: context.recipient.agent.clone(),
        message_key: boundary::MessageKey::from(message_id),
        envelope: envelope.clone(),
    };
    let existing = runtime.load_message_record(
        &request.home_dir,
        &context.recipient.team,
        &context.recipient.agent,
        &record.message_key,
    )?;
    let Some(existing) = existing else {
        records.push(record);
        return Ok(DeliveryPersistenceResult::persisted(envelope));
    };
    match super::persistence::classify_existing_message(
        existing,
        &envelope,
        message_id,
        &context.recipient.team,
        &context.recipient.agent,
        peer_hosts(request),
    )? {
        DuplicateWriteDisposition::AlreadyDeliveredRemote => {
            Ok(DeliveryPersistenceResult::already_persisted(envelope))
        }
        DuplicateWriteDisposition::SameStorePeerReceipt => {
            Ok(DeliveryPersistenceResult::same_store_peer_receipt(envelope))
        }
        DuplicateWriteDisposition::NotDuplicate => unreachable!("existing record is duplicate"),
    }
}

fn peer_hosts(
    request: &WriteRequest,
) -> Option<(&crate::types::HostName, &crate::types::HostName)> {
    request
        .authenticated_source_host
        .as_ref()
        .and_then(|source_host| {
            request
                .to
                .as_ref()
                .and_then(|destination| destination.host())
                .map(|destination_host| (source_host, destination_host))
        })
}

fn finalize_pending<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    observability: &dyn ObservabilityPort,
    pending: PendingPeerWrite,
) -> Result<PreparedWrite, AtmError> {
    let outcome = finalize_send_outcome(
        runtime,
        observability,
        &pending.request,
        &pending.context,
        &pending.body,
        &pending.summary,
        pending.message_id,
        pending.requires_ack,
        pending.task_id.clone(),
        &pending.persistence,
        DeliveryExecutionMode::Deferred,
    )?;
    Ok(PreparedWrite {
        outcome,
        outbound_request: pending.request,
        persisted_timestamp: pending.timestamp,
        post_write_needed: pending.persistence.requires_post_write(),
        same_store_peer_receipt: pending.persistence.duplicate_disposition
            == DuplicateWriteDisposition::SameStorePeerReceipt,
        #[cfg(test)]
        post_write: super::LocalPostWrite {
            post_send_config: pending.context.post_send_config,
            recipient: pending.context.recipient,
            delivery_snapshot: pending.context.delivery_snapshot,
            messages: post_send_messages_from_persistence(
                &pending.persistence,
                pending.requires_ack,
                false,
            )?,
        },
        acknowledgement: None,
    })
}
