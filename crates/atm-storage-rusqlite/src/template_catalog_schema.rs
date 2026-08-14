//! SQLite-owned schema and writer submission seam for immutable templates.
//!
//! Keeping this beside the catalog adapter prevents the generic shared-database
//! root from accumulating feature-specific DDL or capability methods.

use crate::shared_db::{SharedDb, SharedDbTarget, SqliteConnection, ensure_column, sqlite_error};
use crate::writer::{WriteOp, WriteOpResult};
use atm_storage::{
    DecomposedMessageAdmission, DecomposedMessageAdmissionOutcome, TemplateRegistration,
    TemplateRegistrationOutcome,
};

const TEMPLATE_CATALOG_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS message_templates (
    template_sha TEXT NOT NULL PRIMARY KEY,
    template_type TEXT NULL,
    template_name TEXT NULL,
    content_bytes BLOB NOT NULL,
    content_text TEXT NOT NULL,
    schema_json TEXT NOT NULL DEFAULT '{}',
    first_seen_at TEXT NOT NULL,
    first_seen_by TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_message_templates_type
    ON message_templates(template_type) WHERE template_type IS NOT NULL;
"#;

const DECOMPOSED_MESSAGES_VIEW_V2: &str = r#"
DROP VIEW IF EXISTS decomposed_messages;
CREATE VIEW decomposed_messages AS
SELECT m.team, m.agent, m.from_agent, m.message_at, m.message_id,
       m.template_sha, t.template_type, m.vars_json,
       m.category, m.tags_json, m.tags_json AS instance_tags_json, m.summary,
       m.workflow_scope_kind, m.workflow_scope_id, m.workflow_state,
       m.workflow_stage, m.workflow_transition, m.workflow_iteration,
       m.applied_template_tags_json,
       CASE WHEN m.workflow_scope_kind IS NULL THEN NULL ELSE COALESCE((
           SELECT json_group_array(effective.value)
           FROM json_each(COALESCE(m.effective_tags_json, '[]')) AS effective
           WHERE NOT EXISTS (
               SELECT 1 FROM json_each(COALESCE(m.tags_json, '[]')) AS instance
               WHERE instance.value = effective.value
           )
           AND NOT EXISTS (
               SELECT 1 FROM json_each(COALESCE(m.applied_template_tags_json, '[]')) AS applied
               WHERE applied.value = effective.value
           )
       ), '[]') END AS derived_tags_json,
       m.effective_tags_json,
       s.read, s.acknowledged_at, s.pending_ack_at
FROM mail_messages m
JOIN message_templates t ON t.template_sha = m.template_sha
LEFT JOIN mail_message_states s
  ON (s.team, s.agent, s.message_key) = (m.team, m.agent, m.message_key)
WHERE m.template_sha IS NOT NULL;
"#;

pub(crate) fn ensure_schema(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), atm_storage::AtmError> {
    ensure_mail_message_template_columns(connection, target)?;
    connection
        .execute_batch(TEMPLATE_CATALOG_DDL)
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to initialize template catalog schema",
                error,
            )
        })?;
    connection
        .execute_batch(DECOMPOSED_MESSAGES_VIEW_V2)
        .map_err(|error| sqlite_error(target, "failed to create decomposed_messages view", error))
}

/// Applies catalog-owned compatibility columns before the catalog view refers
/// to them. Keeping these migrations with the catalog DDL prevents the generic
/// shared database root from owning feature-specific schema policy.
fn ensure_mail_message_template_columns(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), atm_storage::AtmError> {
    ensure_column(
        connection,
        target,
        "mail_messages",
        "template_sha",
        "ALTER TABLE mail_messages ADD COLUMN template_sha TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "vars_json",
        "ALTER TABLE mail_messages ADD COLUMN vars_json TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "category",
        "ALTER TABLE mail_messages ADD COLUMN category TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "content_format",
        "ALTER TABLE mail_messages ADD COLUMN content_format TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "tags_json",
        "ALTER TABLE mail_messages ADD COLUMN tags_json TEXT NOT NULL DEFAULT '[]';",
    )?;
    for (name, statement) in [
        (
            "workflow_scope_kind",
            "ALTER TABLE mail_messages ADD COLUMN workflow_scope_kind TEXT NULL;",
        ),
        (
            "workflow_scope_id",
            "ALTER TABLE mail_messages ADD COLUMN workflow_scope_id TEXT NULL;",
        ),
        (
            "workflow_state",
            "ALTER TABLE mail_messages ADD COLUMN workflow_state TEXT NULL;",
        ),
        (
            "workflow_stage",
            "ALTER TABLE mail_messages ADD COLUMN workflow_stage TEXT NULL;",
        ),
        (
            "workflow_transition",
            "ALTER TABLE mail_messages ADD COLUMN workflow_transition TEXT NULL;",
        ),
        (
            "workflow_iteration",
            "ALTER TABLE mail_messages ADD COLUMN workflow_iteration TEXT NULL;",
        ),
        (
            "applied_template_tags_json",
            "ALTER TABLE mail_messages ADD COLUMN applied_template_tags_json TEXT NULL;",
        ),
        (
            "effective_tags_json",
            "ALTER TABLE mail_messages ADD COLUMN effective_tags_json TEXT NULL;",
        ),
    ] {
        ensure_column(connection, target, "mail_messages", name, statement)?;
    }
    Ok(())
}

impl SharedDb {
    pub(crate) fn submit_template_registration(
        &self,
        request: TemplateRegistration,
    ) -> Result<TemplateRegistrationOutcome, atm_storage::AtmError> {
        match self.submit_writer_op(WriteOp::RegisterTemplate(Box::new(request)))? {
            WriteOpResult::TemplateRegistration(outcome) => Ok(outcome),
            other => Err(atm_storage::AtmError::daemon_unavailable(format!(
                "sqlite writer returned the wrong result for template registration: {other:?}"
            ))),
        }
    }

    pub(crate) fn submit_decomposed_message_admission(
        &self,
        admission: DecomposedMessageAdmission,
    ) -> Result<DecomposedMessageAdmissionOutcome, atm_storage::AtmError> {
        match self.submit_writer_op(WriteOp::AdmitDecomposedMessage(Box::new(admission)))? {
            WriteOpResult::DecomposedMessageAdmission(outcome) => Ok(outcome),
            other => Err(atm_storage::AtmError::daemon_unavailable(format!(
                "sqlite writer returned the wrong result for decomposed message admission: {other:?}"
            ))),
        }
    }
}
