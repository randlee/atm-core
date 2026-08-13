//! Tokio durable-admission preparation for the canonical send pipeline.

use tracing::warn;

use super::*;

/// Prepares the canonical write after its one asynchronous durable admission.
pub(super) async fn prepare_persisted_write_async(
    mut request: SendRequest,
    observability: &(dyn ObservabilityPort + Send + Sync),
    runtime: &LocalServiceRuntime,
    acknowledgement: Option<crate::ack::ResolvedAcknowledgement>,
) -> Result<PreparedWrite, AtmError> {
    let mut context = prepare_send_context(runtime, &request)?;
    let task_id = request.task_id.clone();
    let requires_ack = request_requires_ack(&request, &task_id);
    let verified_template = verify_template_request(runtime, &request)?;
    let body = resolve_async_body(&request, &context, verified_template.as_ref())?;
    super::annotate_path_only_body(&mut request, &mut context, &body);
    let summary = summary::build_summary(&body, request.summary_override.clone());
    let message_id = request.origin_message_id.unwrap_or_default();
    let timestamp = request.origin_timestamp.unwrap_or_else(IsoTimestamp::now);
    let persistence = persist_send_message_async(
        runtime,
        &request,
        &context,
        &body,
        &summary,
        message_id,
        timestamp,
        requires_ack,
        task_id.clone(),
        verified_template.as_ref(),
    )
    .await?;
    let received_hook = prepare_received_hook(
        runtime,
        &context,
        &persistence,
        requires_ack,
        acknowledgement.is_some(),
    );
    let outcome = finalize_send_outcome(
        runtime,
        observability,
        &request,
        &context,
        &body,
        &summary,
        message_id,
        requires_ack,
        task_id,
        &persistence,
        DeliveryExecutionMode::Deferred,
    )?;
    Ok(PreparedWrite {
        outcome,
        outbound_request: request,
        persisted_timestamp: timestamp,
        post_write_needed: persistence.requires_post_write(),
        same_store_peer_receipt: persistence.duplicate_disposition
            == DuplicateWriteDisposition::SameStorePeerReceipt,
        received_hook,
        acknowledgement,
    })
}

fn verify_template_request(
    runtime: &LocalServiceRuntime,
    request: &SendRequest,
) -> Result<Option<template::VerifiedTemplateSend>, AtmError> {
    let SendMessageSource::Template(source) = &request.message_source else {
        return Ok(None);
    };
    let composer = runtime.template_composer().ok_or_else(|| {
        AtmError::daemon_unavailable("Tokio template admission was not installed in this runtime")
    })?;
    verify_template_send(composer.as_ref(), source, request.max_message_bytes).map(Some)
}

fn resolve_async_body(
    request: &SendRequest,
    context: &SendExecutionContext,
    verified_template: Option<&template::VerifiedTemplateSend>,
) -> Result<String, AtmError> {
    match (&request.message_source, verified_template) {
        (SendMessageSource::Template(_), Some(verified)) => Ok(verified.rendered.text.clone()),
        (source, None) => resolve_message_body(
            source,
            &request.current_dir,
            &request.home_dir,
            &context.recipient.team,
            request.max_message_bytes,
        ),
        (_, Some(_)) => unreachable!("only template sends produce template verification"),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Send persistence keeps the canonical request/body/message envelope fields explicit at the async seam."
)]
async fn persist_send_message_async(
    runtime: &LocalServiceRuntime,
    request: &SendRequest,
    context: &SendExecutionContext,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
    requires_ack: bool,
    task_id: Option<TaskId>,
    verified_template: Option<&template::VerifiedTemplateSend>,
) -> Result<DeliveryPersistenceResult, AtmError> {
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
    if request.dry_run {
        return Ok(DeliveryPersistenceResult::persisted(envelope));
    }
    set_async_peer_delivery_target(request, &mut envelope);
    let plain_template_fallback = plain_template_fallback(request, context, verified_template);
    warn_template_fallback(verified_template, request, plain_template_fallback);
    if let Some(verified) = verified_template.filter(|_| !plain_template_fallback) {
        return admit_verified_template(
            runtime, request, context, envelope, message_id, timestamp, verified,
        )
        .await;
    }
    persist_plain_async_message(runtime, request, context, &envelope).await
}

fn set_async_peer_delivery_target(request: &SendRequest, envelope: &mut InboxMessage) {
    if let Some(destination) = request.to.as_ref()
        && let Some(host) = direct_peer_destination(request, destination)
    {
        set_peer_delivery_target(envelope, &host);
    }
}

fn plain_template_fallback(
    request: &SendRequest,
    context: &SendExecutionContext,
    verified_template: Option<&template::VerifiedTemplateSend>,
) -> bool {
    verified_template.is_some_and(|verified| {
        requires_plain_template_fallback(
            &verified.inspection,
            &request.caller_team,
            &context.recipient.team,
            direct_peer_destination(request, request.to.as_ref().expect("send target")).is_some(),
        )
    })
}

fn warn_template_fallback(
    verified_template: Option<&template::VerifiedTemplateSend>,
    request: &SendRequest,
    plain_template_fallback: bool,
) {
    if let Some(verified) = verified_template
        && !verified.inspection.include_references.is_empty()
    {
        warn!(
            template_sha = %verified.inspection.sha,
            include_count = verified.inspection.include_references.len(),
            "template includes were confinement-verified and will be persisted as rendered plain text"
        );
    }
    if let Some(verified) = verified_template
        && plain_template_fallback
        && request_has_classification(request)
    {
        warn!(
            template_sha = %verified.inspection.sha,
            "template classification is retained as ordinary-envelope metadata for plain-text fallback"
        );
    }
}

async fn admit_verified_template(
    runtime: &LocalServiceRuntime,
    request: &SendRequest,
    context: &SendExecutionContext,
    envelope: InboxMessage,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
    verified: &template::VerifiedTemplateSend,
) -> Result<DeliveryPersistenceResult, AtmError> {
    let admission =
        build_template_admission(request, context, &envelope, message_id, timestamp, verified)?;
    if let Some(existing) = runtime.admit_template_message_async(admission).await? {
        if existing.envelope != envelope {
            return Err(AtmError::message_id_conflict(format!(
                "message {message_id} already exists with different immutable data"
            )));
        }
        return Ok(DeliveryPersistenceResult::already_persisted(envelope));
    }
    Ok(DeliveryPersistenceResult::persisted(envelope))
}

fn build_template_admission(
    request: &SendRequest,
    context: &SendExecutionContext,
    envelope: &InboxMessage,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
    verified: &template::VerifiedTemplateSend,
) -> Result<atm_storage::TemplateMessageAdmission, AtmError> {
    let template_type = verified
        .inspection
        .frontmatter
        .metadata
        .get("type")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let template_name = verified
        .inspection
        .frontmatter
        .metadata
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    if template_type.is_none() {
        warn!(
            template_sha = %verified.inspection.sha,
            "template metadata.type is absent; catalog registration remains valid but untyped"
        );
    }
    let frontmatter = verified
        .inspection
        .frontmatter
        .clone()
        .with_normalized_workflow_metadata()?;
    let vars = verified.vars.clone().into_storage_json()?;
    let tags = request
        .classification
        .tags
        .iter()
        .cloned()
        .map(atm_storage::InstanceTag::new)
        .collect::<Result<Vec<_>, _>>()?;
    let workflow_snapshot = frontmatter
        .workflow
        .as_ref()
        .map(|workflow| crate::template_workflow::resolve_template_workflow(workflow, &vars))
        .transpose()?;
    let mut admission = atm_storage::TemplateMessageAdmission {
        record: boundary::Message {
            team: context.recipient.team.clone(),
            agent: context.recipient.agent.clone(),
            message_key: boundary::MessageKey::from(message_id),
            envelope: envelope.clone(),
        },
        decomposition: atm_storage::DecomposedMessageAdmission {
            template: atm_storage::TemplateRegistration {
                sha: verified.inspection.sha.clone(),
                template_type,
                template_name,
                content_bytes: verified.source.raw_file_bytes.clone(),
                content_text: std::str::from_utf8(&verified.source.raw_file_bytes)
                    .map_err(|_| AtmError::template_content_not_utf8())?
                    .to_owned(),
                frontmatter,
                first_seen: atm_storage::TemplateFirstSeen::new(
                    timestamp,
                    context.canonical_sender.to_string(),
                )?,
            },
            message: atm_storage::DecomposedMessageRecord {
                key: boundary::MessageKey::from(message_id),
                template_sha: verified.inspection.sha.clone(),
                vars,
                category: request.classification.category.clone(),
                tags,
                content_format: request.classification.content_format.clone(),
                workflow_snapshot,
                tag_provenance: None,
            },
        },
    };
    if let Some(snapshot) = admission.decomposition.message.workflow_snapshot.clone() {
        admission.decomposition.message.tag_provenance =
            Some(admission.decomposition.expected_tag_provenance(&snapshot)?);
    }
    admission.validate()?;
    Ok(admission)
}

async fn persist_plain_async_message(
    runtime: &LocalServiceRuntime,
    request: &SendRequest,
    context: &SendExecutionContext,
    envelope: &InboxMessage,
) -> Result<DeliveryPersistenceResult, AtmError> {
    persistence::persist_message_with_async_admission(
        runtime,
        &request.home_dir,
        &context.delivery_snapshot,
        &context.inbox_path,
        envelope,
        false,
        request
            .authenticated_source_host
            .as_ref()
            .zip(request.to.as_ref().and_then(|recipient| recipient.host())),
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::{Map, json};

    use super::*;
    use crate::delivery_policy::DeliveryRecipientSnapshot;

    fn workflow_frontmatter() -> atm_storage::TemplateFrontmatter {
        atm_storage::TemplateFrontmatter {
            metadata: [
                ("type".to_owned(), json!("task")),
                ("tags".to_owned(), json!(["phase:an"])),
                (
                    "workflow".to_owned(),
                    json!({
                        "scope": { "kind": "sprint", "variable": "sprint" },
                        "state": "dev-start",
                        "stage": "development",
                        "transition": "started",
                        "iteration_variable": "iteration"
                    }),
                ),
            ]
            .into_iter()
            .collect(),
            ..atm_storage::TemplateFrontmatter::default()
        }
    }

    fn context() -> SendExecutionContext {
        let agent: crate::types::AgentName = "receiver".parse().expect("agent");
        let team: crate::types::TeamName = "test-team".parse().expect("team");
        SendExecutionContext {
            recipient: super::ResolvedRecipient {
                agent: agent.clone(),
                team: team.clone(),
            },
            canonical_sender: "sender".parse().expect("sender"),
            inbox_path: PathBuf::from("/tmp/atm-an10-test-inbox"),
            delivery_snapshot: DeliveryRecipientSnapshot::remote(agent, team),
            delivery_family: crate::delivery_policy::DeliveryEventFamily::NewMessage,
            warnings: Vec::new(),
        }
    }

    fn request(source: TemplateSendSource) -> SendRequest {
        WriteRequest::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            "sender".parse().expect("sender"),
            "receiver@test-team",
            "test-team".parse().expect("team"),
            SendMessageSource::Template(source),
            None,
            false,
            None,
            false,
        )
        .expect("request")
        .with_classification(super::MessageClassification {
            category: Some("assignment".to_owned()),
            tags: vec!["audience:engineering".to_owned()],
            content_format: Some("markdown".to_owned()),
        })
    }

    #[test]
    fn template_admission_resolves_and_attaches_workflow_snapshot_before_storage() {
        let raw_file_bytes = b"workflow template".to_vec();
        let source = TemplateSendSource {
            canonical_template_path: PathBuf::from("/tmp/task.j2"),
            canonical_template_root: PathBuf::from("/tmp"),
            raw_file_bytes: raw_file_bytes.clone(),
            input_defaults: Map::new(),
            var_file_values: Map::new(),
            explicit_values: Map::from_iter([
                ("sprint".to_owned(), json!("an-10")),
                ("iteration".to_owned(), json!(2)),
            ]),
            environment_values: Map::new(),
        };
        let frontmatter = workflow_frontmatter();
        let verified = template::VerifiedTemplateSend {
            source: crate::boundary::TemplateSource::file_backed(
                raw_file_bytes,
                PathBuf::from("/tmp/task.j2"),
            ),
            inspection: crate::boundary::TemplateInspection {
                sha: "a".repeat(64).parse().expect("sha"),
                frontmatter: frontmatter.clone(),
                include_references: Vec::new(),
            },
            vars: template::resolve_merged_vars(&frontmatter, &source).expect("merged vars"),
            rendered: crate::boundary::RenderedBody {
                text: "rendered task".to_owned(),
            },
        };
        let request = request(source);
        let context = context();
        let message_id = AtmMessageId::new();
        let timestamp = IsoTimestamp::now();
        let envelope = build_send_envelope(
            &request,
            &context,
            &verified.rendered.text,
            "summary",
            message_id,
            timestamp,
            false,
            None,
        );

        let admission = build_template_admission(
            &request, &context, &envelope, message_id, timestamp, &verified,
        )
        .expect("admission");
        let message = admission.decomposition.message;
        assert_eq!(
            message
                .workflow_snapshot
                .as_ref()
                .expect("snapshot")
                .scope_id
                .as_str(),
            "an-10"
        );
        assert_eq!(
            message
                .workflow_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.iteration.as_ref())
                .expect("iteration")
                .as_str(),
            "2"
        );
        assert!(
            message
                .tag_provenance
                .expect("provenance")
                .effective_tags
                .iter()
                .any(|tag| tag.as_str() == "workflow-state:dev-start")
        );
    }

    #[test]
    fn template_without_workflow_keeps_snapshot_and_provenance_absent() {
        let source = TemplateSendSource {
            canonical_template_path: PathBuf::from("/tmp/plain.j2"),
            canonical_template_root: PathBuf::from("/tmp"),
            raw_file_bytes: b"plain template".to_vec(),
            input_defaults: Map::new(),
            var_file_values: Map::new(),
            explicit_values: Map::new(),
            environment_values: Map::new(),
        };
        let frontmatter = atm_storage::TemplateFrontmatter::default();
        let verified = template::VerifiedTemplateSend {
            source: crate::boundary::TemplateSource::file_backed(
                source.raw_file_bytes.clone(),
                PathBuf::from("/tmp/plain.j2"),
            ),
            inspection: crate::boundary::TemplateInspection {
                sha: "b".repeat(64).parse().expect("sha"),
                frontmatter: frontmatter.clone(),
                include_references: Vec::new(),
            },
            vars: template::resolve_merged_vars(&frontmatter, &source).expect("merged vars"),
            rendered: crate::boundary::RenderedBody {
                text: "plain rendered".to_owned(),
            },
        };
        let request = request(source);
        let context = context();
        let message_id = AtmMessageId::new();
        let timestamp = IsoTimestamp::now();
        let envelope = build_send_envelope(
            &request,
            &context,
            &verified.rendered.text,
            "summary",
            message_id,
            timestamp,
            false,
            None,
        );
        let message = build_template_admission(
            &request, &context, &envelope, message_id, timestamp, &verified,
        )
        .expect("admission")
        .decomposition
        .message;
        assert!(message.workflow_snapshot.is_none());
        assert!(message.tag_provenance.is_none());
    }
}
