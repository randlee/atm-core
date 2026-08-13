//! Transport-neutral resolution of template-declared workflow metadata.
//!
//! This module is intentionally pure: callers provide already-merged values,
//! and AN.10 decides when the returned snapshot crosses storage admission.

use serde_json::Value;

use crate::error::AtmError;

/// Resolves declared variable names into an immutable workflow snapshot.
pub fn resolve_template_workflow(
    declaration: &atm_storage::TemplateWorkflowDeclaration,
    variables: &atm_storage::MergedVarsJson,
) -> Result<atm_storage::WorkflowSnapshot, AtmError> {
    let scope_id = resolve_scalar(variables, &declaration.scope_variable)?;
    let iteration = declaration
        .iteration_variable
        .as_ref()
        .map(|variable| resolve_scalar(variables, variable))
        .transpose()?;

    // Keep both resolved workflow dimensions typed before constructing the
    // immutable snapshot; neither value crosses the resolver boundary as a
    // raw string after validation.
    let scope_kind: atm_storage::WorkflowScopeKind = declaration.scope_kind.clone();
    let scope_id = atm_storage::WorkflowScopeId::new(scope_id)?;
    let iteration = iteration
        .map(atm_storage::WorkflowIteration::new)
        .transpose()?;
    Ok(atm_storage::WorkflowSnapshot::from_declaration(
        declaration,
        scope_kind,
        scope_id,
        iteration,
    ))
}

fn resolve_scalar(
    variables: &atm_storage::MergedVarsJson,
    variable: &atm_storage::TemplateVariableName,
) -> Result<String, AtmError> {
    let value = variables.as_map().get(variable.as_str()).ok_or_else(|| {
        AtmError::new(
            atm_storage::AtmErrorCode::TemplateWorkflowValueInvalid,
            format!("workflow variable '{}' is missing", variable.as_str()),
        )
    })?;
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => Err(AtmError::new(
            atm_storage::AtmErrorCode::TemplateWorkflowValueInvalid,
            format!(
                "workflow variable '{}' must resolve to a scalar",
                variable.as_str()
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use super::*;

    fn declaration() -> atm_storage::TemplateWorkflowDeclaration {
        atm_storage::TemplateWorkflowDeclaration {
            scope_kind: atm_storage::WorkflowScopeKind::new("release-train").expect("kind"),
            scope_variable: atm_storage::TemplateVariableName::new("train").expect("variable"),
            state: atm_storage::WorkflowState::new("authoring-started").expect("state"),
            stage: atm_storage::WorkflowStage::new("content").expect("stage"),
            transition: atm_storage::WorkflowTransition::new("entered").expect("transition"),
            iteration_variable: Some(
                atm_storage::TemplateVariableName::new("revision").expect("variable"),
            ),
        }
    }

    #[test]
    fn resolves_valid_opaque_scalar_values() {
        let vars = atm_storage::MergedVarsJson::from_merged_object(Map::from_iter([
            ("train".to_owned(), json!(42)),
            ("revision".to_owned(), json!(true)),
        ]));
        let snapshot = resolve_template_workflow(&declaration(), &vars).expect("snapshot");
        assert_eq!(snapshot.scope_id.as_str(), "42");
        assert_eq!(snapshot.iteration.expect("iteration").as_str(), "true");
    }

    #[test]
    fn rejects_missing_null_non_scalar_empty_and_out_of_bounds_values() {
        for value in [
            json!(null),
            json!([]),
            json!({}),
            json!(""),
            json!("x".repeat(257)),
        ] {
            let vars = atm_storage::MergedVarsJson::from_merged_object(Map::from_iter([
                ("train".to_owned(), value),
                ("revision".to_owned(), json!("1")),
            ]));
            assert!(resolve_template_workflow(&declaration(), &vars).is_err());
        }
        let vars = atm_storage::MergedVarsJson::default();
        assert!(resolve_template_workflow(&declaration(), &vars).is_err());
    }
}
