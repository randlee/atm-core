#![forbid(unsafe_code)]
//! Fixture-only implementation of ATM's template-composition port.
//!
//! This crate reserves the production `atm-template-sc-compose` boundary while
//! `sc-compose` and `sc-sha` finish their public APIs. It deliberately does
//! not hash bytes, parse frontmatter, inspect directives, or resolve paths.
//! Tests register parser/hash results obtained from an oracle and exercise
//! only ATM's port wiring and fail-closed policy. The published upstream
//! adapter will replace this fixture implementation without changing callers.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use atm_core::boundary::sealed;
use atm_core::boundary::{
    RenderedBody, TemplateComposer, TemplateInspection, TemplateRoot, TemplateSource,
};
use atm_core::error::AtmError;
use serde_json::{Map, Value};

/// Fixture-backed placeholder for the future public `sc-compose` adapter.
///
/// It is intentionally suitable only for contract tests: callers must
/// register every inspection and render result explicitly. This prevents ATM
/// from accidentally acquiring a second hash/parser/loader implementation
/// while the upstream API is unpublished.
#[derive(Clone, Default)]
pub struct ScComposeTemplateComposer {
    inspections: Arc<BTreeMap<Vec<u8>, TemplateInspection>>,
    renders: Arc<BTreeMap<Vec<u8>, RenderedBody>>,
    // [cass: helpful b-mr7cp6x0-ipdnhs] Test-only observability uses an atomic
    // counter so this cloneable fixture stays Send + Sync without a lock.
    root_render_calls: Arc<AtomicUsize>,
}

impl ScComposeTemplateComposer {
    /// Builds a fixture adapter from parser/hash oracle results.
    pub fn from_fixture(
        inspections: impl IntoIterator<Item = (Vec<u8>, TemplateInspection)>,
        renders: impl IntoIterator<Item = (Vec<u8>, RenderedBody)>,
    ) -> Self {
        Self {
            inspections: Arc::new(inspections.into_iter().collect()),
            renders: Arc::new(renders.into_iter().collect()),
            root_render_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Reports attempted loader-backed render operations for contract tests.
    pub fn root_render_calls(&self) -> usize {
        self.root_render_calls.load(Ordering::Relaxed)
    }

    fn registered_render(&self, source: &TemplateSource) -> Result<RenderedBody, AtmError> {
        self.renders
            .get(&source.raw_file_bytes)
            .cloned()
            .ok_or_else(|| AtmError::config("template fixture has no registered render result"))
    }
}

impl sealed::Sealed for ScComposeTemplateComposer {}

impl TemplateComposer for ScComposeTemplateComposer {
    fn inspect(&self, raw_file_bytes: &[u8]) -> Result<TemplateInspection, AtmError> {
        self.inspections
            .get(raw_file_bytes)
            .cloned()
            .ok_or_else(|| AtmError::config("template fixture has no registered inspection result"))
    }

    fn render_within_root(
        &self,
        template: &TemplateSource,
        _vars: &Map<String, Value>,
        _root: &TemplateRoot,
    ) -> Result<RenderedBody, AtmError> {
        self.root_render_calls.fetch_add(1, Ordering::Relaxed);
        self.registered_render(template)
    }

    fn render_without_includes(
        &self,
        source: &TemplateSource,
        _vars: &Map<String, Value>,
    ) -> Result<RenderedBody, AtmError> {
        let inspection = self.inspect(&source.raw_file_bytes)?;
        if !inspection.include_references.is_empty() {
            return Err(AtmError::decomposed_template_include_forbidden());
        }
        self.registered_render(source)
    }
}

#[cfg(test)]
mod tests {
    use atm_core::boundary::{
        SourceSpan, TemplateComposer, TemplateInspection, TemplateReference, TemplateReferenceKind,
        TemplateSource,
    };
    use atm_storage::{TemplateFrontmatter, TemplateSha};
    use serde_json::Map;

    use super::ScComposeTemplateComposer;

    fn source() -> TemplateSource {
        TemplateSource {
            raw_file_bytes: b"{% include 'child.j2' %}".to_vec(),
        }
    }

    fn inspection() -> TemplateInspection {
        TemplateInspection {
            sha: TemplateSha::new(
                "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            )
            .expect("fixture SHA is valid"),
            frontmatter: TemplateFrontmatter::default(),
            include_references: vec![TemplateReference {
                directive: TemplateReferenceKind::Include,
                source_span: SourceSpan {
                    byte_start: 0,
                    byte_end: 24,
                },
            }],
        }
    }

    #[test]
    fn fixture_decomposed_render_rejects_registered_dependencies_before_loader_use() {
        let source = source();
        let composer = ScComposeTemplateComposer::from_fixture(
            [(source.raw_file_bytes.clone(), inspection())],
            [(
                source.raw_file_bytes.clone(),
                atm_core::boundary::RenderedBody {
                    text: "must not render".to_string(),
                },
            )],
        );

        let error = composer
            .render_without_includes(&source, &Map::new())
            .expect_err("fixture marks source as dependency-bearing");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::DecomposedTemplateIncludeForbidden
        );
        assert_eq!(composer.root_render_calls(), 0);
    }

    #[test]
    fn fixture_decomposed_render_uses_registered_parser_proof() {
        let source = TemplateSource {
            raw_file_bytes: b"hello {{ name }}".to_vec(),
        };
        let inspection = TemplateInspection {
            sha: TemplateSha::new(
                "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03",
            )
            .expect("fixture SHA is valid"),
            frontmatter: TemplateFrontmatter::default(),
            include_references: Vec::new(),
        };
        let composer = ScComposeTemplateComposer::from_fixture(
            [(source.raw_file_bytes.clone(), inspection)],
            [(
                source.raw_file_bytes.clone(),
                atm_core::boundary::RenderedBody {
                    text: "hello Rand".to_string(),
                },
            )],
        );

        let rendered = composer
            .render_without_includes(&source, &Map::new())
            .expect("registered dependency-free source renders");

        assert_eq!(rendered.text, "hello Rand");
        assert_eq!(composer.root_render_calls(), 0);
    }
}
