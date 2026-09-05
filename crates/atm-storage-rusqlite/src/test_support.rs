//! Test-only durable SQLite inspection helpers.

use std::path::Path;

use atm_storage::AtmError;
use rusqlite::{Connection, params};

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
