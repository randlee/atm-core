//! Backend-neutral template workflow declarations and admission snapshots.
//!
//! These values are deliberately independent of any workflow engine.  ATM
//! validates their durable shape but treats the vocabulary as opaque.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{AtmError, AtmErrorCode, TemplateFrontmatter};

const MAX_IDENTIFIER_BYTES: usize = 128;
const MAX_TAG_BYTES: usize = 256;
const MAX_SNAPSHOT_VALUE_BYTES: usize = 256;

/// Prefixes that only ATM may add to a derived tag projection.
pub const RESERVED_DERIVED_TAG_PREFIXES: [&str; 6] = [
    "template-type:",
    "content-format:",
    "workflow-state:",
    "workflow-stage:",
    "workflow-transition:",
    "workflow-scope-kind:",
];

macro_rules! workflow_identifier {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
                let value = value.into();
                if !is_lower_kebab_identifier(&value) {
                    return Err(AtmError::new(
                        AtmErrorCode::TemplateWorkflowInvalid,
                        concat!($label, " must be a bounded lower-kebab-case identifier"),
                    ));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                String::deserialize(deserializer)
                    .and_then(|value| Self::new(value).map_err(serde::de::Error::custom))
            }
        }
    };
}

workflow_identifier!(WorkflowScopeKind, "workflow scope kind");
workflow_identifier!(WorkflowState, "workflow state");
workflow_identifier!(WorkflowStage, "workflow stage");
workflow_identifier!(WorkflowTransition, "workflow transition");

/// A declared variable name, never a rendered workflow value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct TemplateVariableName(String);

impl TemplateVariableName {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= MAX_IDENTIFIER_BYTES
            && value
                .bytes()
                .enumerate()
                .all(|(_index, byte)| byte.is_ascii_alphanumeric() || byte == b'_')
            && value
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
        if !valid {
            return Err(AtmError::new(
                AtmErrorCode::TemplateWorkflowInvalid,
                "workflow variable names must be bounded identifier names",
            ));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for TemplateVariableName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer)
            .and_then(|value| Self::new(value).map_err(serde::de::Error::custom))
    }
}

/// A literal, template-authored tag.  Derived prefixes are intentionally not
/// accepted here: they are reserved to ATM's admission projection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct TemplateTag(String);

impl TemplateTag {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_TAG_BYTES
            || value.chars().any(char::is_control)
            || contains_template_expression(&value)
        {
            return Err(AtmError::new(
                AtmErrorCode::TemplateWorkflowInvalid,
                "template tags must be bounded literal text without template expressions",
            ));
        }
        if has_reserved_derived_prefix(&value) {
            return Err(AtmError::new(
                AtmErrorCode::TemplateTagReserved,
                "template tags cannot use an ATM-reserved derived-tag prefix",
            ));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for TemplateTag {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer)
            .and_then(|value| Self::new(value).map_err(serde::de::Error::custom))
    }
}

macro_rules! provenance_tag {
    ($name:ident, $label:literal, $doc:literal, $reject_reserved:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
                let value = value.into();
                if !is_literal_tag(&value) {
                    return Err(AtmError::new(
                        AtmErrorCode::TemplateWorkflowInvalid,
                        concat!($label, " must be bounded literal tag text"),
                    ));
                }
                if $reject_reserved && has_reserved_derived_prefix(&value) {
                    return Err(AtmError::new(
                        AtmErrorCode::TemplateTagReserved,
                        concat!($label, " cannot use an ATM-reserved derived-tag prefix"),
                    ));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                String::deserialize(deserializer)
                    .and_then(|value| Self::new(value).map_err(serde::de::Error::custom))
            }
        }
    };
}

provenance_tag!(
    InstanceTag,
    "instance tags",
    "Literal sender/instance tag admitted with a message.",
    true
);
provenance_tag!(
    DerivedTag,
    "derived tags",
    "ATM-generated tag derived from an immutable admission snapshot.",
    false
);
provenance_tag!(
    EffectiveTag,
    "effective tags",
    "Deterministic union used for the indexed query projection.",
    false
);

/// Full workflow declaration captured from immutable template frontmatter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateWorkflowDeclaration {
    pub scope_kind: WorkflowScopeKind,
    pub scope_variable: TemplateVariableName,
    pub state: WorkflowState,
    pub stage: WorkflowStage,
    pub transition: WorkflowTransition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iteration_variable: Option<TemplateVariableName>,
}

/// Template metadata that is captured in canonical, lexical tag order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateTagDeclaration {
    #[serde(default)]
    pub tags: Vec<TemplateTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<TemplateWorkflowDeclaration>,
}

impl TemplateTagDeclaration {
    /// Parses the supported `metadata.tags` and `metadata.workflow` values.
    ///
    /// Any malformed workflow is rejected before a catalog transaction begins.
    pub fn from_frontmatter(frontmatter: &TemplateFrontmatter) -> Result<Self, AtmError> {
        let tags = match frontmatter.metadata.get("tags") {
            None => Vec::new(),
            Some(Value::Array(values)) => values
                .iter()
                .map(|value| {
                    value.as_str().ok_or_else(|| {
                        AtmError::new(
                            AtmErrorCode::TemplateWorkflowInvalid,
                            "metadata.tags must contain only literal strings",
                        )
                    })
                })
                .map(|value| value.and_then(|value| TemplateTag::new(value.to_owned())))
                .collect::<Result<Vec<_>, _>>()?,
            Some(_) => {
                return Err(AtmError::new(
                    AtmErrorCode::TemplateWorkflowInvalid,
                    "metadata.tags must be an array of literal strings",
                ));
            }
        };
        let mut tags = tags;
        tags.sort();
        if tags.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(AtmError::new(
                AtmErrorCode::TemplateWorkflowInvalid,
                "metadata.tags must not contain duplicates",
            ));
        }

        let workflow = match frontmatter.metadata.get("workflow") {
            None => None,
            Some(Value::Object(workflow)) => Some(parse_workflow(workflow)?),
            Some(_) => {
                return Err(AtmError::new(
                    AtmErrorCode::TemplateWorkflowInvalid,
                    "metadata.workflow must be a complete object when present",
                ));
            }
        };
        Ok(Self { tags, workflow })
    }
}

/// Immutable resolved classification persisted on a decomposed message by
/// AN.10.  AN.9 only introduces the leaf type and optional admission shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSnapshot {
    pub scope_kind: WorkflowScopeKind,
    pub scope_id: WorkflowScopeId,
    pub state: WorkflowState,
    pub stage: WorkflowStage,
    pub transition: WorkflowTransition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iteration: Option<WorkflowIteration>,
}

macro_rules! snapshot_value {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
                let value = value.into();
                if value.trim().is_empty()
                    || value.len() > MAX_SNAPSHOT_VALUE_BYTES
                    || value.chars().any(char::is_control)
                {
                    return Err(AtmError::new(
                        AtmErrorCode::TemplateWorkflowValueInvalid,
                        concat!($label, " must be non-empty bounded scalar text"),
                    ));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                String::deserialize(deserializer)
                    .and_then(|value| Self::new(value).map_err(serde::de::Error::custom))
            }
        }
    };
}

snapshot_value!(WorkflowScopeId, "workflow scope id");
snapshot_value!(WorkflowIteration, "workflow iteration");

/// The three source/projection tag sets that AN.10 atomically persists.
/// Kept as raw validated strings here because sender tags and generated tags
/// intentionally have different prefix policies from template-authored tags.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageTagProvenance {
    pub instance_tags: Vec<InstanceTag>,
    pub applied_template_tags: Vec<TemplateTag>,
    pub derived_tags: Vec<DerivedTag>,
    pub effective_tags: Vec<EffectiveTag>,
}

/// Rejects caller tags that impersonate an ATM-generated classification.
pub fn validate_instance_tags(tags: &[String]) -> Result<(), AtmError> {
    for tag in tags {
        let _ = InstanceTag::new(tag.clone())?;
    }
    Ok(())
}

pub(crate) fn has_reserved_derived_prefix(value: &str) -> bool {
    RESERVED_DERIVED_TAG_PREFIXES
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

fn is_lower_kebab_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_BYTES
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn contains_template_expression(value: &str) -> bool {
    value.contains("{{") || value.contains("{%") || value.contains("{#")
}

fn is_literal_tag(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TAG_BYTES
        && !value.chars().any(char::is_control)
        && !contains_template_expression(value)
}

fn parse_workflow(workflow: &Map<String, Value>) -> Result<TemplateWorkflowDeclaration, AtmError> {
    let scope = workflow
        .get("scope")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AtmError::new(
                AtmErrorCode::TemplateWorkflowInvalid,
                "metadata.workflow.scope must be an object with kind and variable",
            )
        })?;
    let scope_kind = required_string(scope, "kind")?.to_owned();
    let scope_variable = required_string(scope, "variable")?.to_owned();
    let iteration_variable = match workflow.get("iteration_variable") {
        None => None,
        Some(Value::String(value)) => Some(TemplateVariableName::new(value.clone())?),
        Some(_) => {
            return Err(AtmError::new(
                AtmErrorCode::TemplateWorkflowInvalid,
                "metadata.workflow.iteration_variable must be a variable name",
            ));
        }
    };
    for key in ["state", "stage", "transition"] {
        let _ = required_string(workflow, key)?;
    }
    Ok(TemplateWorkflowDeclaration {
        scope_kind: WorkflowScopeKind::new(scope_kind)?,
        scope_variable: TemplateVariableName::new(scope_variable)?,
        state: WorkflowState::new(required_string(workflow, "state")?.to_owned())?,
        stage: WorkflowStage::new(required_string(workflow, "stage")?.to_owned())?,
        transition: WorkflowTransition::new(required_string(workflow, "transition")?.to_owned())?,
        iteration_variable,
    })
}

fn required_string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, AtmError> {
    object.get(key).and_then(Value::as_str).ok_or_else(|| {
        AtmError::new(
            AtmErrorCode::TemplateWorkflowInvalid,
            format!("metadata.workflow.{key} must be a literal string"),
        )
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_opaque_workflow_vocabulary_and_canonicalizes_tags() {
        let frontmatter = TemplateFrontmatter {
            metadata: serde_json::Map::from_iter([
                (
                    "workflow".to_owned(),
                    json!({
                        "scope": { "kind": "release-train", "variable": "train_id" },
                        "state": "authoring-started",
                        "stage": "content",
                        "transition": "entered",
                        "iteration_variable": "revision"
                    }),
                ),
                (
                    "tags".to_owned(),
                    json!(["audience:engineering", "domain:templates"]),
                ),
            ]),
            ..TemplateFrontmatter::default()
        };
        let declaration = TemplateTagDeclaration::from_frontmatter(&frontmatter).expect("valid");
        assert_eq!(declaration.tags[0].as_str(), "audience:engineering");
        assert_eq!(
            declaration.workflow.expect("workflow").stage.as_str(),
            "content"
        );
    }

    #[test]
    fn rejects_partial_duplicate_dynamic_and_reserved_metadata() {
        for metadata in [
            json!({"workflow": {"state": "dev-start"}}),
            json!({"tags": ["a", "a"]}),
            json!({"tags": ["{{ dynamic }}"]}),
            json!({"tags": ["workflow-state:spoof"]}),
        ] {
            let frontmatter = TemplateFrontmatter {
                metadata: metadata.as_object().expect("object").clone(),
                ..TemplateFrontmatter::default()
            };
            assert!(TemplateTagDeclaration::from_frontmatter(&frontmatter).is_err());
        }
    }

    #[test]
    fn rejects_reserved_prefixes_for_instance_tags_but_allows_derived_tags() {
        assert!(InstanceTag::new("workflow-state:spoof").is_err());
        assert!(DerivedTag::new("workflow-state:dev-start").is_ok());
        assert!(EffectiveTag::new("workflow-state:dev-start").is_ok());
    }
}
