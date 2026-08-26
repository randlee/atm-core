//! The sealed acknowledgement admission pipeline: atomic acknowledgement
//! construction, admission, and reply-target resolution.

use super::*;

/// Immutable acknowledgement outcome assembled by the sealed storage
/// transaction, then completed through the canonical post-write pipeline.
#[derive(Clone)]
pub(crate) struct ResolvedAcknowledgement {
    actor: AgentName,
    team: TeamName,
    reply_target: ReplyTarget,
    reply_text: String,
    acknowledged_message_id: AtmMessageId,
    source_task_id: Option<TaskId>,
}

/// Reply data retained only long enough to form the post-commit route and
/// acknowledgement response.  The source itself is loaded by the storage
/// writer, never by the application-layer admission path.
#[derive(Clone)]
pub(crate) struct AtomicAcknowledgementWrite {
    pub(crate) reply: StoredMessage,
    pub(crate) canonical_request: SendRequest,
    pub(crate) acknowledgement: ResolvedAcknowledgement,
}

#[derive(Clone)]
enum AtomicAcknowledgementKind {
    Local(Box<AckRequest>),
    Received(Box<SendRequest>),
}

struct AtomicAcknowledgementBuilder {
    kind: AtomicAcknowledgementKind,
    // The storage callback requires `&self`; this narrow mutex publishes one
    // immutable reply assembled exactly once across that callback boundary.
    built: Mutex<Option<AtomicAcknowledgementWrite>>,
}

impl AtomicAcknowledgementBuilder {
    fn new(kind: AtomicAcknowledgementKind) -> Self {
        Self {
            kind,
            built: Mutex::new(None),
        }
    }

    fn take(&self) -> Result<AtomicAcknowledgementWrite, AtmError> {
        self.built
            .lock()
            .map_err(|_| {
                AtmError::daemon_unavailable("acknowledgement builder state lock poisoned")
            })?
            .take()
            .ok_or_else(|| {
                AtmError::daemon_unavailable(
                    "acknowledgement transaction completed without a reply",
                )
            })
    }
}

impl AcknowledgementReplyBuilder for AtomicAcknowledgementBuilder {
    fn build_reply(&self, source: &StoredMessage) -> Result<StoredMessage, AtmError> {
        let built = match &self.kind {
            AtomicAcknowledgementKind::Local(request) => {
                let actor = request.caller_identity.clone();
                let team = request.caller_team.clone();
                let reply_target = reply_target_from_source(&source.envelope, &team)?;
                let canonical_request = canonical_ack_write_request(
                    request,
                    &actor,
                    &team,
                    &reply_target,
                    &boundary::Message {
                        team: source.team.clone(),
                        agent: source.agent.clone(),
                        message_key: source.message_key.clone(),
                        envelope: source.envelope.clone(),
                    },
                )?;
                build_atomic_acknowledgement(
                    canonical_request,
                    actor,
                    team,
                    reply_target,
                    request.reply_body.clone(),
                    request.message_id,
                    source.envelope.task_id.clone(),
                )?
            }
            AtomicAcknowledgementKind::Received(request) => {
                let target = request.to.clone().ok_or_else(|| {
                    AtmError::validation("received peer acknowledgement is missing a destination")
                })?;
                let actor = target.agent().clone();
                let team = target
                    .team()
                    .cloned()
                    .unwrap_or_else(|| request.caller_team.clone());
                let reply_target =
                    ReplyTarget::new(actor.clone(), team.clone(), target.host().cloned());
                let message_id = request.acknowledges_message_id.ok_or_else(|| {
                    AtmError::validation("acknowledgement write is missing acknowledges_message_id")
                })?;
                let reply_text = match &request.message_source {
                    SendMessageSource::Inline(value) => value.clone(),
                    SendMessageSource::File { .. } | SendMessageSource::Template(_) => {
                        return Err(AtmError::validation(
                            "acknowledgement reply body must be inline",
                        ));
                    }
                };
                build_atomic_acknowledgement(
                    *request.clone(),
                    actor,
                    team,
                    reply_target,
                    reply_text,
                    message_id,
                    source.envelope.task_id.clone(),
                )?
            }
        };
        let mut slot = self.built.lock().map_err(|_| {
            AtmError::daemon_unavailable("acknowledgement builder state lock poisoned")
        })?;
        *slot = Some(built.clone());
        Ok(built.reply)
    }
}

pub(crate) fn admit_acknowledgement_write<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: SendRequest,
    runtime: &R,
) -> Result<AtomicAcknowledgementWrite, AtmError> {
    let provenance = validate_write_provenance(
        if request.to.is_some() {
            WriteIngress::Peer
        } else {
            WriteIngress::Canonical
        },
        WriteProvenance {
            target_host: request.to.as_ref().and_then(|address| address.host()),
            authenticated_source_host: request.authenticated_source_host.as_ref(),
            origin_message_id: request.origin_message_id.is_some(),
            origin_timestamp: request.origin_timestamp.is_some(),
        },
    )?;
    let (source, builder) = if let Some(target) = request.to.as_ref() {
        if !provenance.is_authenticated_peer() {
            return Err(AtmError::validation(
                "acknowledgement write must not include a client-supplied destination",
            ));
        }
        let message_id = request.acknowledges_message_id.ok_or_else(|| {
            AtmError::validation("acknowledgement write is missing acknowledges_message_id")
        })?;
        let team = target
            .team()
            .cloned()
            .unwrap_or_else(|| request.caller_team.clone());
        (
            AcknowledgementSource {
                team,
                agent: target.agent().clone(),
                message_id,
            },
            Arc::new(AtomicAcknowledgementBuilder::new(
                AtomicAcknowledgementKind::Received(Box::new(request)),
            )),
        )
    } else {
        let request = AckRequest::from_unresolved_write(request)?;
        ensure_roster_member_exists(
            runtime,
            &request.caller_team,
            &request.caller_identity,
            "Repair or reload the ATM roster before retrying `atm ack`.",
        )?;
        (
            AcknowledgementSource {
                team: request.caller_team.clone(),
                agent: request.caller_identity.clone(),
                message_id: request.message_id,
            },
            Arc::new(AtomicAcknowledgementBuilder::new(
                AtomicAcknowledgementKind::Local(Box::new(request)),
            )),
        )
    };
    let _commit = runtime.acknowledge_message_atomically(&source, builder.clone())?;
    builder.take()
}

/// Async counterpart of [`admit_acknowledgement_write`] for the replacement
/// Tokio daemon. The roster check remains synchronous core validation; the
/// source lookup, reply creation, and atomic source transition are one await
/// on the storage-owned durable-admission lane.
pub(crate) async fn admit_acknowledgement_write_async(
    request: SendRequest,
    runtime: &LocalServiceRuntime,
) -> Result<AtomicAcknowledgementWrite, AtmError> {
    let provenance = validate_write_provenance(
        if request.to.is_some() {
            WriteIngress::Peer
        } else {
            WriteIngress::Canonical
        },
        WriteProvenance {
            target_host: request.to.as_ref().and_then(|address| address.host()),
            authenticated_source_host: request.authenticated_source_host.as_ref(),
            origin_message_id: request.origin_message_id.is_some(),
            origin_timestamp: request.origin_timestamp.is_some(),
        },
    )?;
    let (source, builder) = if let Some(target) = request.to.as_ref() {
        if !provenance.is_authenticated_peer() {
            return Err(AtmError::validation(
                "acknowledgement write must not include a client-supplied destination",
            ));
        }
        let message_id = request.acknowledges_message_id.ok_or_else(|| {
            AtmError::validation("acknowledgement write is missing acknowledges_message_id")
        })?;
        let team = target
            .team()
            .cloned()
            .unwrap_or_else(|| request.caller_team.clone());
        (
            AcknowledgementSource {
                team,
                agent: target.agent().clone(),
                message_id,
            },
            Arc::new(AtomicAcknowledgementBuilder::new(
                AtomicAcknowledgementKind::Received(Box::new(request)),
            )),
        )
    } else {
        let request = AckRequest::from_unresolved_write(request)?;
        ensure_roster_member_exists(
            runtime,
            &request.caller_team,
            &request.caller_identity,
            "Repair or reload the ATM roster before retrying `atm ack`.",
        )?;
        (
            AcknowledgementSource {
                team: request.caller_team.clone(),
                agent: request.caller_identity.clone(),
                message_id: request.message_id,
            },
            Arc::new(AtomicAcknowledgementBuilder::new(
                AtomicAcknowledgementKind::Local(Box::new(request)),
            )),
        )
    };
    let _commit = runtime
        .acknowledge_message_atomically_async(source, builder.clone())
        .await?;
    builder.take()
}

pub(crate) fn build_atomic_acknowledgement(
    canonical_request: SendRequest,
    actor: AgentName,
    team: TeamName,
    reply_target: ReplyTarget,
    reply_text: String,
    acknowledged_message_id: AtmMessageId,
    source_task_id: Option<TaskId>,
) -> Result<AtomicAcknowledgementWrite, AtmError> {
    if canonical_request.authenticated_source_host.is_none()
        && reply_target.host.is_none()
        && actor == reply_target.agent
        && team == reply_target.team
    {
        return Err(AtmError::self_addressed_send_invalid(format!(
            "self-addressed messages are invalid ATM input: '{actor}@{team}' may not send to itself"
        )));
    }
    let destination = canonical_request.to.as_ref().ok_or_else(|| {
        AtmError::validation("acknowledgement reply is missing a canonical destination")
    })?;
    let message_id = canonical_request.origin_message_id.unwrap_or_default();
    let timestamp = canonical_request
        .origin_timestamp
        .unwrap_or_else(IsoTimestamp::now);
    let summary = crate::send::summary::build_summary(&reply_text, None);
    let mut envelope = InboxMessage {
        from: actor.clone(),
        source_chat_id: canonical_request.caller_chat_id.clone(),
        text: reply_text.clone(),
        timestamp,
        read: false,
        source_team: Some(team.clone()),
        destination_chat_id: destination.chat_id().cloned(),
        summary: Some(summary.clone()),
        message_id: Some(message_id),
        requires_ack: false,
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: Some(acknowledged_message_id),
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: None,
        extra: serde_json::Map::new(),
    };
    persist_direct_peer_target(&canonical_request, destination, &mut envelope);
    let reply = StoredMessage {
        team: destination.team().cloned().ok_or_else(|| {
            AtmError::validation("acknowledgement reply destination is missing a team")
        })?,
        agent: destination.agent().clone(),
        message_key: MessageKey::from(message_id),
        envelope,
    };
    let acknowledgement = ResolvedAcknowledgement {
        actor,
        team,
        reply_target,
        reply_text,
        acknowledged_message_id,
        source_task_id,
    };
    Ok(AtomicAcknowledgementWrite {
        reply,
        canonical_request,
        acknowledgement,
    })
}

fn persist_direct_peer_target(
    canonical_request: &SendRequest,
    destination: &crate::address::AgentAddress,
    envelope: &mut InboxMessage,
) {
    if let Some(host) = crate::send::direct_peer_destination(canonical_request, destination) {
        crate::schema::set_peer_delivery_target(envelope, &host);
    }
}

fn reply_target_from_source(
    source: &InboxMessage,
    current_team: &TeamName,
) -> Result<ReplyTarget, AtmError> {
    let team = source
        .source_team
        .clone()
        .unwrap_or_else(|| current_team.clone());
    let agent = crate::threading::canonical_sender_identity(source);
    Ok(ReplyTarget::new(agent, team, reply_target_host(source)?))
}

impl ResolvedAcknowledgement {
    pub(crate) fn source_task_id(&self) -> Option<TaskId> {
        self.source_task_id.clone()
    }

    /// Receiver-side acknowledgement mutation occurs only after the canonical
    /// write succeeded. A failed write leaves the source pending.
    pub(crate) fn finish<R: RetainedMailboxRuntime>(
        self,
        _runtime: &R,
        observability: &dyn ObservabilityPort,
        send_outcome: SendOutcome,
    ) -> Result<AckOutcome, AtmError> {
        let outcome = AckOutcome {
            action: CommandAction::Ack,
            team: self.team.clone(),
            agent: self.actor.clone(),
            message_id: self.acknowledged_message_id,
            task_id: self.source_task_id.clone(),
            reply_disposition: AckReplyDisposition::Sent {
                reply_message_id: send_outcome.message_id,
                reply_target: self.reply_target,
            },
            reply_text: self.reply_text,
            warnings: send_outcome.warnings,
        };
        record_ack_telemetry(
            observability,
            &self.actor,
            self.team,
            outcome.message_id,
            outcome.task_id.clone(),
        );
        Ok(outcome)
    }
}

pub(crate) fn canonical_ack_write_request(
    request: &AckRequest,
    actor: &AgentName,
    team: &TeamName,
    target: &ReplyTarget,
    source: &boundary::Message,
) -> Result<SendRequest, AtmError> {
    Ok(SendRequest {
        home_dir: request.home_dir.clone(),
        current_dir: request.current_dir.clone(),
        caller_identity: actor.clone(),
        caller_chat_id: request.caller_chat_id.clone(),
        caller_team: team.clone(),
        activity_observation: request.activity_observation.clone(),
        authenticated_source_host: None,
        origin_message_id: None,
        origin_timestamp: None,
        to: Some(crate::address::AgentAddress::new(
            target.agent.clone(),
            source.envelope.source_chat_id.clone(),
            Some(target.team.clone()),
            target.host.clone(),
        )?),
        message_source: SendMessageSource::Inline(request.reply_body.clone()),
        classification: crate::send::MessageClassification::default(),
        max_message_bytes: crate::send::input::default_message_max_bytes(),
        summary_override: None,
        requires_ack: false,
        task_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        acknowledges_message_id: Some(request.message_id),
        dry_run: false,
    })
}

fn ensure_roster_member_exists<R: RetainedServiceRuntime>(
    runtime: &R,
    team: &TeamName,
    agent: &AgentName,
    recovery: &str,
) -> Result<(), AtmError> {
    if runtime.load_roster_member(team, agent)?.is_none() {
        return Err(AtmError::new(
            crate::error_codes::AtmErrorCode::AgentNotFound,
            format!("agent '{agent}' was not found in team '{team}'\n  Recovery: {recovery}"),
        ));
    }
    Ok(())
}

pub(crate) fn reply_target_host(
    source: &InboxMessage,
) -> Result<Option<crate::types::HostName>, AtmError> {
    let authenticated = authenticated_source_host(source)?;
    let outbound = peer_delivery_target(source)?;
    let validated = validate_write_provenance(
        WriteIngress::Canonical,
        WriteProvenance {
            target_host: outbound.as_ref(),
            authenticated_source_host: authenticated.as_ref(),
            origin_message_id: authenticated.is_some(),
            origin_timestamp: authenticated.is_some(),
        },
    )?;
    Ok(validated
        .is_authenticated_peer()
        .then_some(authenticated)
        .flatten()
        .or(outbound))
}

fn record_ack_telemetry(
    observability: &dyn ObservabilityPort,
    actor: &AgentName,
    team: TeamName,
    message_id: AtmMessageId,
    task_id: Option<TaskId>,
) {
    if let Err(error) = observability.emit(CommandEvent {
        command: "ack",
        action: action_name("ack"),
        outcome: outcome_label("ok"),
        team,
        agent: actor.clone(),
        sender: actor.clone(),
        message_id: Some(message_id),
        requires_ack: false,
        dry_run: false,
        task_id,
        error_code: None,
        error_message: None,
    }) {
        tracing::warn!(%error, command = "ack", "failed to emit acknowledgement telemetry");
    }
}
