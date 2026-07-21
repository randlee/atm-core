use crate::SqliteMailboxMetadataRow;
use crate::shared_db::SharedDb;
use atm_storage::AtmError;
use atm_storage::contract::MessageKey;
use atm_storage::schema::{AtmMessageId, ThreadMode};
use atm_storage::types::{AgentName, ChatId, IsoTimestamp, TaskId, TeamName};
use atm_storage::{AckRequirementState, InboxMessage, derive_ack_requirement};
use rusqlite::params;
use serde_json::Map;

type MetadataQueryRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    String,
    i64,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn parse_optional_timestamp(
    raw: Option<String>,
    field_name: &str,
) -> Result<Option<IsoTimestamp>, AtmError> {
    raw.map(|value| value.parse::<IsoTimestamp>())
        .transpose()
        .map_err(|error| {
            AtmError::validation(format!(
                "failed to parse sqlite mailbox metadata {field_name}: {error}"
            ))
        })
}

fn parse_optional_chat_id(
    raw: Option<String>,
    field_name: &str,
) -> Result<Option<ChatId>, AtmError> {
    raw.map(|value| value.parse::<ChatId>())
        .transpose()
        .map_err(|error| {
            AtmError::validation(format!(
                "failed to parse sqlite mailbox metadata {field_name}: {error}"
            ))
        })
}

fn decode_metadata_query_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MetadataQueryRow> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, Option<String>>(1)?,
        row.get::<_, Option<String>>(2)?,
        row.get::<_, Option<String>>(3)?,
        row.get::<_, Option<String>>(4)?,
        row.get::<_, Option<String>>(5)?,
        row.get::<_, String>(6)?,
        row.get::<_, Option<String>>(7)?,
        row.get::<_, String>(8)?,
        row.get::<_, i64>(9)?,
        row.get::<_, i64>(10)?,
        row.get::<_, Option<String>>(11)?,
        row.get::<_, Option<String>>(12)?,
        row.get::<_, Option<String>>(13)?,
    ))
}

fn parse_optional_message_id(
    raw: Option<String>,
    field_name: &str,
) -> Result<Option<AtmMessageId>, AtmError> {
    raw.map(|value| {
        value.parse::<AtmMessageId>().map_err(|error| {
            AtmError::validation(format!(
                "failed to parse sqlite mailbox metadata {field_name}: {error}"
            ))
        })
    })
    .transpose()
}

fn parse_thread_mode(raw: Option<String>) -> Result<Option<ThreadMode>, AtmError> {
    raw.map(|value| {
        serde_json::from_str::<ThreadMode>(&format!("\"{value}\"")).map_err(|error| {
            AtmError::validation(format!(
                "failed to parse sqlite mailbox metadata thread_mode: {error}"
            ))
        })
    })
    .transpose()
}

fn parse_task_id(raw: Option<String>, message_key: &str) -> Result<Option<TaskId>, AtmError> {
    raw.map(|value| {
        value.parse::<TaskId>().map_err(|error| {
            AtmError::validation(format!(
                "failed to parse sqlite mailbox metadata task_id for {message_key}: {error}"
            ))
        })
    })
    .transpose()
}

fn parse_message_key(message_key: &str) -> Result<MessageKey, AtmError> {
    MessageKey::new(message_key.to_string()).map_err(|error| {
        AtmError::validation(format!(
            "failed to parse sqlite mailbox metadata message key: {error}"
        ))
    })
}

fn parse_from_agent(value: &str, message_key: &str) -> Result<AgentName, AtmError> {
    value.parse().map_err(|error| {
        AtmError::validation(format!(
            "failed to parse sqlite mailbox metadata from_agent for {message_key}: {error}"
        ))
    })
}

fn parse_message_at(value: &str) -> Result<IsoTimestamp, AtmError> {
    value.parse::<IsoTimestamp>().map_err(|error| {
        AtmError::validation(format!(
            "failed to parse sqlite mailbox metadata timestamp: {error}"
        ))
    })
}

fn decode_mailbox_metadata_row(
    row: MetadataQueryRow,
) -> Result<SqliteMailboxMetadataRow, AtmError> {
    let (
        message_key,
        message_id,
        parent_message_id,
        thread_mode,
        source_chat_id,
        destination_chat_id,
        from_agent,
        summary,
        message_at,
        read,
        requires_ack,
        acknowledged_at,
        expires_at,
        task_id,
    ) = row;
    let parsed_message_key = parse_message_key(&message_key)?;
    let parsed_message_id = parse_optional_message_id(message_id, "message_id")?;
    let parsed_parent_message_id =
        parse_optional_message_id(parent_message_id, "parent_message_id")?;
    let parsed_thread_mode = parse_thread_mode(thread_mode)?;
    let parsed_from_agent = parse_from_agent(&from_agent, &message_key)?;
    let parsed_source_chat_id = parse_optional_chat_id(source_chat_id, "source_chat_id")?;
    let parsed_destination_chat_id =
        parse_optional_chat_id(destination_chat_id, "destination_chat_id")?;
    let parsed_message_at = parse_message_at(&message_at)?;
    let parsed_acknowledged_at =
        parse_optional_timestamp(acknowledged_at, "acknowledged_at timestamp")?;
    let parsed_expires_at = parse_optional_timestamp(expires_at, "expires_at timestamp")?;
    let parsed_task_id = parse_task_id(task_id, &message_key)?;
    let ack_requirement = derive_ack_requirement(&InboxMessage {
        from: parsed_from_agent.clone(),
        source_chat_id: parsed_source_chat_id.clone(),
        text: String::new(),
        timestamp: parsed_message_at,
        read: read != 0,
        source_team: None,
        destination_chat_id: parsed_destination_chat_id.clone(),
        summary: summary.clone(),
        message_id: parsed_message_id,
        requires_ack: requires_ack != 0,
        pending_ack_at: None,
        acknowledged_at: parsed_acknowledged_at,
        acknowledges_message_id: None,
        parent_message_id: parsed_parent_message_id,
        thread_mode: parsed_thread_mode,
        expires_at: parsed_expires_at,
        task_id: parsed_task_id.clone(),
        extra: Map::new(),
    });
    Ok(SqliteMailboxMetadataRow {
        message_key: parsed_message_key,
        message_id: parsed_message_id,
        parent_message_id: parsed_parent_message_id,
        thread_mode: parsed_thread_mode,
        from_agent: parsed_from_agent,
        source_chat_id: parsed_source_chat_id,
        destination_chat_id: parsed_destination_chat_id,
        summary,
        message_at: parsed_message_at,
        read: read != 0,
        requires_ack: !matches!(ack_requirement, AckRequirementState::NotRequired),
        pending_ack: matches!(ack_requirement, AckRequirementState::RequiredPending),
        acknowledged_at: parsed_acknowledged_at,
        expires_at: parsed_expires_at,
        task_id: parsed_task_id,
    })
}

pub(crate) fn query_mailbox_metadata_rows(
    db: &SharedDb,
    team: &TeamName,
    agent: &AgentName,
    limit: Option<usize>,
) -> Result<Vec<SqliteMailboxMetadataRow>, AtmError> {
    db.with_connection(|connection| {
        let limit_i64 = limit.map(i64::try_from).transpose().map_err(|_| {
            AtmError::validation("mailbox metadata limit exceeds sqlite i64 range".to_string())
        })?;
        // AD.20 keeps this query bounded to mailbox header data only. Full
        // durable message text is reloaded later, and only for surviving
        // summary-miss `--contains` candidates that still need a body check.
        let sql = "SELECT
                 mail_messages.message_key,
                 mail_messages.message_id,
                 mail_messages.parent_message_id,
                 mail_messages.thread_mode,
                 mail_messages.source_chat_id,
                 mail_messages.destination_chat_id,
                 mail_messages.from_agent,
                 mail_messages.summary,
                 mail_messages.message_at,
                 COALESCE(
                     mail_message_states.read,
                     json_extract(mail_messages.envelope_json, '$.read'),
                     0
                 ),
                 COALESCE(
                     json_extract(mail_messages.envelope_json, '$.requiresAck'),
                     0
                 ),
                 mail_message_states.acknowledged_at,
                 mail_message_states.expires_at,
                 json_extract(mail_messages.envelope_json, '$.taskId')
             FROM mail_messages
             LEFT JOIN mail_message_states
               ON mail_message_states.team = mail_messages.team
              AND mail_message_states.agent = mail_messages.agent
              AND mail_message_states.message_key = mail_messages.message_key
             WHERE mail_messages.team = ?1
               AND mail_messages.agent = ?2
               AND mail_message_states.deleted_at IS NULL
               AND (
                    mail_message_states.expires_at IS NULL
                    OR mail_message_states.expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 )
             ORDER BY mail_messages.message_at DESC, mail_messages.message_key DESC
             LIMIT COALESCE(?3, -1);";
        let mut statement = connection
            .prepare(sql)
            .map_err(|error| db.error("failed to prepare sqlite mailbox metadata query", error))?;
        let rows = statement
            .query_map(
                params![team.as_str(), agent.as_str(), limit_i64],
                decode_metadata_query_row,
            )
            .map_err(|error| db.error("failed to execute sqlite mailbox metadata query", error))?;
        let mut collected = Vec::new();
        for row in rows {
            let row: MetadataQueryRow = row
                .map_err(|error| db.error("failed to decode sqlite mailbox metadata row", error))?;
            collected.push(decode_mailbox_metadata_row(row)?);
        }
        Ok(collected)
    })
}
