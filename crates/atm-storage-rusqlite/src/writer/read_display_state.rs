use super::ops::WriteOpResult;
use super::stmt_cache::WriterStatementCache;
use crate::shared_db::{SharedDbTarget, sqlite_error};
use atm_storage::contract::{MailboxScope, MessageKey};
use atm_storage::error::AtmError;
use atm_storage::types::IsoTimestamp;
use rusqlite::{Connection, params};

pub(super) fn execute_read_display_state(
    mailbox: &MailboxScope,
    message_ids: &[MessageKey],
    seen_watermark: Option<IsoTimestamp>,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    let now = IsoTimestamp::now();
    let updated_at = now.to_string();
    let next_due = atm_storage::next_reminder_due(now).to_string();
    for message_key in message_ids {
        let updated = cache
            .mark_message_read(
                connection,
                params![
                    mailbox.team.as_str(),
                    mailbox.agent.as_str(),
                    message_key.as_str(),
                    updated_at,
                    next_due,
                ],
            )
            .map_err(|error| sqlite_error(target, "failed to mark mailbox message read", error))?;
        if updated != 1 {
            return Err(AtmError::mailbox_read(format!(
                "message {} was not found for {}@{} while applying read display state",
                message_key.as_str(),
                mailbox.agent.as_str(),
                mailbox.team.as_str(),
            )));
        }
    }
    if let Some(watermark) = seen_watermark {
        connection
            .execute(
                "INSERT INTO mail_seen_watermarks(team, agent, watermark)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(team, agent) DO UPDATE SET watermark = excluded.watermark",
                params![
                    mailbox.team.as_str(),
                    mailbox.agent.as_str(),
                    watermark.into_inner().to_rfc3339(),
                ],
            )
            .map_err(|error| {
                sqlite_error(target, "failed to persist mailbox seen watermark", error)
            })?;
    }
    Ok(WriteOpResult::ReadDisplayStateApplied)
}
