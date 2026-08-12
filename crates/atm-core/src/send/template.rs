//! Core-owned templated-send composition.
//!
//! The CLI only captures caller input. This module owns the deterministic
//! merge, required-variable validation, and renderer-port invocation.

use serde_json::{Map, Value};

use crate::boundary::{RenderedBody, TemplateComposer, TemplateRoot, TemplateSource};
use crate::error::AtmError;

use super::TemplateSendSource;

/// Fully resolved template variables. Environment values have already been
/// captured by the CLI, so this stays reproducible after the HTTP hop.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MergedVars(Map<String, Value>);

impl MergedVars {
    #[must_use]
    pub fn as_map(&self) -> &Map<String, Value> {
        &self.0
    }

    pub fn into_storage_json(self) -> Result<atm_storage::MergedVarsJson, AtmError> {
        atm_storage::MergedVarsJson::try_from_merged_object(self.0)
    }
}

/// Output required by the routing/persistence step after verification.
#[derive(Debug, Clone)]
pub(super) struct VerifiedTemplateSend {
    pub source: TemplateSource,
    pub inspection: crate::boundary::TemplateInspection,
    pub vars: MergedVars,
    pub rendered: RenderedBody,
}

/// The four-cell Decision 5 matrix in one auditable predicate. A direct peer
/// destination denotes the cross-host cells; team inequality denotes the
/// foreign-team cells. Either condition forces a rendered ordinary row.
#[must_use]
pub(super) fn requires_plain_template_fallback(
    inspection: &crate::boundary::TemplateInspection,
    caller_team: &crate::types::TeamName,
    recipient_team: &crate::types::TeamName,
    is_direct_peer_destination: bool,
) -> bool {
    !inspection.include_references.is_empty()
        || caller_team != recipient_team
        || is_direct_peer_destination
}

pub(super) fn verify_template_send(
    composer: &dyn TemplateComposer,
    request: &TemplateSendSource,
    max_message_bytes: usize,
) -> Result<VerifiedTemplateSend, AtmError> {
    let inspection = composer.inspect(&request.raw_file_bytes).map_err(|error| {
        AtmError::new(
            atm_storage::AtmErrorCode::TemplateHashApiFailed,
            error.detail(),
        )
        .with_cause(error)
    })?;
    let vars = resolve_merged_vars(&inspection.frontmatter, request)?;
    let source = TemplateSource::file_backed(
        request.raw_file_bytes.clone(),
        request.canonical_template_path.clone(),
    );
    let root = TemplateRoot {
        canonical_path: request.canonical_template_root.clone(),
    };
    // A loader-constrained render is mandatory even for the include fallback:
    // it proves each dependency is in-root before returning the plain body.
    let rendered = composer
        .render_within_root(&source, vars.as_map(), &root)
        .map_err(|error| {
            if !inspection.include_references.is_empty() {
                AtmError::template_include_unresolved(error)
            } else {
                AtmError::template_render_verification_failed(error)
            }
        })?;
    let rendered = RenderedBody {
        text: super::input::validate_message_text_with_limit(rendered.text, max_message_bytes)?,
    };
    Ok(VerifiedTemplateSend {
        source,
        inspection,
        vars,
        rendered,
    })
}

/// Applies the documented immutable precedence at the core composition seam.
pub fn resolve_merged_vars(
    frontmatter: &atm_storage::TemplateFrontmatter,
    request: &TemplateSendSource,
) -> Result<MergedVars, AtmError> {
    let mut merged = frontmatter.defaults.clone();
    merged.extend(request.var_file_values.clone());
    merged.extend(request.environment_values.clone());
    merged.extend(request.explicit_values.clone());
    for required in &frontmatter.required_variables {
        if !merged.contains_key(required) {
            return Err(AtmError::template_required_variable_missing(required));
        }
    }
    Ok(MergedVars(merged))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::{Map, Value, json};

    use super::{requires_plain_template_fallback, resolve_merged_vars, verify_template_send};
    use crate::boundary::sealed;
    use crate::boundary::{
        RenderedBody, TemplateComposer, TemplateInspection, TemplateRoot, TemplateSource,
    };
    use crate::error::AtmError;
    use crate::send::{TemplateSendSource, input};

    struct FixtureComposer {
        inspection: TemplateInspection,
        render_result: Result<RenderedBody, AtmError>,
        renders: AtomicUsize,
    }

    impl sealed::Sealed for FixtureComposer {}

    impl TemplateComposer for FixtureComposer {
        fn inspect(&self, _raw_file_bytes: &[u8]) -> Result<TemplateInspection, AtmError> {
            Ok(self.inspection.clone())
        }

        fn render_within_root(
            &self,
            _template: &TemplateSource,
            _vars: &Map<String, Value>,
            _root: &TemplateRoot,
        ) -> Result<RenderedBody, AtmError> {
            self.renders.fetch_add(1, Ordering::Relaxed);
            self.render_result.clone()
        }

        fn render_without_includes(
            &self,
            _source: &TemplateSource,
            _vars: &Map<String, Value>,
        ) -> Result<RenderedBody, AtmError> {
            unreachable!("AN.3 verification uses the confined render seam")
        }
    }

    fn source() -> TemplateSendSource {
        TemplateSendSource {
            canonical_template_path: "template.j2".into(),
            canonical_template_root: ".".into(),
            raw_file_bytes: b"hello".to_vec(),
            var_file_values: Map::from_iter([(String::from("name"), json!("file"))]),
            explicit_values: Map::from_iter([(String::from("name"), json!("flag"))]),
            environment_values: Map::from_iter([
                (String::from("name"), json!("environment")),
                (String::from("region"), json!("captured")),
            ]),
        }
    }

    #[test]
    fn merge_precedence_is_explicit_over_file_environment_and_defaults() {
        let frontmatter = atm_storage::TemplateFrontmatter {
            required_variables: vec!["name".to_string(), "region".to_string()],
            defaults: Map::from_iter([
                (String::from("name"), json!("default")),
                (String::from("region"), json!("default-region")),
            ]),
            metadata: Map::new(),
        };
        let merged = resolve_merged_vars(&frontmatter, &source()).expect("merged vars");
        assert_eq!(merged.as_map().get("name"), Some(&json!("flag")));
        assert_eq!(merged.as_map().get("region"), Some(&json!("captured")));
    }

    #[test]
    fn missing_required_variable_fails_before_render() {
        let frontmatter = atm_storage::TemplateFrontmatter {
            required_variables: vec!["missing".to_string()],
            defaults: Map::<String, Value>::new(),
            metadata: Map::new(),
        };
        let error = resolve_merged_vars(&frontmatter, &source()).expect_err("missing var");
        assert!(error.message().contains("required variable 'missing'"));
    }

    #[test]
    fn verification_uses_confined_render_and_captures_the_merged_body() {
        let inspection = TemplateInspection {
            sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .parse()
                .expect("sha"),
            frontmatter: atm_storage::TemplateFrontmatter {
                required_variables: vec!["name".to_owned()],
                defaults: Map::new(),
                metadata: Map::new(),
            },
            include_references: Vec::new(),
        };
        let composer = FixtureComposer {
            inspection,
            render_result: Ok(RenderedBody {
                text: "verified body".to_owned(),
            }),
            renders: AtomicUsize::new(0),
        };

        let verified =
            verify_template_send(&composer, &source(), input::default_message_max_bytes())
                .expect("verification");

        assert_eq!(verified.rendered.text, "verified body");
        assert_eq!(composer.renders.load(Ordering::Relaxed), 1);
        assert_eq!(verified.vars.as_map().get("name"), Some(&json!("flag")));
    }

    #[test]
    fn include_render_failure_is_mapped_to_the_closed_containment_error() {
        let inspection = TemplateInspection {
            sha: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .parse()
                .expect("sha"),
            frontmatter: atm_storage::TemplateFrontmatter::default(),
            include_references: vec![crate::boundary::TemplateReference {
                directive: crate::boundary::TemplateReferenceKind::Include,
                source_span: crate::boundary::SourceSpan {
                    byte_start: 0,
                    byte_end: 1,
                },
            }],
        };
        let composer = FixtureComposer {
            inspection,
            render_result: Err(AtmError::config("include escaped declared root")),
            renders: AtomicUsize::new(0),
        };

        let error = verify_template_send(&composer, &source(), input::default_message_max_bytes())
            .expect_err("unresolved include fails closed");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateIncludeUnresolved
        );
        assert_eq!(composer.renders.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn routing_matrix_decomposes_only_same_team_same_host_without_includes() {
        let inspection = TemplateInspection {
            sha: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                .parse()
                .expect("sha"),
            frontmatter: atm_storage::TemplateFrontmatter::default(),
            include_references: Vec::new(),
        };
        let local_team = "local-team".parse().expect("team");
        let foreign_team = "foreign-team".parse().expect("team");

        assert!(!requires_plain_template_fallback(
            &inspection,
            &local_team,
            &local_team,
            false,
        ));
        assert!(requires_plain_template_fallback(
            &inspection,
            &local_team,
            &local_team,
            true,
        ));
        assert!(requires_plain_template_fallback(
            &inspection,
            &local_team,
            &foreign_team,
            false,
        ));
        assert!(requires_plain_template_fallback(
            &inspection,
            &local_team,
            &foreign_team,
            true,
        ));
    }
}
