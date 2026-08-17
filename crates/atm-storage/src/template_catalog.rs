//! Backend-neutral immutable template-catalog contract.
//!
//! This module deliberately owns only durable DTOs and the sealed capability.
//! It neither parses templates nor renders them; the core-owned composition
//! boundary converts validated adapter output into these values exactly once.

use std::collections::BTreeSet;

#[cfg(test)]
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::contract::{Message, MessageKey};
use crate::error::AtmError;
use crate::template_workflow::{
    DerivedTag, EffectiveTag, InstanceTag, MessageTagProvenance, TemplateTag, WorkflowSnapshot,
};
use crate::types::{IsoTimestamp, TemplateFrontmatter, TemplateSha};

/// The source representation selected before durable admission.
#[derive(Debug, Clone, PartialEq)]
pub enum MessageBody {
    Inline(String),
    Decomposed {
        template_sha: TemplateSha,
        vars: MergedVarsJson,
    },
    FileRef(String),
}

/// The durable column projection of one [`MessageBody`] variant.
///
/// This is constructed exclusively by [`MessageBody::columns`], so callers
/// cannot represent an inline/file-reference body with decomposition metadata,
/// nor a decomposed body with `message_text`. AN.3 will pass this projection at
/// its core-to-storage admission boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MessageBodyColumns<'a> {
    message_text: Option<&'a str>,
    template_sha: Option<&'a TemplateSha>,
    vars: Option<&'a MergedVarsJson>,
}

impl<'a> MessageBodyColumns<'a> {
    #[must_use]
    pub fn message_text(self) -> Option<&'a str> {
        self.message_text
    }

    #[must_use]
    pub fn template_sha(self) -> Option<&'a TemplateSha> {
        self.template_sha
    }

    #[must_use]
    pub fn vars(self) -> Option<&'a MergedVarsJson> {
        self.vars
    }
}

impl MessageBody {
    /// Projects this tagged union onto its mutually exclusive durable columns.
    #[must_use]
    pub fn columns(&self) -> MessageBodyColumns<'_> {
        match self {
            Self::Inline(text) | Self::FileRef(text) => MessageBodyColumns {
                message_text: Some(text),
                template_sha: None,
                vars: None,
            },
            Self::Decomposed { template_sha, vars } => MessageBodyColumns {
                message_text: None,
                template_sha: Some(template_sha),
                vars: Some(vars),
            },
        }
    }
}

/// Backend-neutral, already-merged JSON object persisted for a decomposed row.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MergedVarsJson(Map<String, Value>);

impl MergedVarsJson {
    /// Accepts only an object produced by the core-owned merge boundary.
    pub fn from_merged_object(object: Map<String, Value>) -> Self {
        // `serde_json::Map` can only represent a JSON object. Keeping this
        // constructor is intentional: callers cannot substitute an array or
        // scalar when crossing the core/storage contract.
        Self(object)
    }

    #[must_use]
    pub fn as_map(&self) -> &Map<String, Value> {
        &self.0
    }
}

/// Who first made the immutable template visible to this storage backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateFirstSeen {
    pub at: IsoTimestamp,
    pub by: String,
}

/// Persisted output contract selected by the approved renderer adapter.
///
/// This is intentionally an ATM-owned semantic enum rather than an upstream
/// renderer type.  The adapter translates the released upstream classification
/// exactly once at file admission; storage and core only carry this durable
/// value and never infer it from bytes, metadata, or a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateOutputFormat {
    /// A text template with no additional document-format validation contract.
    Text,
    /// A template whose rendered output is a complete JSON document.
    Json,
}

impl TemplateOutputFormat {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
        }
    }

    pub fn parse(value: &str) -> Result<Self, AtmError> {
        match value {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => Err(AtmError::mailbox_read(format!(
                "stored template output_format is invalid: {value:?}"
            ))),
        }
    }
}

impl TemplateFirstSeen {
    pub fn new(at: IsoTimestamp, by: impl Into<String>) -> Result<Self, AtmError> {
        let by = by.into();
        if by.trim().is_empty() {
            return Err(AtmError::validation(
                "template first_seen_by must not be blank",
            ));
        }
        Ok(Self { at, by })
    }
}

/// Immutable registration input for a content-addressed template.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateRegistration {
    pub sha: TemplateSha,
    pub template_type: Option<String>,
    pub template_name: Option<String>,
    pub content_bytes: Vec<u8>,
    pub content_text: String,
    /// Adapter-derived output contract for every newly admitted template.
    pub output_format: TemplateOutputFormat,
    pub frontmatter: TemplateFrontmatter,
    pub first_seen: TemplateFirstSeen,
}

impl TemplateRegistration {
    /// Captures supported raw metadata in the canonical immutable catalog
    /// fields used by workflow-aware consumers.
    pub fn into_normalized_workflow_metadata(mut self) -> Result<Self, AtmError> {
        self.frontmatter = self.frontmatter.with_normalized_workflow_metadata()?;
        Ok(self)
    }

    /// Ensures bytes and strict UTF-8 projection agree before any write begins.
    pub fn validate(&self) -> Result<(), AtmError> {
        let content_text = std::str::from_utf8(&self.content_bytes)
            .map_err(|_| AtmError::template_content_not_utf8())?;
        if content_text != self.content_text {
            return Err(AtmError::validation(
                "template content_text must equal the strict UTF-8 projection of content_bytes",
            ));
        }
        self.frontmatter.validate_workflow_metadata()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateRegistrationOutcome {
    Inserted,
    AlreadyRegistered,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredTemplate {
    pub sha: TemplateSha,
    pub template_type: Option<String>,
    pub template_name: Option<String>,
    pub content_bytes: Vec<u8>,
    pub content_text: String,
    /// `None` is a pre-AN.13 legacy row whose original filename/format is not
    /// trustworthy. It remains readable but cannot be used as checked-render
    /// evidence until the source is re-registered through a current adapter.
    pub output_format: Option<TemplateOutputFormat>,
    pub frontmatter: TemplateFrontmatter,
    pub first_seen: TemplateFirstSeen,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateListFilter {
    pub template_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSummary {
    pub sha: TemplateSha,
    pub template_type: Option<String>,
    pub template_name: Option<String>,
    pub first_seen: TemplateFirstSeen,
}

/// The decomposed-column portion of an already admitted canonical message.
///
/// The message key identifies the mailbox row to convert. This deliberately
/// does not carry an envelope or renderer value: immutable envelope admission
/// remains on `MessageStore`, while this capability atomically registers the
/// template and records the storage-only decomposition columns.
#[derive(Debug, Clone, PartialEq)]
pub struct DecomposedMessageRecord {
    pub key: MessageKey,
    pub template_sha: TemplateSha,
    pub vars: MergedVarsJson,
    pub category: Option<String>,
    /// Caller-supplied instance tags are validated before crossing the sealed
    /// catalog capability boundary.  `tags_json` remains their historical
    /// storage projection.
    pub tags: Vec<InstanceTag>,
    pub content_format: Option<String>,
    /// AN.10 populates this paired snapshot/provenance atomically. AN.9
    /// preserves historical rows and leaves it absent.
    pub workflow: Option<WorkflowAdmission>,
}

/// The workflow snapshot and its derived tag provenance are one admission
/// fact. Keeping them in one value makes a partially-populated pair
/// unrepresentable to callers and storage backends.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowAdmission {
    pub snapshot: WorkflowSnapshot,
    pub tag_provenance: MessageTagProvenance,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecomposedMessageAdmission {
    pub template: TemplateRegistration,
    pub message: DecomposedMessageRecord,
}

/// One atomic Tokio-writer admission: first create the immutable mailbox
/// record, then register and decompose it in the same writer transaction.
/// Keeping both values here makes it impossible for the runtime to expose a
/// plain row when template registration/decomposition has failed.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateMessageAdmission {
    pub record: Message,
    pub decomposition: DecomposedMessageAdmission,
}

impl TemplateMessageAdmission {
    pub fn validate(&self) -> Result<(), AtmError> {
        self.decomposition.validate()?;
        if self.record.message_key != self.decomposition.message.key {
            return Err(AtmError::validation(
                "template message admission key must match its decomposed record key",
            ));
        }
        Ok(())
    }
}

impl DecomposedMessageAdmission {
    /// Reconstructs the only valid tag provenance for this immutable
    /// admission.  The effective set is a materialized search projection;
    /// validating it here prevents callers or storage implementations from
    /// treating it as independently writable data.
    pub fn expected_tag_provenance(
        &self,
        snapshot: &WorkflowSnapshot,
    ) -> Result<MessageTagProvenance, AtmError> {
        Self::expected_tag_provenance_for(
            &self.message.tags,
            &self.template.frontmatter.template_tags,
            self.template.template_type.as_deref(),
            self.message.content_format.as_deref(),
            snapshot,
        )
    }

    /// Computes the immutable provenance projection from the independently
    /// durable source values. Readers use this to detect corrupt stored
    /// projections without reopening a nested catalog connection.
    pub fn expected_tag_provenance_for(
        instance_tags: &[InstanceTag],
        applied_template_tags: &[TemplateTag],
        template_type: Option<&str>,
        content_format: Option<&str>,
        snapshot: &WorkflowSnapshot,
    ) -> Result<MessageTagProvenance, AtmError> {
        let mut derived_tags = Vec::new();
        if let Some(template_type) = template_type {
            derived_tags.push(DerivedTag::new(format!("template-type:{template_type}"))?);
        }
        if let Some(content_format) = content_format {
            derived_tags.push(DerivedTag::new(format!("content-format:{content_format}"))?);
        }
        for (prefix, value) in [
            ("workflow-state:", snapshot.state.as_str()),
            ("workflow-stage:", snapshot.stage.as_str()),
            ("workflow-transition:", snapshot.transition.as_str()),
            ("workflow-scope-kind:", snapshot.scope_kind.as_str()),
        ] {
            derived_tags.push(DerivedTag::new(format!("{prefix}{value}"))?);
        }
        derived_tags.sort();
        derived_tags.dedup();

        let applied_template_tags = applied_template_tags.to_vec();
        let mut effective_values = BTreeSet::new();
        effective_values.extend(instance_tags.iter().map(|tag| tag.as_str()));
        effective_values.extend(applied_template_tags.iter().map(TemplateTag::as_str));
        effective_values.extend(derived_tags.iter().map(DerivedTag::as_str));
        let effective_tags = effective_values
            .into_iter()
            .map(|tag| EffectiveTag::new(tag.to_owned()))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(MessageTagProvenance {
            instance_tags: instance_tags.to_vec(),
            applied_template_tags,
            derived_tags,
            effective_tags,
        })
    }

    pub fn validate(&self) -> Result<(), AtmError> {
        self.template.validate()?;
        if self.message.tags.len() > crate::template_workflow::MAX_INSTANCE_TAGS {
            return Err(AtmError::validation(format!(
                "decomposed message has too many instance tags (maximum {})",
                crate::template_workflow::MAX_INSTANCE_TAGS
            )));
        }
        if self.message.template_sha != self.template.sha {
            return Err(AtmError::validation(
                "decomposed message template_sha must match the registered template SHA",
            ));
        }
        if let Some(workflow) = self.message.workflow.as_ref() {
            let snapshot = &workflow.snapshot;
            let provenance = &workflow.tag_provenance;
            let declaration = self.template.frontmatter.workflow.as_ref().ok_or_else(|| {
                AtmError::validation(
                    "workflow snapshot requires an immutable template workflow declaration",
                )
            })?;
            if snapshot.scope_kind != declaration.scope_kind
                || snapshot.state != declaration.state
                || snapshot.stage != declaration.stage
                || snapshot.transition != declaration.transition
            {
                return Err(AtmError::validation(
                    "workflow snapshot must match the template declaration's fixed fields",
                ));
            }
            let expected = self.expected_tag_provenance(snapshot)?;
            if provenance != &expected {
                return Err(AtmError::validation(
                    "workflow tag provenance must equal the canonical admission projection",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecomposedMessageAdmissionOutcome {
    Inserted {
        template: TemplateRegistrationOutcome,
    },
    MessageAlreadyPresent,
}

/// Sealed backend-neutral capability for immutable templates and the one
/// atomic template-plus-decomposed-message transition.
pub trait TemplateCatalogStore: crate::contract::sealed::Sealed + Send + Sync {
    fn register(
        &self,
        request: TemplateRegistration,
    ) -> Result<TemplateRegistrationOutcome, AtmError>;
    fn load(&self, sha: &TemplateSha) -> Result<Option<StoredTemplate>, AtmError>;
    /// Loads the immutable decomposition columns for one canonical message.
    ///
    /// The default keeps narrow legacy doubles source-compatible; concrete
    /// stores that persist decomposition metadata must override it. Read
    /// surfaces use this capability to render on demand rather than treating
    /// the nullable `message_text` column as the body of truth.
    fn load_decomposed_message(
        &self,
        _key: &MessageKey,
    ) -> Result<Option<DecomposedMessageRecord>, AtmError> {
        Ok(None)
    }
    fn list(&self, filter: TemplateListFilter) -> Result<Vec<TemplateSummary>, AtmError>;
    fn admit_decomposed_message(
        &self,
        admission: DecomposedMessageAdmission,
    ) -> Result<DecomposedMessageAdmissionOutcome, AtmError>;
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct InMemoryTemplateCatalogStore {
    state: std::sync::Mutex<InMemoryTemplateCatalogState>,
}

#[cfg(test)]
#[derive(Default)]
struct InMemoryTemplateCatalogState {
    templates: BTreeMap<TemplateSha, StoredTemplate>,
    decomposed_messages: BTreeMap<MessageKey, DecomposedMessageRecord>,
    fail_next_admission: bool,
}

#[cfg(test)]
impl InMemoryTemplateCatalogStore {
    fn fail_next_admission_for_test(&self) {
        self.state
            .lock()
            .expect("in-memory template catalog lock")
            .fail_next_admission = true;
    }
}

#[cfg(test)]
impl crate::contract::sealed::Sealed for InMemoryTemplateCatalogStore {}

#[cfg(test)]
impl TemplateCatalogStore for InMemoryTemplateCatalogStore {
    fn register(
        &self,
        request: TemplateRegistration,
    ) -> Result<TemplateRegistrationOutcome, AtmError> {
        let request = request.into_normalized_workflow_metadata()?;
        request.validate()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| AtmError::mailbox_write("in-memory template catalog lock poisoned"))?;
        if state.templates.contains_key(&request.sha) {
            return Ok(TemplateRegistrationOutcome::AlreadyRegistered);
        }
        state.templates.insert(
            request.sha.clone(),
            StoredTemplate {
                sha: request.sha,
                template_type: request.template_type,
                template_name: request.template_name,
                content_bytes: request.content_bytes,
                content_text: request.content_text,
                output_format: Some(request.output_format),
                frontmatter: request.frontmatter,
                first_seen: request.first_seen,
            },
        );
        Ok(TemplateRegistrationOutcome::Inserted)
    }

    fn load(&self, sha: &TemplateSha) -> Result<Option<StoredTemplate>, AtmError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| AtmError::mailbox_read("in-memory template catalog lock poisoned"))?
            .templates
            .get(sha)
            .cloned())
    }

    fn list(&self, filter: TemplateListFilter) -> Result<Vec<TemplateSummary>, AtmError> {
        let state = self
            .state
            .lock()
            .map_err(|_| AtmError::mailbox_read("in-memory template catalog lock poisoned"))?;
        Ok(state
            .templates
            .values()
            .filter(|template| {
                filter
                    .template_type
                    .as_ref()
                    .is_none_or(|kind| template.template_type.as_ref() == Some(kind))
            })
            .map(|template| TemplateSummary {
                sha: template.sha.clone(),
                template_type: template.template_type.clone(),
                template_name: template.template_name.clone(),
                first_seen: template.first_seen.clone(),
            })
            .collect())
    }

    fn load_decomposed_message(
        &self,
        key: &MessageKey,
    ) -> Result<Option<DecomposedMessageRecord>, AtmError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| AtmError::mailbox_read("in-memory template catalog lock poisoned"))?
            .decomposed_messages
            .get(key)
            .cloned())
    }

    fn admit_decomposed_message(
        &self,
        admission: DecomposedMessageAdmission,
    ) -> Result<DecomposedMessageAdmissionOutcome, AtmError> {
        let mut admission = admission;
        admission.template = admission.template.into_normalized_workflow_metadata()?;
        admission.validate()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| AtmError::mailbox_write("in-memory template catalog lock poisoned"))?;
        if state
            .decomposed_messages
            .contains_key(&admission.message.key)
        {
            return Ok(DecomposedMessageAdmissionOutcome::MessageAlreadyPresent);
        }
        // Mutate the in-memory state only after the injected failure point,
        // matching the concrete backend's all-or-nothing admission contract.
        if state.fail_next_admission {
            state.fail_next_admission = false;
            return Err(AtmError::mailbox_write(
                "in-memory template catalog injected decomposed-admission failure",
            ));
        }
        let template_outcome = if state.templates.contains_key(&admission.template.sha) {
            TemplateRegistrationOutcome::AlreadyRegistered
        } else {
            let request = admission.template;
            state.templates.insert(
                request.sha.clone(),
                StoredTemplate {
                    sha: request.sha,
                    template_type: request.template_type,
                    template_name: request.template_name,
                    content_bytes: request.content_bytes,
                    content_text: request.content_text,
                    output_format: Some(request.output_format),
                    frontmatter: request.frontmatter,
                    first_seen: request.first_seen,
                },
            );
            TemplateRegistrationOutcome::Inserted
        };
        state
            .decomposed_messages
            .insert(admission.message.key.clone(), admission.message);
        Ok(DecomposedMessageAdmissionOutcome::Inserted {
            template: template_outcome,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registration(bytes: Vec<u8>) -> TemplateRegistration {
        TemplateRegistration {
            sha: TemplateSha::new("a".repeat(64)).expect("sha"),
            template_type: Some("task".to_string()),
            template_name: Some("example".to_string()),
            content_text: String::from_utf8(bytes.clone()).unwrap_or_default(),
            content_bytes: bytes,
            output_format: TemplateOutputFormat::Text,
            frontmatter: TemplateFrontmatter::default(),
            first_seen: TemplateFirstSeen::new(IsoTimestamp::now(), "test").expect("first seen"),
        }
    }

    #[test]
    fn message_body_column_projection_keeps_source_variants_mutually_exclusive() {
        let sha = TemplateSha::new("b".repeat(64)).expect("sha");
        let vars = MergedVarsJson::from_merged_object(Map::from_iter([(
            "priority".to_string(),
            Value::String("high".to_string()),
        )]));

        for body in [
            MessageBody::Inline("inline message".to_string()),
            MessageBody::FileRef("docs/task.xml".to_string()),
        ] {
            let columns = body.columns();
            assert!(columns.message_text().is_some());
            assert!(columns.template_sha().is_none());
            assert!(columns.vars().is_none());
        }

        let decomposed = MessageBody::Decomposed {
            template_sha: sha.clone(),
            vars: vars.clone(),
        };
        let columns = decomposed.columns();
        assert!(columns.message_text().is_none());
        assert_eq!(columns.template_sha(), Some(&sha));
        assert_eq!(columns.vars(), Some(&vars));
    }

    #[test]
    fn in_memory_contract_registration_is_idempotent_and_loads_exact_bytes() {
        let store = InMemoryTemplateCatalogStore::default();
        let request = registration(b"title: example\r\n".to_vec());
        assert_eq!(
            store.register(request.clone()).expect("insert"),
            TemplateRegistrationOutcome::Inserted
        );
        assert_eq!(
            store.register(request.clone()).expect("repeat"),
            TemplateRegistrationOutcome::AlreadyRegistered
        );
        assert_eq!(
            store
                .load(&request.sha)
                .expect("load")
                .expect("template")
                .content_bytes,
            request.content_bytes
        );
    }

    #[test]
    fn invalid_utf8_is_rejected_before_catalog_mutation() {
        let store = InMemoryTemplateCatalogStore::default();
        let request = registration(vec![0xff]);
        assert_eq!(
            store
                .register(request.clone())
                .expect_err("invalid utf8")
                .code()
                .as_str(),
            "TEMPLATE_CONTENT_NOT_UTF8"
        );
        assert!(store.load(&request.sha).expect("load").is_none());
    }

    #[test]
    fn catalog_registration_normalizes_workflow_metadata_before_mutation() {
        let store = InMemoryTemplateCatalogStore::default();
        let mut request = registration(b"template".to_vec());
        request.frontmatter.metadata = serde_json::Map::from_iter([(
            "workflow".to_owned(),
            serde_json::json!({
                "scope": { "kind": "sprint", "variable": "sprint" },
                "state": "dev-start",
                "stage": "dev",
                "transition": "start"
            }),
        )]);
        store.register(request).expect("normalized registration");
        let stored = store
            .load(&TemplateSha::new("a".repeat(64)).expect("sha"))
            .expect("load")
            .expect("stored");
        assert_eq!(
            stored
                .frontmatter
                .workflow
                .expect("workflow")
                .state
                .as_str(),
            "dev-start"
        );
    }

    #[test]
    fn in_memory_admission_failure_leaves_no_partial_catalog_or_message_state() {
        let store = InMemoryTemplateCatalogStore::default();
        let request = registration(b"template".to_vec());
        let sha = request.sha.clone();
        store.fail_next_admission_for_test();
        let error = store
            .admit_decomposed_message(DecomposedMessageAdmission {
                template: request,
                message: DecomposedMessageRecord {
                    key: MessageKey::new("atm:in-memory-template").expect("key"),
                    template_sha: sha.clone(),
                    vars: MergedVarsJson::default(),
                    category: None,
                    tags: vec![],
                    content_format: None,
                    workflow: None,
                },
            })
            .expect_err("injected admission failure");
        assert_eq!(error.code().as_str(), "ATM_MAILBOX_WRITE_FAILED");
        assert!(store.load(&sha).expect("load").is_none());
    }

    #[test]
    fn pre_an10_admission_rejects_snapshot_and_provenance_before_mutation() {
        let store = InMemoryTemplateCatalogStore::default();
        let request = registration(b"template".to_vec());
        let sha = request.sha.clone();
        let error = store
            .admit_decomposed_message(DecomposedMessageAdmission {
                template: request,
                message: DecomposedMessageRecord {
                    key: MessageKey::new("atm:pre-an10-projection").expect("key"),
                    template_sha: sha.clone(),
                    vars: MergedVarsJson::default(),
                    category: None,
                    tags: vec![],
                    content_format: None,
                    workflow: Some(WorkflowAdmission {
                        snapshot: WorkflowSnapshot {
                            scope_kind: crate::template_workflow::WorkflowScopeKind::new("sprint")
                                .expect("kind"),
                            scope_id: crate::template_workflow::WorkflowScopeId::new("an-9")
                                .expect("scope"),
                            state: crate::template_workflow::WorkflowState::new("dev-start")
                                .expect("state"),
                            stage: crate::template_workflow::WorkflowStage::new("dev")
                                .expect("stage"),
                            transition: crate::template_workflow::WorkflowTransition::new("start")
                                .expect("transition"),
                            iteration: None,
                        },
                        tag_provenance: MessageTagProvenance::default(),
                    }),
                },
            })
            .expect_err("AN.9 must not silently discard AN.10 data");
        assert_eq!(error.code().as_str(), "ATM_MESSAGE_VALIDATION_FAILED");
        assert!(store.load(&sha).expect("load").is_none());
    }

    #[test]
    fn decomposed_admission_rejects_instance_tag_count_overflow() {
        let template = registration(b"tag-cap".to_vec());
        let tags = (0..crate::template_workflow::MAX_INSTANCE_TAGS + 1)
            .map(|index| InstanceTag::new(format!("tag-{index}")).expect("tag"))
            .collect();
        let admission = DecomposedMessageAdmission {
            message: DecomposedMessageRecord {
                key: MessageKey::new("atm:tag-cap").expect("key"),
                template_sha: template.sha.clone(),
                vars: MergedVarsJson::default(),
                category: None,
                tags,
                content_format: None,
                workflow: None,
            },
            template,
        };
        let error = admission
            .validate()
            .expect_err("tag cap must reject overflow");
        assert!(error.message().contains("too many instance tags"));
    }

    #[test]
    fn expected_tag_provenance_deduplicates_overlapping_values_deterministically() {
        let snapshot = WorkflowSnapshot {
            scope_kind: crate::template_workflow::WorkflowScopeKind::new("sprint").expect("kind"),
            scope_id: crate::template_workflow::WorkflowScopeId::new("an-10").expect("scope"),
            state: crate::template_workflow::WorkflowState::new("dev-start").expect("state"),
            stage: crate::template_workflow::WorkflowStage::new("dev").expect("stage"),
            transition: crate::template_workflow::WorkflowTransition::new("start")
                .expect("transition"),
            iteration: None,
        };
        let instance = [InstanceTag::new("shared").expect("tag")];
        let template = [TemplateTag::new("shared").expect("tag")];
        let first = DecomposedMessageAdmission::expected_tag_provenance_for(
            &instance,
            &template,
            Some("task"),
            Some("markdown"),
            &snapshot,
        )
        .expect("provenance");
        let second = DecomposedMessageAdmission::expected_tag_provenance_for(
            &instance,
            &template,
            Some("task"),
            Some("markdown"),
            &snapshot,
        )
        .expect("provenance");
        assert_eq!(first, second);
        assert_eq!(
            first
                .effective_tags
                .iter()
                .filter(|tag| tag.as_str() == "shared")
                .count(),
            1
        );
    }
}
