use std::sync::Arc;

use atm_storage::{
    DecomposedMessageAdmission, DecomposedMessageAdmissionOutcome, StoredTemplate,
    TemplateCatalogStore, TemplateFirstSeen, TemplateListFilter, TemplateRegistration,
    TemplateRegistrationOutcome, TemplateSummary,
};
use rusqlite::{OptionalExtension, params};

use crate::shared_db::{SharedDb, deserialize_json};

pub(crate) fn template_catalog_store(db: Arc<SharedDb>) -> Arc<dyn TemplateCatalogStore> {
    Arc::new(SqliteTemplateCatalogStore::new(db))
}

#[derive(Debug)]
struct SqliteTemplateCatalogStore {
    db: Arc<SharedDb>,
}

impl SqliteTemplateCatalogStore {
    fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }
}

impl atm_storage::contract::sealed::Sealed for SqliteTemplateCatalogStore {}

impl TemplateCatalogStore for SqliteTemplateCatalogStore {
    fn register(
        &self,
        request: TemplateRegistration,
    ) -> Result<TemplateRegistrationOutcome, atm_storage::AtmError> {
        request.validate()?;
        self.db.submit_template_registration(request)
    }

    fn load(
        &self,
        sha: &atm_storage::TemplateSha,
    ) -> Result<Option<StoredTemplate>, atm_storage::AtmError> {
        self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT template_sha, template_type, template_name, content_bytes,
                            content_text, schema_json, first_seen_at, first_seen_by
                     FROM message_templates WHERE template_sha = ?1",
                    params![sha.as_str()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, Vec<u8>>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| self.db.error("failed to load immutable template", error))?
                .map(decode_stored_template)
                .transpose()
        })
    }

    fn list(
        &self,
        filter: TemplateListFilter,
    ) -> Result<Vec<TemplateSummary>, atm_storage::AtmError> {
        self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT template_sha, template_type, template_name, first_seen_at, first_seen_by
                     FROM message_templates
                     WHERE (?1 IS NULL OR template_type = ?1)
                     ORDER BY first_seen_at ASC, template_sha ASC",
                )
                .map_err(|error| self.db.error("failed to prepare template listing", error))?;
            statement
                .query_map(params![filter.template_type.as_deref()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })
                .map_err(|error| self.db.error("failed to execute template listing", error))?
                .map(|row| {
                    let (sha, template_type, template_name, first_seen_at, first_seen_by) = row
                        .map_err(|error| {
                            self.db.error("failed to decode template listing", error)
                        })?;
                    Ok(TemplateSummary {
                        sha: sha.parse()?,
                        template_type,
                        template_name,
                        first_seen: TemplateFirstSeen::new(
                            first_seen_at.parse().map_err(|error| {
                                atm_storage::AtmError::mailbox_read(format!(
                                    "stored template first_seen_at is invalid: {error}"
                                ))
                            })?,
                            first_seen_by,
                        )?,
                    })
                })
                .collect()
        })
    }

    fn admit_decomposed_message(
        &self,
        admission: DecomposedMessageAdmission,
    ) -> Result<DecomposedMessageAdmissionOutcome, atm_storage::AtmError> {
        admission.validate()?;
        self.db.submit_decomposed_message_admission(admission)
    }
}

type StoredTemplateRow = (
    String,
    Option<String>,
    Option<String>,
    Vec<u8>,
    String,
    String,
    String,
    String,
);

fn decode_stored_template(row: StoredTemplateRow) -> Result<StoredTemplate, atm_storage::AtmError> {
    let (
        sha,
        template_type,
        template_name,
        content_bytes,
        content_text,
        schema_json,
        first_seen_at,
        first_seen_by,
    ) = row;
    let frontmatter = deserialize_json(&schema_json, "template frontmatter")?;
    let template = StoredTemplate {
        sha: sha.parse()?,
        template_type,
        template_name,
        content_bytes,
        content_text,
        frontmatter,
        first_seen: TemplateFirstSeen::new(
            first_seen_at.parse().map_err(|error| {
                atm_storage::AtmError::mailbox_read(format!(
                    "stored template first_seen_at is invalid: {error}"
                ))
            })?,
            first_seen_by,
        )?,
    };
    let projection = std::str::from_utf8(&template.content_bytes)
        .map_err(|_| atm_storage::AtmError::template_content_not_utf8())?;
    if projection != template.content_text {
        return Err(atm_storage::AtmError::mailbox_read(
            "stored template content_text does not match its UTF-8 byte projection",
        ));
    }
    Ok(template)
}
