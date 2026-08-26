//! Send command service implementation and post-send hook handling.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;
use tracing::warn;

use crate::address::AgentAddress;
use crate::boundary;
use crate::caller_context::ActivityObservation;
use crate::delivery_execution::{
    DeliveryTransitionContext, emit_delivery_plan_transitions, execute_delivery_plan,
};
use crate::error::AtmError;
use crate::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use crate::schema::{
    AckIntentFields, AtmMessageId, InboxMessage, ThreadMode, set_authenticated_source_host,
    set_peer_delivery_target,
};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::types::{AgentName, ChatId, HostName, IsoTimestamp, TaskId, TeamName};

pub(crate) mod async_persistence;
mod delivery_persistence;
pub(crate) mod file_policy;
pub(crate) mod hook;
pub mod input;
#[doc(hidden)]
pub(crate) mod nudge_template;
mod outcome;
mod peer_routing;
mod persistence;
mod received_hook;
mod recipient;
mod request;
pub(crate) mod summary;
mod template;
mod write_context;

pub(crate) use delivery_persistence::{
    DeliveryPersistenceDisposition, DeliveryPersistenceResult, DuplicateWriteDisposition,
};
#[doc(hidden)]
pub use nudge_template::{
    default_template, qualified_sender_identity as qualified_nudge_sender_identity,
    render_resolved_built_in_nudge,
};
pub use outcome::{SendCommandOutcome, SendOutcome, WarningEntry};
pub(crate) use peer_routing::direct_peer_destination;
#[cfg(test)]
pub(crate) use persistence::persist_message;
pub(crate) use received_hook::{PreparedReceivedHook, prepare_received_hook};
pub(crate) use recipient::{ResolvedRecipient, resolve_recipient, validate_non_self_recipient};
use request::prepare_threaded_message;
pub(crate) use request::resolve_message_body;
use template::{requires_plain_template_fallback, verify_template_send};
pub(crate) use write_context::{SendExecutionContext, prepare_send_context};
use write_context::{build_send_delivery_plan, build_send_outcome};

// The canonical write pipeline lives in `crate::write`; `send` re-exports the
// public entry points so external paths are unchanged.
pub use crate::write::{
    PreparedWrite, WriteOutcome, prepare_write_with_async_runtime, prepare_write_with_runtime,
    send_mail, send_mail_with_runtime, write_mail, write_mail_with_runtime,
};

#[cfg(test)]
pub(crate) use crate::write::{send_mail_with_runtime_impl, write_mail_with_runtime_impl};

/// Selects when a committed write's receiver nudge is emitted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NudgeMode {
    #[default]
    Immediate,
    Deferred,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SendMessageSource {
    Inline(String),
    File {
        path: PathBuf,
        message: Option<String>,
    },
    /// A self-contained template request. The CLI captures all caller-owned
    /// inputs before the local HTTP hop. The selected daemon owns inspection,
    /// variable merge, verification render, and routing policy.
    Template(TemplateSendSource),
}

/// Transport-safe caller input for a templated send.
///
/// This deliberately carries source bytes and captured environment values,
/// rather than asking the daemon to inspect its environment after the local
/// HTTP request has arrived.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateSendSource {
    pub canonical_template_path: PathBuf,
    pub canonical_template_root: PathBuf,
    pub raw_file_bytes: Vec<u8>,
    /// Caller-provided defaults that sit above frontmatter defaults but below
    /// the explicit compose-time sources. Kept on the request so the daemon
    /// merges a complete, reproducible input snapshot.
    #[serde(default)]
    pub input_defaults: Map<String, Value>,
    pub var_file_values: Map<String, Value>,
    pub explicit_values: Map<String, Value>,
    pub environment_values: Map<String, Value>,
}

/// User-facing message classification. This stays on the canonical write
/// request so ordinary and template-derived sends persist the same metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageClassification {
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub content_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub caller_identity: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller_chat_id: Option<ChatId>,
    pub caller_team: TeamName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_observation: Option<ActivityObservation>,
    /// Set only by the authenticated HTTPS ingress before the shared writer
    /// persists an inbound record. It is not trusted from wire JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authenticated_source_host: Option<HostName>,
    /// The immutable identity assigned by the origin canonical writer.
    /// Authenticated peer ingress preserves it so both hosts store one ULID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_message_id: Option<AtmMessageId>,
    /// The immutable origin timestamp carried with a peer write.  It is set
    /// alongside `origin_message_id` by the canonical origin writer so a
    /// repeated peer delivery compares equal at the receiving store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_timestamp: Option<IsoTimestamp>,
    /// Destination is omitted only by an `atm ack` command.  The daemon
    /// resolves that destination from the acknowledged source before calling
    /// the canonical writer.
    pub to: Option<AgentAddress>,
    pub message_source: SendMessageSource,
    #[serde(default)]
    pub classification: MessageClassification,
    /// Caller-selected payload limit carried across local HTTP so the daemon
    /// applies the exact same inline/stdin policy after transport framing.
    #[serde(default = "input::default_message_max_bytes")]
    pub max_message_bytes: usize,
    pub summary_override: Option<String>,
    pub requires_ack: bool,
    pub task_id: Option<TaskId>,
    pub parent_message_id: Option<AtmMessageId>,
    pub thread_mode: Option<ThreadMode>,
    pub expires_at: Option<crate::types::IsoTimestamp>,
    /// When present this write is an acknowledgement reply.  It otherwise
    /// follows the exact same persistence and post-write path as a send.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledges_message_id: Option<AtmMessageId>,
    pub dry_run: bool,
    /// Whether this write's receiver nudge is emitted immediately or
    /// deferred to durable at-most-once queue delivery.
    #[serde(default)]
    pub nudge_mode: NudgeMode,
}

impl WriteRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        home_dir: PathBuf,
        current_dir: PathBuf,
        caller_identity: AgentName,
        to: &str,
        caller_team: TeamName,
        message_source: SendMessageSource,
        summary_override: Option<String>,
        requires_ack: bool,
        task_id: Option<TaskId>,
        dry_run: bool,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            home_dir,
            current_dir,
            caller_identity,
            caller_chat_id: None,
            caller_team,
            activity_observation: None,
            authenticated_source_host: None,
            origin_message_id: None,
            origin_timestamp: None,
            to: Some(to.parse()?),
            message_source,
            classification: MessageClassification::default(),
            max_message_bytes: input::default_message_max_bytes(),
            summary_override,
            requires_ack,
            task_id,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            acknowledges_message_id: None,
            dry_run,
            nudge_mode: NudgeMode::default(),
        })
    }

    #[must_use]
    pub fn with_caller_chat_id(mut self, caller_chat_id: Option<ChatId>) -> Self {
        self.caller_chat_id = caller_chat_id;
        self
    }

    #[must_use]
    pub fn with_max_message_bytes(mut self, max_message_bytes: usize) -> Self {
        self.max_message_bytes = max_message_bytes;
        self
    }

    #[must_use]
    pub fn with_classification(mut self, classification: MessageClassification) -> Self {
        self.classification = classification;
        self
    }

    /// Select whether this write's receiver nudge is emitted immediately
    /// (default) or deferred to durable at-most-once queue delivery.
    #[must_use]
    pub fn with_nudge_mode(mut self, nudge_mode: NudgeMode) -> Self {
        self.nudge_mode = nudge_mode;
        self
    }

    #[must_use]
    pub fn with_activity_observation(
        mut self,
        activity_observation: Option<ActivityObservation>,
    ) -> Self {
        self.activity_observation = activity_observation;
        self
    }

    #[must_use]
    pub fn with_origin_message_id(mut self, message_id: AtmMessageId) -> Self {
        self.origin_message_id = Some(message_id);
        self
    }

    /// Select the canonical acknowledgement write shape.
    ///
    /// Acknowledgements have no caller-supplied destination and can never ask
    /// for another acknowledgement. They otherwise continue through the same
    /// write pipeline as ordinary sends.
    #[must_use]
    pub fn with_acknowledges_message_id(mut self, message_id: AtmMessageId) -> Self {
        self.to = None;
        self.requires_ack = false;
        self.acknowledges_message_id = Some(message_id);
        self
    }

    #[must_use]
    pub fn with_origin_metadata(
        mut self,
        message_id: AtmMessageId,
        timestamp: IsoTimestamp,
    ) -> Self {
        self.origin_message_id = Some(message_id);
        self.origin_timestamp = Some(timestamp);
        self
    }
}

/// Compatibility name for existing callers.  There is one write payload;
/// acknowledgement is represented by `acknowledges_message_id` on it.
pub type SendRequest = WriteRequest;

/// Selects the owner of non-durable delivery work after a write commits.
///
/// Direct/core callers retain the historical synchronous contract. The
/// replacement runtime selects `Deferred`: it retains all hook-planning data
/// during the async durable write, so its post-commit path never has to reopen
/// the just-written record through a synchronous storage reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryExecutionMode {
    Inline,
    Deferred,
}

pub(crate) fn request_requires_ack(request: &SendRequest, task_id: &Option<TaskId>) -> bool {
    request.requires_ack
        || task_id.is_some()
        || matches!(
            &request.message_source,
            SendMessageSource::File { path, .. } if file_policy::is_task_envelope(path)
        )
}

#[expect(
    clippy::too_many_arguments,
    reason = "Y.6 closeout keeps the explicit send outcome pieces visible at the sprint seam."
)]
pub(crate) fn finalize_send_outcome<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    runtime: &R,
    observability: &dyn ObservabilityPort,
    request: &SendRequest,
    context: &SendExecutionContext,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    requires_ack: bool,
    task_id: Option<TaskId>,
    persistence: &DeliveryPersistenceResult,
    delivery_mode: DeliveryExecutionMode,
) -> Result<SendOutcome, AtmError> {
    let command_outcome = if request.dry_run {
        SendCommandOutcome::DryRun
    } else {
        SendCommandOutcome::Sent
    };
    let mut outcome = build_send_outcome(
        request,
        context,
        body,
        summary,
        message_id,
        requires_ack,
        task_id.clone(),
        command_outcome,
        persistence,
    );
    if !request.dry_run && delivery_mode == DeliveryExecutionMode::Inline {
        let plan = build_send_delivery_plan(context, requires_ack, false, persistence)?;
        let execution = execute_delivery_plan(runtime, None, &plan)?;
        emit_delivery_plan_transitions(
            observability,
            DeliveryTransitionContext {
                family: context.delivery_family,
                team: &context.recipient.team,
                agent: &context.recipient.agent,
                sender: &context.canonical_sender,
                message_id,
                task_id: task_id.clone(),
            },
            &plan,
            &execution,
        )?;
        outcome.warnings.extend(execution.warnings);
    }
    emit_send_command_event(
        observability,
        command_outcome.as_str(),
        &outcome,
        task_id.clone(),
        &context.canonical_sender,
    );
    if looks_like_path_only_body(body, &request.current_dir, &request.home_dir) {
        emit_path_body_detection_event(observability, &outcome, task_id, context);
    }
    Ok(outcome)
}

/// Detect the migration pattern where a rendered file path is sent instead
/// of the file's content. This is deliberately advisory: the message is
/// admitted and retained while telemetry guides callers toward `--template`.
pub(crate) fn looks_like_path_only_body(
    body: &str,
    current_dir: &std::path::Path,
    home_dir: &std::path::Path,
) -> bool {
    if body.len() >= 512 || body.is_empty() || body.trim() != body {
        return false;
    }
    if body.contains(['\n', '\r']) {
        return false;
    }
    let candidate = std::path::Path::new(body);
    let resolved = if body == "~" || body.starts_with("~/") {
        Some(if body == "~" {
            home_dir.to_path_buf()
        } else {
            home_dir.join(body.trim_start_matches("~/"))
        })
    } else if candidate.is_absolute() {
        Some(candidate.to_path_buf())
    } else {
        let from_current = current_dir.join(candidate);
        let from_home = home_dir.join(candidate);
        if from_current.is_file() {
            Some(from_current)
        } else if from_home.is_file() {
            Some(from_home)
        } else {
            None
        }
    };
    let Some(resolved) = resolved else {
        return false;
    };
    // Whitespace is valid in an existing filename, but an unresolvable path
    // containing prose must never become a false positive.
    if body.chars().any(char::is_whitespace) && !resolved.is_file() {
        return false;
    }
    true
}

pub(crate) fn annotate_path_only_body(
    request: &mut SendRequest,
    context: &mut SendExecutionContext,
    body: &str,
) {
    if !looks_like_path_only_body(body, &request.current_dir, &request.home_dir) {
        return;
    }
    // Detection is an admission fact, so it wins over an optional caller
    // label; otherwise explicit labels could hide the migration telemetry.
    request.classification.content_format = Some("path-ref".to_owned());
    context.warnings.push(WarningEntry::new(
        "message body looks like a file path; send file content with `atm send --template <path> --vars <file>` (path retained for compatibility)",
        Some("use `atm compose --template <path>` to preview rendered content"),
    ));
}

fn emit_path_body_detection_event(
    observability: &dyn ObservabilityPort,
    outcome: &SendOutcome,
    task_id: Option<TaskId>,
    context: &SendExecutionContext,
) {
    if let Err(error) = observability.emit(CommandEvent {
        command: "send",
        action: action_name("path_body_detected"),
        outcome: outcome_label("warn"),
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender: context.canonical_sender.clone(),
        message_id: Some(outcome.message_id),
        requires_ack: outcome.requires_ack,
        dry_run: outcome.dry_run,
        task_id,
        error_code: None,
        error_message: Some("content_format=path-ref".to_owned()),
    }) {
        warn!(%error, command = "send", action = "path_body_detected", "failed to emit path-body detection event");
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Send persistence needs the explicit request/body/message envelope fields documented in the Y.4 state-machine seam."
)]
pub(crate) fn persist_send_message<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: &SendRequest,
    context: &SendExecutionContext,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
    requires_ack: bool,
    task_id: Option<TaskId>,
    acknowledgement_source_update: Option<boundary::Message>,
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
    // Origin metadata is assigned only by the canonical origin writer and is
    // required on every peer receipt. It prevents an inbound peer write from
    // becoming a second outbound peer delivery while preserving its original
    // host-qualified address for the shared writer and a later ACK.
    if let Some(destination) = request.to.as_ref()
        && let Some(host) = direct_peer_destination(request, destination)
    {
        set_peer_delivery_target(&mut envelope, &host);
    }
    persistence::persist_message_with_ack_update(
        runtime,
        &request.home_dir,
        &context.delivery_snapshot,
        &context.inbox_path,
        &envelope,
        false,
        request
            .authenticated_source_host
            .as_ref()
            .and_then(|source_host| {
                request
                    .to
                    .as_ref()
                    .and_then(|destination| destination.host())
                    .map(|destination_host| (source_host, destination_host))
            }),
        acknowledgement_source_update,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the immutable envelope is assembled from the canonical write fields"
)]
fn build_send_envelope(
    request: &SendRequest,
    context: &SendExecutionContext,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
    requires_ack: bool,
    task_id: Option<TaskId>,
) -> InboxMessage {
    let ack_intent = AckIntentFields::from_requires_ack(requires_ack, timestamp);
    let mut envelope = InboxMessage {
        from: context.canonical_sender.clone(),
        source_chat_id: request.caller_chat_id.clone(),
        text: body.to_string(),
        timestamp,
        read: false,
        source_team: Some(request.caller_team.clone()),
        destination_chat_id: request
            .to
            .as_ref()
            .and_then(|address| address.chat_id().cloned()),
        summary: Some(summary.to_string()),
        message_id: Some(message_id),
        requires_ack: ack_intent.requires_ack,
        pending_ack_at: ack_intent.pending_ack_at,
        acknowledged_at: ack_intent.acknowledged_at,
        acknowledges_message_id: request.acknowledges_message_id,
        parent_message_id: request.parent_message_id,
        thread_mode: request.thread_mode,
        expires_at: request.expires_at,
        task_id: task_id.clone(),
        extra: Map::new(),
    };
    insert_classification_metadata(&mut envelope, &request.classification);
    set_authenticated_source_host(&mut envelope, request.authenticated_source_host.clone());
    envelope
}

fn request_has_classification(request: &SendRequest) -> bool {
    request_has_classification_from(&request.classification)
}

fn insert_classification_metadata(
    envelope: &mut InboxMessage,
    classification: &MessageClassification,
) {
    if !request_has_classification_from(classification) {
        return;
    }
    envelope.extra.insert(
        "category".to_owned(),
        classification
            .category
            .clone()
            .map_or(serde_json::Value::Null, serde_json::Value::String),
    );
    envelope.extra.insert(
        "tags".to_owned(),
        serde_json::Value::Array(
            classification
                .tags
                .iter()
                .cloned()
                .map(serde_json::Value::String)
                .collect(),
        ),
    );
    envelope.extra.insert(
        "content_format".to_owned(),
        classification
            .content_format
            .clone()
            .map_or(serde_json::Value::Null, serde_json::Value::String),
    );
}

fn request_has_classification_from(classification: &MessageClassification) -> bool {
    classification.category.is_some()
        || !classification.tags.is_empty()
        || classification.content_format.is_some()
}

pub(crate) fn emit_send_command_event(
    observability: &dyn ObservabilityPort,
    outcome_name: &'static str,
    outcome: &SendOutcome,
    task_id: Option<TaskId>,
    canonical_sender: &AgentName,
) {
    if let Err(error) = observability.emit(CommandEvent {
        command: "send",
        action: action_name("send"),
        outcome: outcome_label(outcome_name),
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender: canonical_sender.clone(),
        message_id: Some(outcome.message_id),
        requires_ack: outcome.requires_ack,
        dry_run: outcome.dry_run,
        task_id,
        error_code: None,
        error_message: None,
    }) {
        warn!(%error, command = "send", action = "send", "failed to emit send command event");
    }
}

#[cfg(test)]
mod post_write_tests;
#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod path_body_tests {
    use super::looks_like_path_only_body;

    #[test]
    fn detects_existing_relative_and_absolute_files() {
        let root = tempfile::tempdir().expect("temporary workspace");
        let home = tempfile::tempdir().expect("temporary home");
        let relative = root.path().join("rendered.xml");
        std::fs::write(&relative, "<message />").expect("fixture");

        assert!(looks_like_path_only_body(
            "rendered.xml",
            root.path(),
            home.path()
        ));
        assert!(looks_like_path_only_body(
            &relative.display().to_string(),
            root.path(),
            home.path()
        ));
    }

    #[test]
    fn detects_home_shorthand_but_not_prose_containing_a_path() {
        let root = tempfile::tempdir().expect("temporary workspace");
        let home = tempfile::tempdir().expect("temporary home");
        assert!(looks_like_path_only_body(
            "~/report.xml",
            root.path(),
            home.path()
        ));
        assert!(!looks_like_path_only_body(
            "Please see /tmp/report.xml for details.",
            root.path(),
            home.path()
        ));
    }

    #[test]
    fn rejects_multiline_and_oversized_bodies() {
        let root = tempfile::tempdir().expect("temporary workspace");
        let home = tempfile::tempdir().expect("temporary home");
        assert!(!looks_like_path_only_body(
            "/tmp/report.xml\n",
            root.path(),
            home.path()
        ));
        assert!(!looks_like_path_only_body(
            &"/".repeat(512),
            root.path(),
            home.path()
        ));
    }
}
