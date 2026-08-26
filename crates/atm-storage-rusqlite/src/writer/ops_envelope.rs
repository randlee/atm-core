use atm_storage::schema::MessageEnvelope;
use atm_storage::types::IsoTimestamp;
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct StorageEnvelope<'a> {
    from: &'a atm_storage::types::AgentName,
    #[serde(
        rename = "sourceChatId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    source_chat_id: &'a Option<atm_storage::types::ChatId>,
    text: &'a str,
    timestamp: IsoTimestamp,
    read: bool,
    #[serde(default)]
    source_team: &'a Option<atm_storage::types::TeamName>,
    #[serde(
        rename = "destinationChatId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    destination_chat_id: &'a Option<atm_storage::types::ChatId>,
    #[serde(default)]
    summary: &'a Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message_id: Option<String>,
    #[serde(rename = "pendingAckAt", skip_serializing_if = "Option::is_none")]
    pending_ack_at: Option<IsoTimestamp>,
    #[serde(rename = "acknowledgedAt", skip_serializing_if = "Option::is_none")]
    acknowledged_at: Option<IsoTimestamp>,
    #[serde(
        rename = "acknowledgesMessageId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    acknowledges_message_id: Option<String>,
    #[serde(
        rename = "parentMessageId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    parent_message_id: Option<String>,
    #[serde(rename = "threadMode", skip_serializing_if = "Option::is_none")]
    thread_mode: &'a Option<atm_storage::schema::ThreadMode>,
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    expires_at: Option<IsoTimestamp>,
    #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
    task_id: &'a Option<atm_storage::types::TaskId>,
    #[serde(flatten)]
    extra: &'a serde_json::Map<String, serde_json::Value>,
}

impl<'a> StorageEnvelope<'a> {
    pub(super) fn new(envelope: &'a MessageEnvelope) -> Self {
        Self {
            from: &envelope.from,
            source_chat_id: &envelope.source_chat_id,
            text: envelope.text.as_str(),
            timestamp: envelope.timestamp,
            read: envelope.read,
            source_team: &envelope.source_team,
            destination_chat_id: &envelope.destination_chat_id,
            summary: &envelope.summary,
            message_id: envelope.message_id.as_ref().map(ToString::to_string),
            pending_ack_at: envelope.pending_ack_at,
            acknowledged_at: envelope.acknowledged_at,
            acknowledges_message_id: envelope
                .acknowledges_message_id
                .as_ref()
                .map(ToString::to_string),
            parent_message_id: envelope.parent_message_id.as_ref().map(ToString::to_string),
            thread_mode: &envelope.thread_mode,
            expires_at: envelope.expires_at,
            task_id: &envelope.task_id,
            extra: &envelope.extra,
        }
    }
}
