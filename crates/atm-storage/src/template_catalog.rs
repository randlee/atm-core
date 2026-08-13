//! Backend-neutral immutable template-catalog contract.
//!
//! This module deliberately owns only durable DTOs and the sealed capability.
//! It neither parses templates nor renders them; the core-owned composition
//! boundary converts validated adapter output into these values exactly once.

#[cfg(test)]
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::contract::{Message, MessageKey};
use crate::error::AtmError;
use crate::template_workflow::{MessageTagProvenance, WorkflowSnapshot, validate_instance_tags};
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
    pub fn try_from_merged_object(object: Map<String, Value>) -> Result<Self, AtmError> {
        // `serde_json::Map` can only represent a JSON object. Keeping this
        // constructor is intentional: callers cannot substitute an array or
        // scalar when crossing the core/storage contract.
        Ok(Self(object))
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
    pub tags: Vec<String>,
    pub content_format: Option<String>,
    /// AN.10 populates this resolved snapshot atomically. AN.9 preserves
    /// historical rows and leaves it absent.
    pub workflow_snapshot: Option<WorkflowSnapshot>,
    /// AN.10 persists the matching source/projection tag sets atomically.
    pub tag_provenance: Option<MessageTagProvenance>,
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
    pub fn validate(&self) -> Result<(), AtmError> {
        self.template.validate()?;
        validate_instance_tags(&self.message.tags)?;
        if self.message.template_sha != self.template.sha {
            return Err(AtmError::validation(
                "decomposed message template_sha must match the registered template SHA",
            ));
        }
        if self.message.workflow_snapshot.is_some() != self.message.tag_provenance.is_some() {
            return Err(AtmError::validation(
                "workflow snapshot and tag provenance must be supplied together",
            ));
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
            frontmatter: TemplateFrontmatter::default(),
            first_seen: TemplateFirstSeen::new(IsoTimestamp::now(), "test").expect("first seen"),
        }
    }

    #[test]
    fn message_body_column_projection_keeps_source_variants_mutually_exclusive() {
        let sha = TemplateSha::new("b".repeat(64)).expect("sha");
        let vars = MergedVarsJson::try_from_merged_object(Map::from_iter([(
            "priority".to_string(),
            Value::String("high".to_string()),
        )]))
        .expect("vars");

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
                    workflow_snapshot: None,
                    tag_provenance: None,
                },
            })
            .expect_err("injected admission failure");
        assert_eq!(error.code().as_str(), "ATM_MAILBOX_WRITE_FAILED");
        assert!(store.load(&sha).expect("load").is_none());
    }
}
