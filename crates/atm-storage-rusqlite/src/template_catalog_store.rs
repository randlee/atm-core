use std::sync::Arc;

use atm_storage::{
    DecomposedMessageAdmission, DecomposedMessageAdmissionOutcome, DecomposedMessageRecord,
    EffectiveTag, MergedVarsJson, MessageTagProvenance, StoredTemplate, TemplateCatalogStore,
    TemplateFirstSeen, TemplateListFilter, TemplateRegistration, TemplateRegistrationOutcome,
    TemplateSummary, WorkflowIteration, WorkflowScopeId, WorkflowScopeKind, WorkflowSnapshot,
    WorkflowStage, WorkflowState, WorkflowTransition,
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
        let request = request.into_normalized_workflow_metadata()?;
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

    fn load_decomposed_message(
        &self,
        key: &atm_storage::contract::MessageKey,
    ) -> Result<Option<DecomposedMessageRecord>, atm_storage::AtmError> {
        self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT m.template_sha, m.vars_json, m.category, m.tags_json, m.content_format,
                            workflow_scope_kind, workflow_scope_id, workflow_state,
                            workflow_stage, workflow_transition, workflow_iteration,
                            applied_template_tags_json, effective_tags_json,
                            t.template_type, t.schema_json
                     FROM mail_messages m
                     JOIN message_templates t ON t.template_sha = m.template_sha
                     WHERE m.message_key = ?1 AND m.template_sha IS NOT NULL",
                    params![key.as_str()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, Option<String>>(7)?,
                            row.get::<_, Option<String>>(8)?,
                            row.get::<_, Option<String>>(9)?,
                            row.get::<_, Option<String>>(10)?,
                            row.get::<_, Option<String>>(11)?,
                            row.get::<_, Option<String>>(12)?,
                            row.get::<_, Option<String>>(13)?,
                            row.get::<_, String>(14)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| self.db.error("failed to load decomposed message", error))?
                .map(
                    |(template_sha, vars_json, category, tags_json, content_format, scope_kind, scope_id, state, stage, transition, iteration, applied_tags_json, effective_tags_json, template_type, schema_json)| {
                        let vars = deserialize_json(&vars_json, "decomposed message vars")?;
                        let vars = MergedVarsJson::try_from_merged_object(vars)?;
                        let tags: Vec<atm_storage::InstanceTag> =
                            deserialize_json(&tags_json, "decomposed message tags")?;
                        let workflow_snapshot = match (scope_kind, scope_id, state, stage, transition, iteration) {
                            (Some(scope_kind), Some(scope_id), Some(state), Some(stage), Some(transition), iteration) => Some(WorkflowSnapshot {
                                scope_kind: WorkflowScopeKind::new(scope_kind)?,
                                scope_id: WorkflowScopeId::new(scope_id)?,
                                state: WorkflowState::new(state)?,
                                stage: WorkflowStage::new(stage)?,
                                transition: WorkflowTransition::new(transition)?,
                                iteration: iteration.map(WorkflowIteration::new).transpose()?,
                            }),
                            (None, None, None, None, None, None) => None,
                            _ => return Err(atm_storage::AtmError::mailbox_read("stored decomposed workflow snapshot is incomplete")),
                        };
                        let tag_provenance = match (workflow_snapshot.as_ref(), applied_tags_json, effective_tags_json) {
                            (Some(snapshot), Some(applied_tags_json), Some(effective_tags_json)) => {
                                let applied_template_tags = deserialize_json(&applied_tags_json, "applied template tags")?;
                                let effective_tags: Vec<EffectiveTag> = deserialize_json(&effective_tags_json, "effective tags")?;
                                let frontmatter: atm_storage::TemplateFrontmatter = deserialize_json(&schema_json, "template frontmatter")?;
                                let expected = DecomposedMessageAdmission::expected_tag_provenance_for(
                                    &tags,
                                    &frontmatter.template_tags,
                                    template_type.as_deref(),
                                    content_format.as_deref(),
                                    snapshot,
                                )?;
                                if expected.applied_template_tags != applied_template_tags || expected.effective_tags != effective_tags {
                                    return Err(atm_storage::AtmError::mailbox_read("stored decomposed tag provenance does not match its immutable admission inputs"));
                                }
                                Some(MessageTagProvenance { applied_template_tags, effective_tags, ..expected })
                            }
                            (None, None, None) => None,
                            _ => return Err(atm_storage::AtmError::mailbox_read("stored decomposed tag provenance is incomplete")),
                        };
                        Ok(DecomposedMessageRecord {
                            key: key.clone(),
                            template_sha: template_sha.parse()?,
                            vars,
                            category,
                            tags,
                            content_format,
                            workflow_snapshot,
                            tag_provenance,
                        })
                    },
                )
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
        let mut admission = admission;
        admission.template = admission.template.into_normalized_workflow_metadata()?;
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
