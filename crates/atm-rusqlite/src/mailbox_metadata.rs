use crate::shared_db::SharedDb;
use atm_core::boundary::{MailStoreMailboxMetadataCounts, MailStoreMailboxMetadataRow, MessageKey};
use atm_core::error::AtmError;
use atm_core::schema::{AtmMessageId, ThreadMode};
use atm_core::types::{AgentName, IsoTimestamp, TaskId, TeamName};
use rusqlite::params;

pub fn query_mailbox_metadata_rows(
    db: &SharedDb,
    team: &TeamName,
    agent: &AgentName,
    limit: Option<usize>,
) -> Result<Vec<MailStoreMailboxMetadataRow>, AtmError> {
    db.with_connection(|connection| {
        let mut statement = connection
            .prepare(
                "SELECT
                     mail_messages.message_key,
                     mail_messages.message_id,
                     mail_messages.parent_message_id,
                     mail_messages.thread_mode,
                     mail_messages.from_agent,
                     mail_messages.summary,
                     mail_messages.message_at,
                     COALESCE(
                         mail_message_states.read,
                         json_extract(mail_messages.envelope_json, '$.read'),
                         0
                     ),
                     mail_message_states.pending_ack_at,
                     mail_message_states.acknowledged_at,
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
                 ORDER BY mail_messages.message_at DESC, mail_messages.message_key DESC;",
            )
            .map_err(|error| db.error("failed to prepare bounded mailbox metadata query", error))?;
        let rows = statement
            .query_map(params![team.as_str(), agent.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                ))
            })
            .map_err(|error| db.error("failed to execute bounded mailbox metadata query", error))?;
        let mut collected = Vec::new();
        for row in rows {
            let (
                message_key,
                message_id,
                parent_message_id,
                thread_mode,
                from_agent,
                summary,
                message_at,
                read,
                pending_ack_at,
                acknowledged_at,
                task_id,
            ) = row.map_err(|error| db.error("failed to decode bounded mailbox metadata row", error))?;
            collected.push(MailStoreMailboxMetadataRow {
                message_key: MessageKey::new(message_key).map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse bounded mailbox metadata message key: {error}"
                    ))
                    .with_recovery(
                        "Repair or remove the malformed message-key row before retrying the bounded mailbox metadata query.",
                    )
                })?,
                message_id: message_id
                    .map(|value| {
                        value.parse::<AtmMessageId>().map_err(|error| {
                            AtmError::validation(format!(
                                "failed to parse bounded mailbox metadata message_id: {error}"
                            ))
                            .with_recovery(
                                "Repair or remove the malformed message_id row before retrying the bounded mailbox metadata query.",
                            )
                        })
                    })
                    .transpose()?,
                parent_message_id: parent_message_id
                    .map(|value| {
                        value.parse::<AtmMessageId>().map_err(|error| {
                            AtmError::validation(format!(
                                "failed to parse bounded mailbox metadata parent_message_id: {error}"
                            ))
                            .with_recovery(
                                "Repair or remove the malformed parent_message_id row before retrying the bounded mailbox metadata query.",
                            )
                        })
                    })
                    .transpose()?,
                thread_mode: thread_mode
                    .map(|value| {
                        serde_json::from_str::<ThreadMode>(&format!("\"{value}\"")).map_err(
                            |error| {
                                AtmError::validation(format!(
                                    "failed to parse bounded mailbox metadata thread_mode: {error}"
                                ))
                                .with_recovery(
                                    "Repair or remove the malformed thread_mode row before retrying the bounded mailbox metadata query.",
                                )
                            },
                        )
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
                        .with_recovery(
                            "Repair or remove the malformed bounded-mailbox timestamp row before retrying the metadata query.",
                        )
                    })?,
                read: read != 0,
                pending_ack: pending_ack_at.is_some() && acknowledged_at.is_none(),
                task_id: task_id.map(|value| value.parse::<TaskId>()).transpose()?,
            });
            if let Some(limit) = limit
                && collected.len() >= limit
            {
                break;
            }
        }
        Ok(collected)
    })
}

pub fn query_mailbox_metadata_counts(
    db: &SharedDb,
    team: &TeamName,
    agent: &AgentName,
) -> Result<MailStoreMailboxMetadataCounts, AtmError> {
    db.with_connection(|connection| {
        connection
            .query_row(
                "SELECT
                     COUNT(*),
                     SUM(CASE
                           WHEN COALESCE(
                                    mail_message_states.read,
                                    json_extract(mail_messages.envelope_json, '$.read'),
                                    0
                                ) = 0
                           THEN 1 ELSE 0
                         END),
                     SUM(CASE
                           WHEN mail_message_states.pending_ack_at IS NOT NULL
                            AND mail_message_states.acknowledged_at IS NULL
                           THEN 1 ELSE 0
                         END)
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
                   );",
                params![team.as_str(), agent.as_str()],
                |row| {
                    Ok(MailStoreMailboxMetadataCounts {
                        total_messages: row.get::<_, i64>(0)? as u64,
                        unread_messages: row.get::<_, Option<i64>>(1)?.unwrap_or(0) as u64,
                        pending_ack_messages: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
                    })
                },
            )
            .map_err(|error| db.error("failed to query bounded mailbox metadata counts", error))
    })
}
