#![forbid(unsafe_code)]
//! `sc-composer` implementation of ATM's template-composition port.
//!
//! The adapter uses the exact crates.io `sc-composer` 1.4.1 release for every
//! render and root-confinement operation. Its fixture registrations remain
//! deliberately narrow: the upstream crate does not yet expose ATM's required
//! LF-normalized content identity or classified directive spans, so this crate
//! records oracle results in tests instead of growing a second parser or hash
//! implementation.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use atm_core::boundary::sealed;
use atm_core::boundary::{
    RenderedBody, TemplateComposer, TemplateInspection, TemplateOutputFormat, TemplateRoot,
    TemplateSource,
};
use atm_core::error::AtmError;
use sc_composer::{
    ComposeError, ComposePolicy, ConfiningRoot, OutputFormat, check_rendered_output,
    expand_includes, render_template,
};
use serde_json::{Map, Value};

/// Production render/confinement adapter with fixture-only inspection support.
///
/// `inspect` remains registration-backed until upstream publishes the required
/// identity and classified-directive APIs. Every render, however, is delegated
/// to `sc-composer` 1.4.1; callers cannot accidentally exercise a local ATM
/// renderer or loader.
#[derive(Clone, Default)]
pub struct ScComposeTemplateComposer {
    inspections: Arc<BTreeMap<Vec<u8>, TemplateInspection>>,
    // [cass: helpful b-mr7cp6x0-ipdnhs] Test-only observability uses an atomic
    // counter so this cloneable adapter stays Send + Sync without a lock.
    root_render_calls: Arc<AtomicUsize>,
}

impl ScComposeTemplateComposer {
    /// Builds the production adapter.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds an adapter with fixture-only parser/hash oracle results.
    ///
    /// Each raw source representation is registered explicitly, while the
    /// supplied identity reflects the production contract: strict UTF-8 with
    /// CRLF and lone-CR normalized to LF before hashing. This fixture does not
    /// duplicate that algorithm; it records its observed result.
    pub fn from_fixture_inspections(
        inspections: impl IntoIterator<Item = (Vec<u8>, TemplateInspection)>,
    ) -> Self {
        Self {
            inspections: Arc::new(inspections.into_iter().collect()),
            root_render_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Reports attempted loader-backed render operations for contract tests.
    pub fn root_render_calls(&self) -> usize {
        self.root_render_calls.load(Ordering::Relaxed)
    }

    fn source_text(source: &TemplateSource) -> Result<&str, AtmError> {
        std::str::from_utf8(&source.raw_file_bytes)
            .map_err(|_| AtmError::template_content_not_utf8())
    }

    fn template_load_error(cause: impl std::fmt::Display) -> AtmError {
        AtmError::new(
            atm_storage::AtmErrorCode::TemplateLoadFailed,
            "template source could not be read",
        )
        .with_cause(cause)
    }

    fn composition_error(error: ComposeError) -> AtmError {
        match error {
            ComposeError::Include(_) | ComposeError::Resolve(_) => {
                AtmError::template_include_unresolved(error)
            }
            ComposeError::Validation(_) | ComposeError::Render(_) | ComposeError::Config(_) => {
                AtmError::template_render_verification_failed(error)
            }
        }
    }

    /// The only ATM production checked-emission seam.  The checker consumes
    /// the complete body, so invalid JSON can never reach a send or read path.
    fn checked_body(
        text: String,
        format: TemplateOutputFormat,
        template_path: &Path,
    ) -> Result<RenderedBody, AtmError> {
        let format = match format {
            TemplateOutputFormat::Text => OutputFormat::Text,
            TemplateOutputFormat::Json => OutputFormat::Json,
        };
        check_rendered_output(format, template_path, &text)
            .map(|checked| RenderedBody {
                text: checked.body().to_owned(),
            })
            .map_err(AtmError::template_render_verification_failed)
    }
}

impl sealed::Sealed for ScComposeTemplateComposer {}

impl TemplateComposer for ScComposeTemplateComposer {
    fn inspect(&self, source: &TemplateSource) -> Result<TemplateInspection, AtmError> {
        let canonical_path = source.canonical_file_path.as_deref().ok_or_else(|| {
            AtmError::config(
                "template inspection requires a canonical source-file path; stored templates retain their admission classification",
            )
        })?;
        let mut inspection = self
            .inspections
            .get(&source.raw_file_bytes)
            .cloned()
            .ok_or_else(|| {
                AtmError::config("template fixture has no registered inspection result")
            })?;
        // This is the sole format classification site. Core and storage only
        // carry the small ATM-owned enum persisted with the immutable row.
        inspection.output_format =
            match sc_composer::OutputFormat::from_template_path(canonical_path) {
                sc_composer::OutputFormat::Text => TemplateOutputFormat::Text,
                sc_composer::OutputFormat::Json => TemplateOutputFormat::Json,
            };
        Ok(inspection)
    }

    fn render_within_root(
        &self,
        template: &TemplateSource,
        vars: &Map<String, Value>,
        root: &TemplateRoot,
    ) -> Result<RenderedBody, AtmError> {
        self.root_render_calls.fetch_add(1, Ordering::Relaxed);
        let source_path = template.canonical_file_path.as_deref().ok_or_else(|| {
            AtmError::config(
                "root-constrained rendering requires a canonical source-file path; stored templates must render without includes",
            )
        })?;
        let file_bytes = std::fs::read(source_path).map_err(Self::template_load_error)?;
        if file_bytes != template.raw_file_bytes {
            return Err(AtmError::template_render_verification_failed(format!(
                "template source changed after it was loaded: '{}' no longer matches the verified raw bytes",
                source_path.display()
            )));
        }
        let confining_root = ConfiningRoot::from_path_buf(root.canonical_path.clone());
        let expanded = expand_includes(source_path, &confining_root, &ComposePolicy::default())
            .map_err(AtmError::template_include_unresolved)?;
        let text = render_template(&expanded.text, vars)
            .map_err(AtmError::template_render_verification_failed)?;

        Self::checked_body(
            text,
            match OutputFormat::from_template_path(source_path) {
                OutputFormat::Text => TemplateOutputFormat::Text,
                OutputFormat::Json => TemplateOutputFormat::Json,
            },
            source_path,
        )
    }

    fn compose_file(
        &self,
        template: &TemplateSource,
        vars: &Map<String, Value>,
        root: &TemplateRoot,
    ) -> Result<RenderedBody, AtmError> {
        let source_path = template
            .canonical_file_path
            .as_deref()
            .ok_or_else(|| AtmError::config("composition requires a canonical source-file path"))?;
        let current_bytes = std::fs::read(source_path).map_err(Self::template_load_error)?;
        if current_bytes != template.raw_file_bytes {
            return Err(AtmError::template_render_verification_failed(format!(
                "template source changed after it was loaded: '{}' no longer matches the verified raw bytes",
                source_path.display()
            )));
        }

        let vars_input = vars
            .iter()
            .map(|(name, value)| {
                let name = sc_composer::VariableName::new(name.clone()).map_err(|error| {
                    AtmError::validation(format!("invalid template variable '{name}': {error}"))
                })?;
                Ok((name, value.clone()))
            })
            .collect::<Result<std::collections::BTreeMap<_, _>, AtmError>>()?;
        let request = sc_composer::ComposeRequest {
            runtime: None,
            mode: sc_composer::ComposeMode::File {
                template_path: source_path.to_path_buf(),
            },
            root: sc_composer::ConfiningRoot::from_path_buf(root.canonical_path.clone()),
            vars_input,
            vars_env: std::collections::BTreeMap::new(),
            vars_defaults: std::collections::BTreeMap::new(),
            guidance_block: None,
            user_prompt: None,
            policy: sc_composer::ComposePolicy {
                allowed_roots: vec![sc_composer::ConfiningRoot::from_path_buf(
                    root.canonical_path.clone(),
                )],
                ..sc_composer::ComposePolicy::default()
            },
        };
        let result = sc_composer::compose(&request).map_err(Self::composition_error)?;
        Self::checked_body(
            result.rendered_text,
            match OutputFormat::from_template_path(source_path) {
                OutputFormat::Text => TemplateOutputFormat::Text,
                OutputFormat::Json => TemplateOutputFormat::Json,
            },
            source_path,
        )
    }

    fn render_without_includes(
        &self,
        source: &TemplateSource,
        vars: &Map<String, Value>,
    ) -> Result<RenderedBody, AtmError> {
        if source.output_format.is_none() {
            return Err(AtmError::mailbox_read(
                "stored template has legacy/unverified output_format; re-register the source through the current adapter before claiming checked-render compatibility",
            ));
        }
        let inspection = self
            .inspections
            .get(&source.raw_file_bytes)
            .cloned()
            .ok_or_else(|| {
                AtmError::config("template fixture has no registered inspection result")
            })?;
        if !inspection.include_references.is_empty() {
            return Err(AtmError::decomposed_template_include_forbidden());
        }
        let source_text = Self::source_text(source)?;
        let text = render_template(source_text, vars)
            .map_err(AtmError::template_render_verification_failed)?;
        let path = match source.output_format.expect("checked above") {
            TemplateOutputFormat::Text => Path::new("stored-template.txt"),
            TemplateOutputFormat::Json => Path::new("stored-template.json"),
        };
        Self::checked_body(text, source.output_format.expect("checked above"), path)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use atm_core::boundary::{
        SourceSpan, TemplateComposer, TemplateInspection, TemplateOutputFormat, TemplateReference,
        TemplateReferenceKind, TemplateRoot, TemplateSource,
    };
    use atm_storage::{TemplateFrontmatter, TemplateSha};
    use sc_composer::ConfiningRoot;
    use serde_json::{Map, Value};

    use super::ScComposeTemplateComposer;

    fn source() -> TemplateSource {
        TemplateSource::stored(
            b"{% include 'child.j2' %}".to_vec(),
            Some(TemplateOutputFormat::Text),
        )
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
            output_format: TemplateOutputFormat::Text,
        }
    }

    fn dependency_free_inspection() -> TemplateInspection {
        TemplateInspection {
            sha: TemplateSha::new(
                "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03",
            )
            .expect("fixture SHA is valid"),
            frontmatter: TemplateFrontmatter::default(),
            include_references: Vec::new(),
            output_format: TemplateOutputFormat::Text,
        }
    }

    fn temporary_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("atm-template-{label}-{nonce}"));
        fs::create_dir_all(&root).expect("create isolated template root");
        root
    }

    #[test]
    fn fixture_decomposed_render_rejects_registered_dependencies_before_loader_use() {
        let source = source();
        let composer = ScComposeTemplateComposer::from_fixture_inspections([(
            source.raw_file_bytes.clone(),
            inspection(),
        )]);

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
        let source = TemplateSource::stored(
            b"hello {{ name }}".to_vec(),
            Some(TemplateOutputFormat::Text),
        );
        let inspection = dependency_free_inspection();
        let composer = ScComposeTemplateComposer::from_fixture_inspections([(
            source.raw_file_bytes.clone(),
            inspection,
        )]);

        let mut vars = Map::new();
        vars.insert("name".to_string(), Value::String("Rand".to_string()));
        let rendered = composer
            .render_without_includes(&source, &vars)
            .expect("registered dependency-free source renders");

        assert_eq!(rendered.text, "hello Rand");
        assert_eq!(composer.root_render_calls(), 0);
    }

    #[test]
    fn checked_emission_rejects_malformed_json_without_leaking_the_body() {
        let source = TemplateSource::stored(
            br#"{\"secret\": "#.to_vec(),
            Some(TemplateOutputFormat::Json),
        );
        let mut proof = dependency_free_inspection();
        proof.output_format = TemplateOutputFormat::Json;
        let composer = ScComposeTemplateComposer::from_fixture_inspections([(
            source.raw_file_bytes.clone(),
            proof,
        )]);

        let error = composer
            .render_without_includes(&source, &Map::new())
            .expect_err("malformed JSON must not leave the adapter");
        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateRenderVerificationFailed
        );
        assert!(error.cause().is_some_and(|cause| !cause.contains("secret")));
    }

    #[test]
    fn checked_emission_rejects_malformed_file_backed_json() {
        let root = temporary_root("checked-json");
        let template_path = root.join("payload.json.j2");
        fs::write(&template_path, br#"{\"secret\": "#).expect("write malformed JSON");
        let source = TemplateSource::file_backed(
            fs::read(&template_path).expect("read template"),
            fs::canonicalize(&template_path).expect("canonical template"),
        );
        let canonical_root = fs::canonicalize(&root).expect("canonical root");
        let error = ScComposeTemplateComposer::new()
            .render_within_root(
                &source,
                &Map::new(),
                &TemplateRoot {
                    canonical_path: canonical_root,
                },
            )
            .expect_err("malformed JSON must not be emitted");
        fs::remove_dir_all(&root).expect("remove isolated template root");
        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateRenderVerificationFailed
        );
        assert!(error.cause().is_some_and(|cause| !cause.contains("secret")));
    }

    #[test]
    fn checked_emission_rejects_malformed_json_from_native_compose() {
        let root = temporary_root("checked-compose-json");
        let template_path = root.join("payload.json.j2");
        fs::write(&template_path, br#"{\"secret\": "#).expect("write malformed JSON");
        let source = TemplateSource::file_backed(
            fs::read(&template_path).expect("read template"),
            fs::canonicalize(&template_path).expect("canonical template"),
        );
        let canonical_root = fs::canonicalize(&root).expect("canonical root");
        let error = ScComposeTemplateComposer::new()
            .compose_file(
                &source,
                &Map::new(),
                &TemplateRoot {
                    canonical_path: canonical_root,
                },
            )
            .expect_err("malformed JSON must not be emitted by compose");
        fs::remove_dir_all(&root).expect("remove isolated template root");
        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateRenderVerificationFailed
        );
        assert!(error.cause().is_some_and(|cause| !cause.contains("secret")));
    }

    #[test]
    fn fixture_records_one_platform_independent_identity_for_lf_and_crlf() {
        let lf = b"hello\n".to_vec();
        let crlf = b"hello\r\n".to_vec();
        let inspection = dependency_free_inspection();
        let composer = ScComposeTemplateComposer::from_fixture_inspections([
            (lf.clone(), inspection.clone()),
            (crlf.clone(), inspection),
        ]);

        assert_eq!(
            composer
                .inspect(&TemplateSource::file_backed(
                    lf.clone(),
                    "notice.txt.j2".into()
                ))
                .expect("LF fixture inspection")
                .sha,
            composer
                .inspect(&TemplateSource::file_backed(
                    crlf.clone(),
                    "notice.txt.j2".into()
                ))
                .expect("CRLF fixture inspection")
                .sha,
            "the fixture preserves the upstream LF-normalized identity contract"
        );
    }

    #[test]
    fn inspection_uses_only_the_upstream_path_classifier_at_file_admission() {
        let raw = b"fixture".to_vec();
        let composer = ScComposeTemplateComposer::from_fixture_inspections([(
            raw.clone(),
            dependency_free_inspection(),
        )]);
        let json = composer
            .inspect(&TemplateSource::file_backed(
                raw.clone(),
                "task.json.j2".into(),
            ))
            .expect("json inspection");
        let text = composer
            .inspect(&TemplateSource::file_backed(raw, "task.md.j2".into()))
            .expect("text inspection");
        assert_eq!(json.output_format, TemplateOutputFormat::Json);
        assert_eq!(text.output_format, TemplateOutputFormat::Text);
    }

    #[test]
    fn production_adapter_expands_an_in_root_include_then_renders() {
        let root = temporary_root("in-root-include");
        let template_path = root.join("main.j2");
        fs::write(&template_path, "@<child.j2>\n").expect("write main template");
        fs::write(root.join("child.j2"), "hello {{ name }}").expect("write child template");

        let canonical_root = ConfiningRoot::new(&root)
            .expect("canonical root")
            .into_inner();
        let source = TemplateSource::file_backed(
            fs::read(&template_path).expect("read template"),
            template_path,
        );
        let mut vars = Map::new();
        vars.insert("name".to_string(), Value::String("Rand".to_string()));

        let result = ScComposeTemplateComposer::new().render_within_root(
            &source,
            &vars,
            &TemplateRoot {
                canonical_path: canonical_root,
            },
        );
        fs::remove_dir_all(&root).expect("remove isolated template root");

        assert_eq!(result.expect("in-root render").text, "hello Rand");
    }

    #[test]
    fn production_adapter_compose_matches_sc_compose_file_mode() {
        let root = temporary_root("compose-file");
        let template_path = root.join("notice.j2");
        let raw = "---\nrequired_variables:\n  - name\n---\nHello {{ name }}!\n";
        fs::write(&template_path, raw).expect("write template");
        let source = TemplateSource::file_backed(
            raw.as_bytes().to_vec(),
            fs::canonicalize(&template_path).expect("canonical template"),
        );
        let mut vars = Map::new();
        vars.insert("name".to_owned(), Value::String("Rand".to_owned()));
        let composer = ScComposeTemplateComposer::new();
        let canonical_root = fs::canonicalize(&root).expect("canonical root");
        let rendered = composer
            .compose_file(
                &source,
                &vars,
                &TemplateRoot {
                    canonical_path: canonical_root,
                },
            )
            .expect("compose through adapter");

        assert_eq!(rendered.text, "Hello Rand!");
    }

    #[test]
    fn production_adapter_rejects_include_escaping_declared_root() {
        let parent = temporary_root("escape-parent");
        let root = parent.join("root");
        fs::create_dir_all(&root).expect("create root");
        let template_path = root.join("main.j2");
        fs::write(&template_path, "@<../outside.j2>\n").expect("write main template");
        fs::write(parent.join("outside.j2"), "must not load").expect("write escaped template");

        let canonical_root = ConfiningRoot::new(&root)
            .expect("canonical root")
            .into_inner();
        let source = TemplateSource::file_backed(
            fs::read(&template_path).expect("read template"),
            template_path,
        );
        let result = ScComposeTemplateComposer::new().render_within_root(
            &source,
            &Map::new(),
            &TemplateRoot {
                canonical_path: canonical_root,
            },
        );
        fs::remove_dir_all(&parent).expect("remove isolated template parent");

        let error = result.expect_err("escape must be rejected before render");
        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateIncludeUnresolved
        );
        assert!(
            error
                .cause()
                .is_some_and(|cause| cause.contains("escapes confinement root")),
            "upstream confinement diagnostic must be preserved: {error}"
        );
    }

    #[test]
    fn production_adapter_rejects_a_file_changed_after_source_capture() {
        let root = temporary_root("changed-after-capture");
        let template_path = root.join("main.j2");
        fs::write(&template_path, "first").expect("write template");
        let canonical_root = fs::canonicalize(&root).expect("canonical root");
        let canonical_template = fs::canonicalize(&template_path).expect("canonical template");
        let source = TemplateSource::file_backed(
            fs::read(&canonical_template).expect("read captured template"),
            canonical_template.clone(),
        );
        fs::write(&canonical_template, "second").expect("modify template after capture");

        let result = ScComposeTemplateComposer::new().render_within_root(
            &source,
            &Map::new(),
            &TemplateRoot {
                canonical_path: canonical_root,
            },
        );
        fs::remove_dir_all(&root).expect("remove isolated template root");

        let error = result.expect_err("changed source must not produce a verification render");
        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateRenderVerificationFailed
        );
        assert!(
            error
                .cause()
                .is_some_and(|cause| cause.contains("changed after it was loaded"))
        );
    }

    #[test]
    fn production_adapter_retains_upstream_validation_diagnostic_as_a_typed_cause() {
        let root = temporary_root("missing-required-variable");
        let template_path = root.join("main.j2");
        fs::write(
            &template_path,
            "---\nrequired_variables:\n  - required\n---\n{{ required }}",
        )
        .expect("write template");
        let canonical_root = fs::canonicalize(&root).expect("canonical root");
        let canonical_template = fs::canonicalize(&template_path).expect("canonical template");
        let source = TemplateSource::file_backed(
            fs::read(&canonical_template).expect("read template"),
            canonical_template,
        );

        let error = ScComposeTemplateComposer::new()
            .compose_file(
                &source,
                &Map::new(),
                &TemplateRoot {
                    canonical_path: canonical_root,
                },
            )
            .expect_err("missing required variable must fail composition");
        fs::remove_dir_all(&root).expect("remove isolated template root");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateRenderVerificationFailed
        );
        assert!(
            error
                .cause()
                .is_some_and(|cause| cause.contains("ERR_VAL_MISSING_REQUIRED")),
            "the typed ATM error must retain the upstream diagnostic in its cause"
        );
    }
}
