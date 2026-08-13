#![forbid(unsafe_code)]
//! `sc-composer` implementation of ATM's template-composition port.
//!
//! The adapter uses the exact crates.io `sc-composer` and `sc-sha` 1.4.0
//! releases for rendering, root confinement, strict UTF-8/LF-normalized
//! identity, and frontmatter extraction. Its fixture registrations remain
//! deliberately narrow: the upstream crate does not yet expose classified
//! directive spans, so this crate records only that unavailable oracle result
//! instead of growing a second parser or hash implementation.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use atm_core::boundary::sealed;
use atm_core::boundary::{
    RenderedBody, TemplateComposer, TemplateInspection, TemplateReference, TemplateRoot,
    TemplateSource,
};
use atm_core::error::AtmError;
use sc_composer::{ComposePolicy, ConfiningRoot, expand_includes, render_template};
use sc_sha::{HashInput, calculate_hash};
use serde_json::{Map, Value};

/// Production render/confinement adapter with fixture-only directive inspection.
///
/// `inspect` delegates identity and frontmatter extraction to published
/// upstream APIs. It remains registration-backed only for classified directive
/// spans until upstream publishes that API. Every render is delegated to
/// `sc-composer`; callers cannot accidentally exercise a local ATM renderer
/// or loader.
#[derive(Clone, Default)]
pub struct ScComposeTemplateComposer {
    fixture_references: Arc<BTreeMap<Vec<u8>, Vec<TemplateReference>>>,
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

    /// Builds an adapter with fixture-only directive-reference oracle results.
    ///
    /// Each raw source representation is registered explicitly. Identity and
    /// frontmatter never come from this fixture: they are always calculated by
    /// the exact-pinned upstream releases.
    pub fn from_fixture_references(
        fixture_references: impl IntoIterator<Item = (Vec<u8>, Vec<TemplateReference>)>,
    ) -> Self {
        Self {
            fixture_references: Arc::new(fixture_references.into_iter().collect()),
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

    fn inspected_frontmatter(
        raw_file_bytes: &[u8],
    ) -> Result<atm_storage::TemplateFrontmatter, AtmError> {
        let source_text = std::str::from_utf8(raw_file_bytes)
            .map_err(|_| AtmError::template_content_not_utf8())?;
        let parsed = sc_composer::parse_template_document(source_text)
            .map_err(|error| Self::render_error("template frontmatter inspection failed", error))?;
        let Some(frontmatter) = parsed.frontmatter() else {
            return Ok(atm_storage::TemplateFrontmatter::default());
        };
        let defaults = frontmatter
            .defaults()
            .iter()
            .map(|(name, value)| (name.as_str().to_owned(), value.clone()))
            .collect();
        let metadata = frontmatter
            .metadata()
            .iter()
            .map(|(name, value)| {
                value
                    .to_json_value()
                    .map(|value| (name.clone(), value))
                    .map_err(|error| {
                        Self::render_error(
                            "template frontmatter metadata is not JSON-compatible",
                            error,
                        )
                    })
            })
            .collect::<Result<_, _>>()?;
        Ok(atm_storage::TemplateFrontmatter {
            required_variables: frontmatter
                .required_variables()
                .iter()
                .map(|name| name.as_str().to_owned())
                .collect(),
            defaults,
            metadata,
        })
    }

    fn render_error(operation: &str, cause: impl std::fmt::Display) -> AtmError {
        // The caller's AN.3 error mapper assigns the public send-specific code
        // (for example TEMPLATE_INCLUDE_UNRESOLVED) exactly once while this
        // boundary retains the upstream diagnostic as the machine-preserved
        // cause. [cass: helpful starter-rust-errors]
        // Preserve the upstream diagnostic as the primary message as well as
        // the machine-preserved cause.  The standalone sc-compose CLI prints
        // this diagnostic verbatim; keeping it at the ATM boundary makes
        // validation failures actionable and lets callers compare the two
        // process-level surfaces without losing the adapter context.
        let cause = cause.to_string();
        let diagnostic = format!("{operation}: {cause}");
        AtmError::config(diagnostic).with_cause(cause)
    }
}

impl sealed::Sealed for ScComposeTemplateComposer {}

impl TemplateComposer for ScComposeTemplateComposer {
    fn inspect(&self, raw_file_bytes: &[u8]) -> Result<TemplateInspection, AtmError> {
        let sha = calculate_hash(HashInput::TextFileBytes {
            utf8_file_bytes: raw_file_bytes,
        })
        .map_err(|_| AtmError::template_content_not_utf8())?
        .template()
        .to_hex();
        let include_references = self
            .fixture_references
            .get(raw_file_bytes)
            .cloned()
            .ok_or_else(|| {
                AtmError::config(
                    "template has no registered classified directive-inspection result; the pinned upstream API does not expose this result yet",
                )
            })?;
        Ok(TemplateInspection {
            sha: atm_storage::TemplateSha::new(sha)
                .expect("sc-sha always returns a lowercase SHA-256 identity"),
            frontmatter: Self::inspected_frontmatter(raw_file_bytes)?,
            include_references,
        })
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
        let file_bytes = std::fs::read(source_path).map_err(|error| {
            Self::render_error(
                "template source could not be read for render verification",
                error,
            )
        })?;
        if file_bytes != template.raw_file_bytes {
            return Err(AtmError::config(format!(
                "template source changed after it was loaded: '{}' no longer matches the verified raw bytes",
                source_path.display()
            )));
        }
        let confining_root = ConfiningRoot::from_path_buf(root.canonical_path.clone());
        let expanded = expand_includes(source_path, &confining_root, &ComposePolicy::default())
            .map_err(|error| Self::render_error("template include resolution failed", error))?;
        let text = render_template(&expanded.text, vars)
            .map_err(|error| Self::render_error("template render verification failed", error))?;

        Ok(RenderedBody { text })
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
        let current_bytes = std::fs::read(source_path).map_err(|error| {
            Self::render_error("template source could not be read for composition", error)
        })?;
        if current_bytes != template.raw_file_bytes {
            return Err(AtmError::config(format!(
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
        sc_composer::compose(&request)
            .map(|result| RenderedBody {
                text: result.rendered_text,
            })
            .map_err(|error| Self::render_error("template composition failed", error))
    }

    fn render_without_includes(
        &self,
        source: &TemplateSource,
        vars: &Map<String, Value>,
    ) -> Result<RenderedBody, AtmError> {
        let inspection = self.inspect(&source.raw_file_bytes)?;
        if !inspection.include_references.is_empty() {
            return Err(AtmError::decomposed_template_include_forbidden());
        }
        let source_text = Self::source_text(source)?;
        let text = render_template(source_text, vars)
            .map_err(|error| Self::render_error("template render verification failed", error))?;
        Ok(RenderedBody { text })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use atm_core::boundary::{
        SourceSpan, TemplateComposer, TemplateReference, TemplateReferenceKind, TemplateRoot,
        TemplateSource,
    };
    use base64::Engine as _;
    use sc_composer::ConfiningRoot;
    use serde_json::{Map, Value};

    use super::ScComposeTemplateComposer;

    fn source() -> TemplateSource {
        TemplateSource::stored(b"{% include 'child.j2' %}".to_vec())
    }

    fn include_reference() -> TemplateReference {
        TemplateReference {
            directive: TemplateReferenceKind::Include,
            source_span: SourceSpan {
                byte_start: 0,
                byte_end: 24,
            },
        }
    }

    fn no_references() -> Vec<TemplateReference> {
        Vec::new()
    }

    fn temporary_root(label: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("atm-template-{label}-"))
            .tempdir()
            .expect("create isolated template root")
    }

    #[test]
    fn fixture_decomposed_render_rejects_registered_dependencies_before_loader_use() {
        let source = source();
        let composer = ScComposeTemplateComposer::from_fixture_references([(
            source.raw_file_bytes.clone(),
            vec![include_reference()],
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
        let source = TemplateSource::stored(b"hello {{ name }}".to_vec());
        let composer = ScComposeTemplateComposer::from_fixture_references([(
            source.raw_file_bytes.clone(),
            no_references(),
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
    fn sc_sha_matches_all_dolt_identity_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/plans/phase-an/fixtures/dolt-template-sha-vectors.json"
        ))
        .expect("fixture JSON is valid");
        let registrations = vectors["vectors"]
            .as_array()
            .expect("fixture has vectors")
            .iter()
            .map(|vector| {
                (
                    base64::engine::general_purpose::STANDARD
                        .decode(
                            vector["raw_file_bytes_base64"]
                                .as_str()
                                .expect("fixture bytes"),
                        )
                        .expect("fixture base64"),
                    no_references(),
                )
            })
            .collect::<Vec<_>>();
        let composer = ScComposeTemplateComposer::from_fixture_references(registrations);

        for vector in vectors["vectors"].as_array().expect("fixture has vectors") {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(
                    vector["raw_file_bytes_base64"]
                        .as_str()
                        .expect("fixture bytes"),
                )
                .expect("fixture base64");
            assert_eq!(
                composer
                    .inspect(&bytes)
                    .expect("registered inspection")
                    .sha
                    .as_str(),
                vector["sha256"].as_str().expect("fixture SHA"),
                "{}",
                vector["name"].as_str().expect("fixture name")
            );
        }
    }

    #[test]
    fn sc_composer_extracts_frontmatter_while_directive_registration_remains_separate() {
        let raw = b"---\nrequired_variables:\n  - name\ndefaults:\n  greeting: hello\nmetadata:\n  type: task\n---\n{{ greeting }} {{ name }}\n".to_vec();
        let composer =
            ScComposeTemplateComposer::from_fixture_references([(raw.clone(), no_references())]);

        let inspection = composer
            .inspect(&raw)
            .expect("published inspection components");

        assert_eq!(inspection.frontmatter.required_variables, ["name"]);
        assert_eq!(inspection.frontmatter.defaults["greeting"], "hello");
        assert_eq!(inspection.frontmatter.metadata["type"], "task");
        assert!(inspection.include_references.is_empty());
    }

    #[test]
    fn unregistered_directive_inspection_fails_closed() {
        let error = ScComposeTemplateComposer::new()
            .inspect(b"ordinary template")
            .expect_err("unavailable upstream directive inspection must not be guessed");

        assert!(error.message().contains("classified directive-inspection"));
    }

    #[test]
    fn render_error_preserves_operation_context_and_upstream_cause() {
        let error = ScComposeTemplateComposer::render_error("include expansion", "not found");

        assert!(error.message().contains("include expansion: not found"));
        assert_eq!(error.cause(), Some("not found"));
    }

    #[test]
    fn production_adapter_expands_an_in_root_include_then_renders() {
        let root = temporary_root("in-root-include");
        let template_path = root.path().join("main.j2");
        fs::write(&template_path, "@<child.j2>\n").expect("write main template");
        fs::write(root.path().join("child.j2"), "hello {{ name }}").expect("write child template");

        let canonical_root = ConfiningRoot::new(root.path())
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
        assert_eq!(result.expect("in-root render").text, "hello Rand");
    }

    #[test]
    fn production_adapter_compose_matches_sc_compose_file_mode() {
        let root = temporary_root("compose-file");
        let template_path = root.path().join("notice.j2");
        let raw = "---\nrequired_variables:\n  - name\n---\nHello {{ name }}!\n";
        fs::write(&template_path, raw).expect("write template");
        let source = TemplateSource::file_backed(
            raw.as_bytes().to_vec(),
            fs::canonicalize(&template_path).expect("canonical template"),
        );
        let mut vars = Map::new();
        vars.insert("name".to_owned(), Value::String("Rand".to_owned()));
        let composer = ScComposeTemplateComposer::new();
        let canonical_root = fs::canonicalize(root.path()).expect("canonical root");
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
        let root = parent.path().join("root");
        fs::create_dir_all(&root).expect("create root");
        let template_path = root.join("main.j2");
        fs::write(&template_path, "@<../outside.j2>\n").expect("write main template");
        fs::write(parent.path().join("outside.j2"), "must not load")
            .expect("write escaped template");

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
        let error = result.expect_err("escape must be rejected before render");
        assert_eq!(error.code(), atm_storage::AtmErrorCode::ConfigParseFailed);
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
        let template_path = root.path().join("main.j2");
        fs::write(&template_path, "first").expect("write template");
        let canonical_root = fs::canonicalize(root.path()).expect("canonical root");
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
        let error = result.expect_err("changed source must not produce a verification render");
        assert!(error.message().contains("changed after it was loaded"));
    }
}
