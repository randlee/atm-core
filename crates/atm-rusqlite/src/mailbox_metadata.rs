use crate::shared_db::SharedDb;
use atm_core::boundary::MessageKey;
use atm_core::error::AtmError;
use atm_core::schema::LegacyMessageId;
use atm_core::types::{AgentName, IsoTimestamp, TaskId, TeamName};
use rusqlite::params;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxMetadataRow {
    pub message_key: MessageKey,
    pub legacy_message_id: Option<LegacyMessageId>,
    pub parent_message_id: Option<LegacyMessageId>,
    pub from_agent: AgentName,
    pub summary: Option<String>,
    pub message_at: IsoTimestamp,
    pub read: bool,
    pub pending_ack: bool,
    pub task_id: Option<TaskId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailboxMetadataCounts {
    pub total_messages: u64,
    pub unread_messages: u64,
    pub pending_ack_messages: u64,
}

pub fn query_mailbox_metadata_rows(
    db: &SharedDb,
    team: &TeamName,
    agent: &AgentName,
    limit: usize,
) -> Result<Vec<MailboxMetadataRow>, AtmError> {
    db.with_connection(|connection| {
        let mut statement = connection
            .prepare(
                "SELECT
                     mail_messages.message_key,
                     mail_messages.legacy_message_id,
                     mail_messages.parent_message_id,
                     mail_messages.from_agent,
                     mail_messages.summary,
                     mail_messages.message_at,
                     COALESCE(
                         json_extract(mail_visibility_states.state_json, '$.read'),
                         json_extract(mail_messages.envelope_json, '$.read'),
                         0
                     ),
                     ack_state.pending_ack_at,
                     ack_state.acknowledged_at,
                     json_extract(mail_messages.envelope_json, '$.taskId')
                 FROM mail_messages
                 LEFT JOIN mail_visibility_states
                   ON mail_visibility_states.team = mail_messages.team
                  AND mail_visibility_states.agent = mail_messages.agent
                  AND mail_visibility_states.message_key = mail_messages.message_key
                 LEFT JOIN ack_state
                   ON ack_state.team = mail_messages.team
                  AND ack_state.agent = mail_messages.agent
                  AND ack_state.message_key = mail_messages.message_key
                 WHERE mail_messages.team = ?1
                   AND mail_messages.agent = ?2
                 ORDER BY mail_messages.message_at DESC, mail_messages.message_key DESC
                 LIMIT ?3;",
            )
            .map_err(|error| db.error("failed to prepare bounded mailbox metadata query", error))?;
        let rows = statement
            .query_map(params![team.as_str(), agent.as_str(), limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            })
            .map_err(|error| db.error("failed to execute bounded mailbox metadata query", error))?;
        let mut collected = Vec::new();
        for row in rows {
            let (
                message_key,
                legacy_message_id,
                parent_message_id,
                from_agent,
                summary,
                message_at,
                read,
                pending_ack_at,
                acknowledged_at,
                task_id,
            ) = row.map_err(|error| db.error("failed to decode bounded mailbox metadata row", error))?;
            collected.push(MailboxMetadataRow {
                message_key: MessageKey::new(message_key).map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse bounded mailbox metadata message key: {error}"
                    ))
                })?,
                legacy_message_id: legacy_message_id
                    .map(|value| {
                        value.parse::<LegacyMessageId>().map_err(|error| {
                            AtmError::validation(format!(
                                "failed to parse bounded mailbox metadata legacy_message_id: {error}"
                            ))
                        })
                    })
                    .transpose()?,
                parent_message_id: parent_message_id
                    .map(|value| {
                        value.parse::<LegacyMessageId>().map_err(|error| {
                            AtmError::validation(format!(
                                "failed to parse bounded mailbox metadata parent_message_id: {error}"
                            ))
                        })
                    })
                    .transpose()?,
                from_agent: from_agent.parse()?,
                summary,
                message_at: message_at
                    .parse::<chrono::DateTime<chrono::Utc>>()
                    .map(IsoTimestamp::from_datetime)
                    .map_err(|error| {
                        AtmError::validation(format!(
                            "failed to parse bounded mailbox metadata timestamp: {error}"
                        ))
                    })?,
                read: read != 0,
                pending_ack: pending_ack_at.is_some() && acknowledged_at.is_none(),
                task_id: task_id.map(|value| value.parse::<TaskId>()).transpose()?,
            });
        }
        Ok(collected)
    })
}

pub fn query_mailbox_metadata_counts(
    db: &SharedDb,
    team: &TeamName,
    agent: &AgentName,
) -> Result<MailboxMetadataCounts, AtmError> {
    db.with_connection(|connection| {
        connection
            .query_row(
                "SELECT
                     COUNT(*),
                     SUM(CASE
                           WHEN COALESCE(
                                    json_extract(mail_visibility_states.state_json, '$.read'),
                                    json_extract(mail_messages.envelope_json, '$.read'),
                                    0
                                ) = 0
                           THEN 1 ELSE 0
                         END),
                     SUM(CASE
                           WHEN ack_state.pending_ack_at IS NOT NULL
                            AND ack_state.acknowledged_at IS NULL
                           THEN 1 ELSE 0
                         END)
                 FROM mail_messages
                 LEFT JOIN mail_visibility_states
                   ON mail_visibility_states.team = mail_messages.team
                  AND mail_visibility_states.agent = mail_messages.agent
                  AND mail_visibility_states.message_key = mail_messages.message_key
                 LEFT JOIN ack_state
                   ON ack_state.team = mail_messages.team
                  AND ack_state.agent = mail_messages.agent
                  AND ack_state.message_key = mail_messages.message_key
                 WHERE mail_messages.team = ?1
                   AND mail_messages.agent = ?2;",
                params![team.as_str(), agent.as_str()],
                |row| {
                    Ok(MailboxMetadataCounts {
                        total_messages: row.get::<_, i64>(0)? as u64,
                        unread_messages: row.get::<_, Option<i64>>(1)?.unwrap_or(0) as u64,
                        pending_ack_messages: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
                    })
                },
            )
            .map_err(|error| db.error("failed to query bounded mailbox metadata counts", error))
    })
}
