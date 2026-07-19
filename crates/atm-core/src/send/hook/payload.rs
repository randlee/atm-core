use super::*;
use crate::schema::remote_host as message_remote_host;
use crate::send::qualified_sender_origin;

pub(super) struct HookExecution {
    pub(super) command_path: PathBuf,
    pub(super) argv: Vec<String>,
    pub(super) payload: Value,
}

pub(super) fn prepare_post_send_hook_execution(
    config: &AtmConfig,
    rule: &config::types::PostSendHookRule,
    event: &PostSendHookEvent,
) -> Option<HookExecution> {
    let mut argv = rule.command.iter();
    let command_path = resolve_command_path(config, argv.next()?);
    Some(HookExecution {
        command_path,
        argv: argv.cloned().collect(),
        payload: post_send_hook_payload(event),
    })
}

fn post_send_hook_payload(event: &PostSendHookEvent) -> Value {
    let mut payload = json!({
        "from": qualified_sender_origin(
            &event.sender,
            Some(&event.sender_team),
            event.remote_host.as_deref(),
        ),
        "to": qualified_sender_identity(&event.recipient, Some(&event.recipient_team)),
        "sender": event.sender.as_str(),
        "recipient": event.recipient.as_str(),
        "team": event.recipient_team.as_str(),
        "message_id": event.message_id.to_string(),
        "description": event.description,
        "message": event.description,
        "requires_ack": event.requires_ack,
        "is_ack": event.is_ack,
    });
    if let Some(remote_host) = &event.remote_host {
        payload["remote_host"] = Value::String(remote_host.clone());
    }
    if let Some(task_id) = &event.task_id {
        payload["task_id"] = Value::String(task_id.to_string());
    }
    if let Some(recipient_pane_id) = &event.recipient_pane_id {
        payload["recipient_pane_id"] = Value::String(recipient_pane_id.to_string());
    }
    payload
}

pub(super) fn resolve_command_path(config: &config::AtmConfig, command_path: &str) -> PathBuf {
    let path = PathBuf::from(command_path);
    if path.is_absolute() || !config::discovery::command_looks_like_path(command_path) {
        path
    } else {
        config.config_root.join(path)
    }
}

pub(super) fn hook_matches_recipient(
    configured: &HookRecipient,
    candidate: &crate::types::AgentName,
) -> bool {
    configured.matches(candidate)
}

pub(super) fn notification_event(event: &PostSendHookEvent) -> NotificationEvent {
    NotificationEvent {
        kind: NotificationKind::Delivery,
        detail: serde_json::to_string(&json!({
            "sender": event.sender.as_str(),
            "sender_team": event.sender_team.as_str(),
            "message_id": event.message_id.to_string(),
            "description": event.description,
            "remote_host": event.remote_host,
            "requires_ack": event.requires_ack,
            "is_ack": event.is_ack,
            "task_id": event.task_id.as_ref().map(ToString::to_string),
            "recipient_pane_id": event.recipient_pane_id.as_ref().map(ToString::to_string),
        }))
        .expect("delivery notification detail must serialize to valid JSON"),
        team: Some(event.recipient_team.clone()),
        agent: Some(event.recipient.clone()),
    }
}

pub(super) fn post_send_event_from_message(
    recipient: &ResolvedRecipient,
    message: &crate::delivery_plan::LogicalMessage,
    recipient_pane_id: Option<&crate::types::PaneId>,
) -> PostSendHookEvent {
    PostSendHookEvent {
        sender: message.envelope.from.clone(),
        sender_team: message
            .envelope
            .source_team
            .clone()
            .unwrap_or_else(|| recipient.team.clone()),
        recipient: recipient.agent.clone(),
        recipient_team: recipient.team.clone(),
        remote_host: message_remote_host(&message.envelope).map(str::to_owned),
        message_id: message.message_id(),
        description: message
            .envelope
            .summary
            .clone()
            .filter(|summary| !summary.trim().is_empty())
            .unwrap_or_else(|| message.envelope.text.clone()),
        requires_ack: message.requires_ack,
        is_ack: message.is_ack,
        task_id: message.envelope.task_id.clone(),
        recipient_pane_id: recipient_pane_id.cloned(),
    }
}

pub(super) fn sender_config_root(metadata: &serde_json::Map<String, Value>) -> Option<PathBuf> {
    compatible_home_dir(metadata).map(Into::into)
}

pub(super) fn post_send_warning(
    prefix: &str,
    event: &PostSendHookEvent,
    error: &AtmError,
) -> WarningEntry {
    WarningEntry::with_code(
        error.code,
        format!(
            "warning: {prefix} for {}@{} message {} ({}): {}.",
            event.recipient, event.recipient_team, event.message_id, error.code, error.message
        ),
        error.primary_recovery().map(str::to_owned),
    )
}
