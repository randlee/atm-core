//! Test-only durable SQLite inspection helpers.

use std::path::Path;

use atm_storage::AtmError;
use rusqlite::{Connection, params};

/// Returns the concrete lower-priority diagnostic queue bound for
/// cross-crate saturation fixtures.
#[doc(hidden)]
pub fn diagnostic_queue_batches_for_test() -> usize {
    crate::writer::DIAGNOSTIC_QUEUE_BATCHES
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateAdmissionSnapshot {
    pub template_count: usize,
    pub decomposed_count: usize,
    pub messages: Vec<TemplateAdmissionMessage>,
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateAdmissionMessage {
    pub message_key: String,
    pub template_sha: Option<String>,
    pub vars_json: Option<String>,
    pub category: Option<String>,
    pub content_format: Option<String>,
    pub tags_json: String,
    pub message_text: Option<String>,
}

/// Reads only durable template-admission projections for black-box tests.
#[doc(hidden)]
pub fn inspect_template_admission_for_test(
    path: impl AsRef<Path>,
    message_keys: &[String],
) -> Result<TemplateAdmissionSnapshot, AtmError> {
    let connection = Connection::open(path.as_ref()).map_err(|error| {
        AtmError::mailbox_read(format!(
            "failed to inspect template-admission fixture: {error}"
        ))
    })?;
    let (template_count, decomposed_count): (i64, i64) = connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM message_templates), (SELECT COUNT(*) FROM decomposed_messages)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| AtmError::mailbox_read(format!("failed to count template-admission fixture rows: {error}")))?;
    let messages = message_keys
        .iter()
        .map(|message_key| {
            connection.query_row(
                "SELECT message_key, template_sha, vars_json, category, content_format, tags_json, message_text FROM mail_messages WHERE message_key = ?1",
                params![message_key],
                |row| Ok(TemplateAdmissionMessage {
                    message_key: row.get(0)?, template_sha: row.get(1)?, vars_json: row.get(2)?,
                    category: row.get(3)?, content_format: row.get(4)?, tags_json: row.get(5)?, message_text: row.get(6)?,
                }),
            ).map_err(|error| AtmError::mailbox_read(format!("failed to inspect template-admission message '{message_key}': {error}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TemplateAdmissionSnapshot {
        template_count: usize::try_from(template_count).map_err(|_| {
            AtmError::mailbox_read("template-admission fixture count exceeds usize range")
        })?,
        decomposed_count: usize::try_from(decomposed_count).map_err(|_| {
            AtmError::mailbox_read("template-admission fixture count exceeds usize range")
        })?,
        messages,
    })
}

/// Reads the durable pending marker used by black-box queue tests.
#[doc(hidden)]
pub fn inspect_pending_nudge_state_for_test(
    path: impl AsRef<Path>,
    team: &str,
    agent: &str,
    message_key: &str,
) -> Result<(Option<String>, u32), AtmError> {
    let connection = Connection::open(path.as_ref()).map_err(|error| {
        AtmError::mailbox_read(format!("failed to inspect pending nudge fixture: {error}"))
    })?;
    connection
        .query_row(
            "SELECT nudge_pending_at, nudge_attempts FROM mail_message_states
             WHERE team = ?1 AND agent = ?2 AND message_key = ?3",
            params![team, agent, message_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| {
            AtmError::mailbox_read(format!("failed to inspect pending nudge state: {error}"))
        })
}

/// Returns the durable mailbox-state columns for schema black-box tests.
#[doc(hidden)]
pub fn inspect_mail_message_state_columns_for_test(
    path: impl AsRef<Path>,
) -> Result<Vec<String>, AtmError> {
    let connection = Connection::open(path.as_ref()).map_err(|error| {
        AtmError::mailbox_read(format!("failed to inspect mailbox schema fixture: {error}"))
    })?;
    let mut statement = connection
        .prepare("PRAGMA table_info(mail_message_states)")
        .map_err(|error| {
            AtmError::mailbox_read(format!("failed to inspect mailbox schema: {error}"))
        })?;
    statement
        .query_map([], |row| row.get(1))
        .map_err(|error| AtmError::mailbox_read(format!("failed to read mailbox schema: {error}")))?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|error| {
            AtmError::mailbox_read(format!("failed to collect mailbox schema: {error}"))
        })
}

/// Reads the acknowledgement marker used by black-box mailbox tests.
#[doc(hidden)]
pub fn inspect_message_ack_state_for_test(
    path: impl AsRef<Path>,
    team: &str,
    agent: &str,
    message_key: &str,
) -> Result<bool, AtmError> {
    let connection = Connection::open(path.as_ref()).map_err(|error| {
        AtmError::mailbox_read(format!(
            "failed to inspect acknowledgement fixture: {error}"
        ))
    })?;
    connection
        .query_row(
            "SELECT pending_ack_at IS NOT NULL AND acknowledged_at IS NULL
             FROM mail_message_states
             WHERE team = ?1 AND agent = ?2 AND message_key = ?3",
            params![team, agent, message_key],
            |row| row.get(0),
        )
        .map_err(|error| {
            AtmError::mailbox_read(format!("failed to inspect acknowledgement state: {error}"))
        })
}
