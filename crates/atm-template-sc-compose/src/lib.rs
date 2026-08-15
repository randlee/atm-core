#![forbid(unsafe_code)]
//! `sc-composer` implementation of ATM's template-composition port.
//!
//! The adapter uses the exact crates.io `sc-composer` and `sc-sha` 1.4.1
//! releases for inspection, rendering, and root-confinement operations. ATM
//! translates their public results into its small storage DTOs; it does not
//! maintain a second parser, hash implementation, or fixture-backed
//! production path.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use atm_core::boundary::sealed;
use atm_core::boundary::{
    RenderedBody, SourceSpan, TemplateComposer, TemplateInspection, TemplateOutputFormat,
    TemplateReference, TemplateReferenceKind, TemplateRoot, TemplateSource,
};
use atm_core::error::AtmError;
use sc_composer::{
    ComposeError, ComposePolicy, ConfiningRoot, OutputFormat, RenderCheckMeta,
    TemplateDirectiveKind, check_rendered_output_with_meta, expand_includes,
    inspect_template_directives, parse_template_document, render_template,
};
use sc_sha::{HashInput, calculate_hash};
use serde_json::{Map, Value};

/// Production render, confinement, and inspection adapter.
///
/// Every production operation delegates to the exact-pinned upstream crates:
/// `sc-sha` owns strict UTF-8/LF-normalized identity while `sc-composer` owns
/// frontmatter parsing, directive classification, rendering, and confinement.
#[derive(Clone, Default)]
pub struct ScComposeTemplateComposer {
    // [cass: helpful b-mr7cp6x0-ipdnhs] Test-only observability uses an atomic
    // counter so this cloneable adapter stays Send + Sync without a lock.
    root_render_calls: Arc<AtomicUsize>,
}

impl ScComposeTemplateComposer {
    const fn to_sc_output_format(format: TemplateOutputFormat) -> OutputFormat {
        match format {
            TemplateOutputFormat::Text => OutputFormat::Text,
            TemplateOutputFormat::Json => OutputFormat::Json,
        }
    }

    const fn from_sc_output_format(format: OutputFormat) -> TemplateOutputFormat {
        match format {
            OutputFormat::Text => TemplateOutputFormat::Text,
            OutputFormat::Json => TemplateOutputFormat::Json,
        }
    }
    /// Builds the production adapter.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
            ComposeError::Include(error) => AtmError::template_include_unresolved(error),
            ComposeError::Resolve(error) => AtmError::template_include_unresolved(error),
            ComposeError::Validation(error) => AtmError::template_render_verification_failed(error),
            ComposeError::Render(error) => AtmError::template_render_verification_failed(error),
            ComposeError::Config(error) => Self::inspection_parse_error(error),
        }
    }

    fn inspection_parse_error(cause: impl std::fmt::Display) -> AtmError {
        AtmError::new(
            atm_storage::AtmErrorCode::TemplateInspectionParseFailed,
            "template inspection parser rejected frontmatter or body/directive syntax",
        )
        .with_cause(cause)
    }

    fn inspection_error(error: ComposeError) -> AtmError {
        match error {
            ComposeError::Include(error) => AtmError::template_include_unresolved(error),
            ComposeError::Resolve(error) => AtmError::template_include_unresolved(error),
            ComposeError::Validation(error) => AtmError::template_render_verification_failed(error),
            ComposeError::Render(error) => AtmError::template_render_verification_failed(error),
            ComposeError::Config(error) => Self::inspection_parse_error(error),
        }
    }

    fn hash_api_error(cause: impl std::fmt::Display) -> AtmError {
        AtmError::new(
            atm_storage::AtmErrorCode::TemplateHashApiFailed,
            "template SHA/hash identity API failed to produce a valid identity",
        )
        .with_cause(cause)
    }

    fn upstream_frontmatter(
        raw_file_bytes: &[u8],
    ) -> Result<atm_storage::TemplateFrontmatter, AtmError> {
        let source = std::str::from_utf8(raw_file_bytes)
            .map_err(|_| AtmError::template_content_not_utf8())?;
        let document = parse_template_document(source).map_err(Self::inspection_error)?;
        let Some(frontmatter) = document.frontmatter() else {
            return Ok(atm_storage::TemplateFrontmatter::default());
        };
        let required_variables = frontmatter
            .required_variables()
            .iter()
            .map(|name| atm_storage::TemplateVariableName::new(name.as_str()))
            .collect::<Result<Vec<_>, _>>()?;
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
                    .map_err(Self::inspection_parse_error)
            })
            .collect::<Result<_, _>>()?;
        Ok(atm_storage::TemplateFrontmatter {
            required_variables,
            defaults,
            metadata,
            ..atm_storage::TemplateFrontmatter::default()
        })
    }

    fn upstream_references(raw_file_bytes: &[u8]) -> Result<Vec<TemplateReference>, AtmError> {
        inspect_template_directives(raw_file_bytes)
            .map_err(Self::inspection_error)?
            .into_iter()
            .map(|reference| {
                let directive = match reference.directive {
                    TemplateDirectiveKind::Include => TemplateReferenceKind::Include,
                    TemplateDirectiveKind::Import => TemplateReferenceKind::Import,
                    TemplateDirectiveKind::FromImport => TemplateReferenceKind::FromImport,
                };
                Ok(TemplateReference {
                    directive,
                    source_span: SourceSpan {
                        byte_start: reference.source_span.byte_start,
                        byte_end: reference.source_span.byte_end,
                    },
                })
            })
            .collect()
    }

    fn inspect_raw_file(
        raw_file_bytes: &[u8],
        output_format: TemplateOutputFormat,
    ) -> Result<TemplateInspection, AtmError> {
        let sha = calculate_hash(HashInput::TextFileBytes {
            utf8_file_bytes: raw_file_bytes,
        })
        .map_err(|_| AtmError::template_content_not_utf8())?
        .template()
        .to_hex();
        Ok(TemplateInspection {
            sha: atm_storage::TemplateSha::new(sha).map_err(Self::hash_api_error)?,
            frontmatter: Self::upstream_frontmatter(raw_file_bytes)?,
            include_references: Self::upstream_references(raw_file_bytes)?,
            output_format,
        })
    }

    /// Reject a source file that changed after ATM captured its immutable
    /// admission bytes. Both confined render entry points share this check.
    fn verify_unchanged(source_path: &Path, expected: &[u8]) -> Result<(), AtmError> {
        let actual = std::fs::read(source_path).map_err(Self::template_load_error)?;
        if actual == expected {
            return Ok(());
        }
        Err(AtmError::template_render_verification_failed(format!(
            "template source changed after it was loaded: '{}' no longer matches the verified raw bytes",
            source_path.display()
        )))
    }

    /// The only ATM production checked-emission seam.  The checker consumes
    /// the complete body, so invalid JSON can never reach a send or read path.
    fn checked_body(
        text: &str,
        format: TemplateOutputFormat,
        template_path: &Path,
        failing_pass: Option<u8>,
    ) -> Result<RenderedBody, AtmError> {
        let format = Self::to_sc_output_format(format);
        let meta = RenderCheckMeta::for_template_with_format(template_path, format);
        check_rendered_output_with_meta(meta, text)
            .map(|checked| RenderedBody {
                text: checked.body().to_owned(),
            })
            .map_err(|error| {
                let error = error.with_failing_pass(failing_pass);
                let cause = failing_pass.map_or_else(
                    || error.to_string(),
                    |pass| format!("{error}; final output rejected after render pass {pass}"),
                );
                match format {
                    OutputFormat::Json => AtmError::template_json_escape_migration_required(cause),
                    OutputFormat::Text => AtmError::template_render_verification_failed(cause),
                }
            })
    }
}

impl sealed::Sealed for ScComposeTemplateComposer {}

impl TemplateComposer for ScComposeTemplateComposer {
    fn inspect(&self, source: &TemplateSource) -> Result<TemplateInspection, AtmError> {
        let canonical_path = source.canonical_file_path.as_deref().ok_or_else(|| {
            AtmError::new(
                atm_storage::AtmErrorCode::TemplateClassificationInvalid,
                "template inspection requires a canonical source-file path; stored templates retain their admission classification",
            )
        })?;
        // This is the sole format classification site. Core and storage only
        // carry the small ATM-owned enum persisted with the immutable row.
        Self::inspect_raw_file(
            &source.raw_file_bytes,
            Self::from_sc_output_format(OutputFormat::from_template_path(canonical_path)),
        )
    }

    fn render_within_root(
        &self,
        template: &TemplateSource,
        vars: &Map<String, Value>,
        root: &TemplateRoot,
    ) -> Result<RenderedBody, AtmError> {
        self.root_render_calls.fetch_add(1, Ordering::Relaxed);
        let source_path = template.canonical_file_path.as_deref().ok_or_else(|| {
            AtmError::new(
                atm_storage::AtmErrorCode::TemplateClassificationInvalid,
                "root-constrained rendering requires a canonical source-file path; stored templates must render without includes",
            )
        })?;
        Self::verify_unchanged(source_path, &template.raw_file_bytes)?;
        let confining_root = ConfiningRoot::from_path_buf(root.canonical_path.clone());
        let expanded = expand_includes(source_path, &confining_root, &ComposePolicy::default())
            .map_err(AtmError::template_include_unresolved)?;
        let text = render_template(&expanded.text, vars)
            .map_err(AtmError::template_render_verification_failed)?;

        Self::checked_body(
            &text,
            Self::from_sc_output_format(OutputFormat::from_template_path(source_path)),
            source_path,
            None,
        )
    }

    fn compose_file(
        &self,
        template: &TemplateSource,
        vars: &Map<String, Value>,
        root: &TemplateRoot,
    ) -> Result<RenderedBody, AtmError> {
        let source_path = template.canonical_file_path.as_deref().ok_or_else(|| {
            AtmError::new(
                atm_storage::AtmErrorCode::TemplateClassificationInvalid,
                "composition requires a canonical source-file path",
            )
        })?;
        Self::verify_unchanged(source_path, &template.raw_file_bytes)?;

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
            // ATM's template-send contract contains only the rendered
            // template body and captured variables. Guidance and user-prompt
            // blocks are higher-level sc-composer features, so they must not
            // be invented or appended by this adapter.
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
            &result.rendered_text,
            Self::from_sc_output_format(OutputFormat::from_template_path(source_path)),
            source_path,
            result.failing_pass,
        )
    }

    fn render_without_includes(
        &self,
        source: &TemplateSource,
        vars: &Map<String, Value>,
    ) -> Result<RenderedBody, AtmError> {
        let output_format = source.output_format.ok_or_else(|| {
            AtmError::mailbox_read(
                "stored template has legacy/unverified output_format; re-register the source through the current adapter before claiming checked-render compatibility",
            )
        })?;
        let inspection = Self::inspect_raw_file(&source.raw_file_bytes, output_format)?;
        if !inspection.include_references.is_empty() {
            return Err(AtmError::decomposed_template_include_forbidden());
        }
        let source_text = Self::source_text(source)?;
        let text = render_template(source_text, vars)
            .map_err(AtmError::template_render_verification_failed)?;
        let path = match output_format {
            TemplateOutputFormat::Text => Path::new("stored-template.txt"),
            TemplateOutputFormat::Json => Path::new("stored-template.json"),
        };
        Self::checked_body(&text, output_format, path, None)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use atm_core::boundary::{
        TemplateComposer, TemplateOutputFormat, TemplateReferenceKind, TemplateRoot, TemplateSource,
    };
    use base64::Engine as _;
    use sc_composer::ConfiningRoot;
    use serde_json::{Map, Value, json};

    use super::ScComposeTemplateComposer;

    fn source() -> TemplateSource {
        TemplateSource::stored(
            b"{% include 'child.j2' %}".to_vec(),
            Some(TemplateOutputFormat::Text),
        )
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
    fn decomposed_render_rejects_upstream_inspected_dependencies_before_loader_use() {
        let source = source();
        let composer = ScComposeTemplateComposer::new();

        let error = composer
            .render_without_includes(&source, &Map::new())
            .expect_err("upstream inspection marks source as dependency-bearing");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::DecomposedTemplateIncludeForbidden
        );
        assert_eq!(composer.root_render_calls(), 0);
    }

    #[test]
    fn decomposed_render_uses_upstream_parser_proof() {
        let source = TemplateSource::stored(
            b"hello {{ name }}".to_vec(),
            Some(TemplateOutputFormat::Text),
        );
        let composer = ScComposeTemplateComposer::new();

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
        let composer = ScComposeTemplateComposer::new();

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
    fn an15_boundary_probe_rejection_leaves_send_fallback_and_read_inputs_immutable() {
        let malformed = br#"{\"secret\": "#;

        // Template send and its rendered-fallback variant both invoke this
        // confined render before their persistence branches. A rejection has
        // no `RenderedBody` that any caller could persist, cache, or export.
        let root = temporary_root("checked-route-inputs");
        let template_path = root.join("payload.json.j2");
        fs::write(&template_path, malformed).expect("write malformed JSON");
        let source = TemplateSource::file_backed(
            fs::read(&template_path).expect("read template"),
            fs::canonicalize(&template_path).expect("canonical template"),
        );
        let source_before = source.clone();
        let vars = Map::from_iter([("name".to_owned(), Value::String("Rand".to_owned()))]);
        let vars_before = vars.clone();
        let error = ScComposeTemplateComposer::new()
            .render_within_root(
                &source,
                &vars,
                &TemplateRoot {
                    canonical_path: fs::canonicalize(&root).expect("canonical root"),
                },
            )
            .expect_err("template send must reject malformed JSON before persistence");
        assert_eq!(source, source_before, "send source must remain immutable");
        assert_eq!(vars, vars_before, "send variables must remain immutable");
        assert!(error.cause().is_some_and(|cause| !cause.contains("secret")));
        fs::remove_dir_all(&root).expect("remove isolated template root");

        // Render-on-read receives a stored source and captured variables. The
        // core read seam is pure and delegates these exact values to this
        // adapter's no-include port, so the adapter proves it cannot mutate
        // either value on rejection.
        let stored = TemplateSource::stored(malformed.to_vec(), Some(TemplateOutputFormat::Json));
        let stored_before = stored.clone();
        let merged_vars = vars_before;
        let merged_vars_before = merged_vars.clone();
        let composer = ScComposeTemplateComposer::new();
        let error = composer
            .render_without_includes(&stored, &merged_vars)
            .expect_err("decomposed read must reject malformed JSON without mutation");
        assert_eq!(
            stored, stored_before,
            "catalog source must remain immutable"
        );
        assert_eq!(
            merged_vars, merged_vars_before,
            "captured render variables must remain immutable"
        );
        assert!(error.cause().is_some_and(|cause| !cause.contains("secret")));
    }

    #[test]
    fn an15_template_probe_regresses_historical_sc_compose_json_escape_migration() {
        let injected = r#"x\", \"injected\": true, \"y\": \"x"#;
        // These are the two exact placeholder forms changed by ATM's
        // historical sc-compose 1.4.x migration (95899a6f0 / PR #869). The
        // `auto` form is the repaired source; `legacy` preserves a valid
        // migration option for a deliberately manually quoted template.
        for (label, frontmatter, body) in [
            ("auto", "", r#"{"review_mode": {{ review_mode }}}"#),
            (
                "legacy",
                "---\njson_escape_mode: legacy\n---\n",
                r#"{"review_mode": "{{ review_mode }}"}"#,
            ),
        ] {
            let root = temporary_root(&format!("json-{label}-escape"));
            let template_path = root.join("payload.json.j2");
            fs::write(&template_path, format!("{frontmatter}{body}")).expect("write JSON template");
            let source = TemplateSource::file_backed(
                fs::read(&template_path).expect("read template"),
                fs::canonicalize(&template_path).expect("canonical template"),
            );
            let mut vars = Map::new();
            vars.insert("review_mode".to_owned(), Value::String(injected.to_owned()));
            let rendered = ScComposeTemplateComposer::new()
                .compose_file(
                    &source,
                    &vars,
                    &TemplateRoot {
                        canonical_path: fs::canonicalize(&root).expect("canonical root"),
                    },
                )
                .expect("both upstream JSON escape modes must produce checked JSON");
            fs::remove_dir_all(&root).expect("remove isolated template root");

            let parsed: Value = serde_json::from_str(&rendered.text).expect("checked JSON body");
            assert_eq!(parsed["review_mode"], Value::String(injected.to_owned()));
            assert!(parsed.get("injected").is_none());
        }

        // A pre-1.4.x ATM JSON template did quote the placeholder manually.
        // It must fail closed under the 1.4.x default without a panic, raw
        // rendered content, or an opaque error that leaves a newly installed
        // agent unable to repair the source.
        let root = temporary_root("an15-historical-auto-escape");
        let template_path = root.join("rust-best-practices-assignment.json.j2");
        fs::write(&template_path, r#"{"review_mode": "{{ review_mode }}"}"#)
            .expect("write historical pre-1.4.x auto-escape template");
        let source = TemplateSource::file_backed(
            fs::read(&template_path).expect("read historical template"),
            fs::canonicalize(&template_path).expect("canonical historical template"),
        );
        let error = ScComposeTemplateComposer::new()
            .compose_file(
                &source,
                &Map::from_iter([("review_mode".to_owned(), Value::String(injected.to_owned()))]),
                &TemplateRoot {
                    canonical_path: fs::canonicalize(&root).expect("canonical root"),
                },
            )
            .expect_err("pre-1.4.x manually quoted template must fail closed");
        fs::remove_dir_all(&root).expect("remove isolated template root");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateRenderVerificationFailed
        );
        assert!(
            error
                .message()
                .contains("sc-compose automatic JSON escaping")
        );
        assert!(error.message().contains("json_escape_mode: legacy"));
        assert!(
            error.cause().is_some_and(|cause| !cause.contains(injected)),
            "raw rendered values must not leak through the preserved cause"
        );
        eprintln!(
            "AN15_DIAGNOSTIC legacy-json-escape code={} message={}",
            error.code(),
            error.message(),
        );
    }

    #[test]
    fn an15_template_probe_checked_json_is_deterministic_for_unicode_and_escape_vectors() {
        let root = temporary_root("an15-unicode-escape");
        let template_path = root.join("payload.json.j2");
        fs::write(&template_path, r#"{"value": {{ value }}}"#).expect("write JSON template");
        let source = TemplateSource::file_backed(
            fs::read(&template_path).expect("read template"),
            fs::canonicalize(&template_path).expect("canonical template"),
        );
        let root = TemplateRoot {
            canonical_path: fs::canonicalize(&root).expect("canonical root"),
        };
        let composer = ScComposeTemplateComposer::new();

        for case_index in 0..100 {
            let value = format!("case-{case_index}-🙂-漢字-\\\"-newline\\n");
            let vars = Map::from_iter([("value".to_owned(), Value::String(value.clone()))]);
            let rendered = composer
                .compose_file(&source, &vars, &root)
                .expect("checked JSON must remain valid");
            assert_eq!(
                serde_json::from_str::<Value>(&rendered.text).expect("checked JSON")["value"],
                Value::String(value),
            );
        }
        fs::remove_dir_all(&root.canonical_path).expect("remove isolated template root");
    }

    #[test]
    fn an15_template_probe_composes_realistic_jinja_syntax_and_text_target_corpus() {
        let root = temporary_root("an15-realistic-syntax");
        let primary_path = root.join("workflow.txt.j2");
        fs::write(
            &primary_path,
            concat!(
                "{% macro label(value) %}{{ value | trim | default(\"none\") }}{% endmacro %}\n",
                "{% macro state(value) %}{{ \"open\" if value else \"closed\" }}{% endmacro %}\n",
                "{% set deviations = report.rows | selectattr(\"verdict\", \"ne\", \"PASS\") | list -%}\n",
                "{{ report.owner.name }}|{{ state(enabled) }}|{{ label(report.title) }}|",
                "{% for row in deviations %}{{ row.get(\"id\", \"n/a\") }}{% if not loop.last %},{% endif %}{% endfor %}",
                "\n",
            ),
        )
        .expect("write realistic primary template");
        let source = TemplateSource::file_backed(
            fs::read(&primary_path).expect("read primary template"),
            fs::canonicalize(&primary_path).expect("canonical primary template"),
        );
        let template_root = TemplateRoot {
            canonical_path: fs::canonicalize(&root).expect("canonical template root"),
        };
        let composer = ScComposeTemplateComposer::new();

        for case_index in 0..100 {
            let vars = Map::from_iter([
                ("enabled".to_owned(), Value::Bool(case_index % 2 == 0)),
                (
                    "report".to_owned(),
                    json!({
                        "owner": {"name": format!("dev-{case_index}")},
                        "title": format!("  phase-an-{case_index}  "),
                        "rows": [
                            {"id": format!("PASS-{case_index}"), "verdict": "PASS"},
                            {"id": format!("FIX-{case_index}"), "verdict": "FAIL"},
                            {"id": format!("WARN-{case_index}"), "verdict": "WARN"}
                        ]
                    }),
                ),
            ]);
            let rendered = composer
                .compose_file(&source, &vars, &template_root)
                .expect("realistic supported Jinja constructs must compose");
            let expected_state = if case_index % 2 == 0 { "open" } else { "closed" };
            assert!(
                rendered.text.contains(&format!(
                    "dev-{case_index}|{expected_state}|phase-an-{case_index}|FIX-{case_index},WARN-{case_index}"
                )),
                "case {case_index} rendered unexpected text: {}",
                rendered.text
            );
        }

        // Text targets stay deliberately format-neutral: ATM checks JSON only
        // for JSON paths, while common YAML/XML/HTML/code payloads are safely
        // rendered as text and retain their exact protocol characters.
        for (file_name, body, expected) in [
            ("workflow.yaml.j2", "title: {{ title }}\r\nitems:\r\n  - {{ item }}\r", "title: an15"),
            ("notice.xml.j2", "<notice owner=\"{{ owner }}\">{{ title }}</notice>", "<notice owner=\"dev\">an15</notice>"),
            ("report.html.j2", "<h1>{{ title }}</h1><p>{{ item }}</p>", "<h1>an15</h1>"),
            ("handler.rs.j2", "const TITLE: &str = \"{{ title }}\";\n", "const TITLE: &str = \"an15\";"),
            ("bom.txt.j2", "\u{feff}title={{ title }}\n", "title=an15"),
        ] {
            let path = root.join(file_name);
            fs::write(&path, body).expect("write text target template");
            let text_source = TemplateSource::file_backed(
                fs::read(&path).expect("read text target template"),
                fs::canonicalize(&path).expect("canonical text target template"),
            );
            let rendered = composer
                .compose_file(
                    &text_source,
                    &Map::from_iter([
                        ("title".to_owned(), Value::String("an15".to_owned())),
                        ("item".to_owned(), Value::String("checked-emission".to_owned())),
                        ("owner".to_owned(), Value::String("dev".to_owned())),
                    ]),
                    &template_root,
                )
                .expect("realistic text target must compose without a format-specific panic");
            assert!(rendered.text.contains(expected), "{file_name}: {}", rendered.text);
        }
        fs::remove_dir_all(&root).expect("remove isolated realistic syntax root");
    }

    #[test]
    fn an15_template_probe_reports_the_failing_second_render_pass() {
        let root = temporary_root("checked-multipass-json");
        let template_path = root.join("payload.json.j2");
        fs::write(
            &template_path,
            "---\npass: 2\n---\n---\npass: 1\n---\n{\"value\": \"{{{ late | safe }}}\"}",
        )
        .expect("write multipass JSON template");
        let source = TemplateSource::file_backed(
            fs::read(&template_path).expect("read template"),
            fs::canonicalize(&template_path).expect("canonical template"),
        );
        let mut vars = Map::new();
        vars.insert(
            "late".to_owned(),
            Value::String("unterminated\" value".to_owned()),
        );
        let error = ScComposeTemplateComposer::new()
            .compose_file(
                &source,
                &vars,
                &TemplateRoot {
                    canonical_path: fs::canonicalize(&root).expect("canonical root"),
                },
            )
            .expect_err("the checked final body must reject malformed second-pass JSON");
        fs::remove_dir_all(&root).expect("remove isolated template root");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateRenderVerificationFailed
        );
        assert!(
            error
                .cause()
                .is_some_and(|cause| cause.contains("after render pass 2")),
            "the adapter must retain upstream pass provenance: {error}"
        );
    }

    #[test]
    fn inspection_uses_sc_sha_for_one_platform_independent_identity_for_lf_and_crlf() {
        let lf = b"hello\n".to_vec();
        let crlf = b"hello\r\n".to_vec();
        let composer = ScComposeTemplateComposer::new();

        assert_eq!(
            composer
                .inspect(&TemplateSource::file_backed(
                    lf.clone(),
                    "notice.txt.j2".into()
                ))
                .expect("LF upstream inspection")
                .sha,
            composer
                .inspect(&TemplateSource::file_backed(
                    crlf.clone(),
                    "notice.txt.j2".into()
                ))
                .expect("CRLF upstream inspection")
                .sha,
            "sc-sha preserves the upstream LF-normalized identity contract"
        );
    }

    #[test]
    fn inspection_uses_the_retained_dolt_sha_vectors_through_sc_sha() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../docs/plans/phase-an/fixtures/dolt-template-sha-vectors.json"
        ))
        .expect("retained SHA-vector fixture is valid JSON");
        let vectors = fixture["vectors"]
            .as_array()
            .expect("fixture contains vectors");
        let composer = ScComposeTemplateComposer::new();

        for vector in vectors {
            let name = vector["name"].as_str().expect("vector name");
            let raw = base64::engine::general_purpose::STANDARD
                .decode(
                    vector["raw_file_bytes_base64"]
                        .as_str()
                        .expect("base64 input"),
                )
                .expect("fixture base64 is valid");
            let expected_sha = vector["sha256"].as_str().expect("fixture SHA");
            let inspection = composer
                .inspect(&TemplateSource::file_backed(
                    raw,
                    format!("{name}.txt.j2").into(),
                ))
                .expect("public upstream inspection accepts retained vector");

            assert_eq!(inspection.sha.as_str(), expected_sha, "vector {name}");
        }
    }

    #[test]
    fn inspection_projects_public_frontmatter_and_directive_spans() {
        let raw = concat!(
            "---\n",
            "required_variables:\n",
            "  - assignee\n",
            "defaults:\n",
            "  priority: high\n",
            "metadata:\n",
            "  workflow: dev-start\n",
            "  tags:\n",
            "    - phase:an\n",
            "    - team:atm-dev\n",
            "---\n",
            "{% include \"child.j2\" %}\n",
            "{% import \"macros.j2\" as macros %}\n",
            "{% from \"helpers.j2\" import helper %}\n",
            "{# {% include \"commented-out.j2\" %} #}\n",
        );
        let inspection = ScComposeTemplateComposer::new()
            .inspect(&TemplateSource::file_backed(
                raw.as_bytes().to_vec(),
                "workflow.txt.j2".into(),
            ))
            .expect("public upstream inspection succeeds");

        assert_eq!(
            inspection
                .frontmatter
                .required_variables
                .iter()
                .map(atm_storage::TemplateVariableName::as_str)
                .collect::<Vec<_>>(),
            vec!["assignee"]
        );
        assert_eq!(inspection.frontmatter.defaults["priority"], json!("high"));
        assert_eq!(
            inspection.frontmatter.metadata["workflow"],
            json!("dev-start")
        );
        assert_eq!(
            inspection.frontmatter.metadata["tags"],
            json!(["phase:an", "team:atm-dev"])
        );
        assert_eq!(
            inspection
                .include_references
                .iter()
                .map(|reference| reference.directive)
                .collect::<Vec<_>>(),
            vec![
                TemplateReferenceKind::Include,
                TemplateReferenceKind::Import,
                TemplateReferenceKind::FromImport,
            ]
        );
        assert_eq!(
            inspection
                .include_references
                .iter()
                .map(|reference| &raw
                    [reference.source_span.byte_start..reference.source_span.byte_end])
                .collect::<Vec<_>>(),
            vec![
                "{% include \"child.j2\" %}",
                "{% import \"macros.j2\" as macros %}",
                "{% from \"helpers.j2\" import helper %}",
            ],
            "the adapter retains parser-classified source spans and does not scan comments"
        );
    }

    #[test]
    fn inspection_fails_closed_for_non_utf8_source_bytes() {
        let error = ScComposeTemplateComposer::new()
            .inspect(&TemplateSource::file_backed(
                vec![0xff, b'{', b'%'],
                "invalid.txt.j2".into(),
            ))
            .expect_err("sc-sha input must remain strict UTF-8");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateContentNotUtf8
        );
    }

    #[test]
    fn inspection_fails_closed_for_invalid_frontmatter() {
        let error = ScComposeTemplateComposer::new()
            .inspect(&TemplateSource::file_backed(
                b"---\nrequired_variables: [\n---\nconfidential body".to_vec(),
                "invalid-frontmatter.txt.j2".into(),
            ))
            .expect_err("upstream frontmatter parse failure must reject admission");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateInspectionParseFailed
        );
        assert!(
            error
                .cause()
                .is_some_and(|cause| !cause.contains("confidential body")),
            "inspection failures must not expose template bodies"
        );
    }

    #[test]
    fn inspection_fails_closed_for_malformed_directive_syntax() {
        let error = ScComposeTemplateComposer::new()
            .inspect(&TemplateSource::file_backed(
                b"{% include \"child.j2\"".to_vec(),
                "invalid-directive.txt.j2".into(),
            ))
            .expect_err("parser-backed directive inspection must reject malformed Jinja");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateInspectionParseFailed
        );
    }

    #[test]
    fn inspection_fails_closed_for_plain_jinja_body_syntax() {
        let error = ScComposeTemplateComposer::new()
            .inspect(&TemplateSource::file_backed(
                b"hello {{ value".to_vec(),
                "invalid-body.txt.j2".into(),
            ))
            .expect_err("parser-backed body syntax inspection must reject malformed Jinja");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateInspectionParseFailed
        );
        assert!(error.message().contains("inspection parser"));
    }

    #[test]
    fn inspection_parse_and_hash_identity_failures_keep_distinct_codes_and_messages() {
        let parse_error = ScComposeTemplateComposer::new()
            .inspect(&TemplateSource::file_backed(
                b"hello {{ value".to_vec(),
                "invalid-body.txt.j2".into(),
            ))
            .expect_err("malformed Jinja must fail inspection");
        let hash_error = atm_storage::TemplateSha::new("not-a-sha")
            .map_err(ScComposeTemplateComposer::hash_api_error)
            .expect_err("invalid storage identity must fail the hash adapter seam");

        assert_eq!(
            parse_error.code(),
            atm_storage::AtmErrorCode::TemplateInspectionParseFailed
        );
        assert_eq!(
            hash_error.code(),
            atm_storage::AtmErrorCode::TemplateHashApiFailed
        );
        assert_ne!(parse_error.code(), hash_error.code());
        assert_ne!(parse_error.message(), hash_error.message());
    }

    #[test]
    fn inspection_preserves_atm_workflow_identifier_validation() {
        let error = ScComposeTemplateComposer::new()
            .inspect(&TemplateSource::file_backed(
                b"---\nrequired_variables:\n  - task.owner\n---\nbody".to_vec(),
                "invalid-workflow-variable.txt.j2".into(),
            ))
            .expect_err("ATM workflow identifiers remain more constrained than upstream variables");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateWorkflowInvalid
        );
    }

    #[test]
    fn inspection_requires_the_file_admission_path() {
        let error = ScComposeTemplateComposer::new()
            .inspect(&TemplateSource::stored(
                b"body".to_vec(),
                Some(TemplateOutputFormat::Text),
            ))
            .expect_err("stored templates retain their approved classification");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateClassificationInvalid
        );
    }

    #[test]
    fn inspection_uses_only_the_upstream_path_classifier_at_file_admission() {
        let raw = b"fixture".to_vec();
        let composer = ScComposeTemplateComposer::new();
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

        let composer = ScComposeTemplateComposer::new();
        let result = composer.render_within_root(
            &source,
            &vars,
            &TemplateRoot {
                canonical_path: canonical_root,
            },
        );
        fs::remove_dir_all(&root).expect("remove isolated template root");

        assert_eq!(result.expect("in-root render").text, "hello Rand");
        assert_eq!(composer.root_render_calls(), 1);
    }

    #[test]
    fn file_backed_rendering_rejects_a_missing_captured_file() {
        let root = temporary_root("missing-captured-file");
        let source = TemplateSource::file_backed(b"body".to_vec(), root.join("missing.txt.j2"));
        let error = ScComposeTemplateComposer::new()
            .render_within_root(
                &source,
                &Map::new(),
                &TemplateRoot {
                    canonical_path: fs::canonicalize(&root).expect("canonical root"),
                },
            )
            .expect_err("rendering must not fall back when the captured file disappears");
        fs::remove_dir_all(&root).expect("remove isolated template root");

        assert_eq!(error.code(), atm_storage::AtmErrorCode::TemplateLoadFailed);
    }

    #[test]
    fn file_backed_operations_require_a_canonical_source_path() {
        let root = TemplateRoot {
            canonical_path: temporary_root("missing-source-path"),
        };
        let source = TemplateSource::stored(b"body".to_vec(), Some(TemplateOutputFormat::Text));
        let composer = ScComposeTemplateComposer::new();

        let render_error = composer
            .render_within_root(&source, &Map::new(), &root)
            .expect_err("stored source cannot load dependencies");
        let compose_error = composer
            .compose_file(&source, &Map::new(), &root)
            .expect_err("stored source cannot compose from the filesystem");
        fs::remove_dir_all(&root.canonical_path).expect("remove isolated template root");

        assert_eq!(
            render_error.code(),
            atm_storage::AtmErrorCode::TemplateClassificationInvalid
        );
        assert_eq!(
            compose_error.code(),
            atm_storage::AtmErrorCode::TemplateClassificationInvalid
        );
    }

    #[test]
    fn stored_render_requires_an_admission_output_format() {
        let error = ScComposeTemplateComposer::new()
            .render_without_includes(&TemplateSource::stored(b"body".to_vec(), None), &Map::new())
            .expect_err("legacy rows cannot claim checked-render compatibility");

        assert_eq!(error.code(), atm_storage::AtmErrorCode::MailboxReadFailed);
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
    fn production_adapter_compose_maps_include_escape_to_typed_error() {
        let parent = temporary_root("compose-escape-parent");
        let root = parent.join("root");
        fs::create_dir_all(&root).expect("create root");
        let template_path = root.join("main.j2");
        fs::write(&template_path, "@<../outside.j2>\n").expect("write main template");
        fs::write(parent.join("outside.j2"), "must not load").expect("write escaped template");
        let source = TemplateSource::file_backed(
            fs::read(&template_path).expect("read template"),
            fs::canonicalize(&template_path).expect("canonical template"),
        );
        let error = ScComposeTemplateComposer::new()
            .compose_file(
                &source,
                &Map::new(),
                &TemplateRoot {
                    canonical_path: fs::canonicalize(&root).expect("canonical root"),
                },
            )
            .expect_err("native composition must preserve root confinement");
        fs::remove_dir_all(&parent).expect("remove isolated template parent");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateIncludeUnresolved
        );
    }

    #[test]
    fn production_adapter_compose_maps_missing_include_to_typed_error() {
        let root = temporary_root("compose-missing-include");
        let template_path = root.join("main.j2");
        fs::write(&template_path, "@<missing.j2>\n").expect("write main template");
        let source = TemplateSource::file_backed(
            fs::read(&template_path).expect("read template"),
            fs::canonicalize(&template_path).expect("canonical template"),
        );
        let error = ScComposeTemplateComposer::new()
            .compose_file(
                &source,
                &Map::new(),
                &TemplateRoot {
                    canonical_path: fs::canonicalize(&root).expect("canonical root"),
                },
            )
            .expect_err("missing include must not be converted into a generic render failure");
        fs::remove_dir_all(&root).expect("remove isolated template root");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateIncludeUnresolved
        );
    }

    #[test]
    fn an15_boundary_probe_rejects_one_hundred_confined_include_escape_vectors() {
        let parent = temporary_root("an15-include-escape-parent");
        let root = parent.join("root");
        fs::create_dir_all(&root).expect("create root");
        let canonical_root = ConfiningRoot::new(&root)
            .expect("canonical root")
            .into_inner();

        for case_index in 0..100 {
            let template_path = root.join(format!("main-{case_index}.j2"));
            fs::write(&template_path, format!("@<../outside-{case_index}.j2>\\n"))
                .expect("write escaping template");
            fs::write(
                parent.join(format!("outside-{case_index}.j2")),
                "must not load",
            )
            .expect("write outside template");
            let source = TemplateSource::file_backed(
                fs::read(&template_path).expect("read template"),
                template_path,
            );
            let error = ScComposeTemplateComposer::new()
                .render_within_root(
                    &source,
                    &Map::new(),
                    &TemplateRoot {
                        canonical_path: canonical_root.clone(),
                    },
                )
                .expect_err("outside include must be rejected");
            assert_eq!(
                error.code(),
                atm_storage::AtmErrorCode::TemplateIncludeUnresolved
            );
        }
        fs::remove_dir_all(&parent).expect("remove isolated template parent");
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

    #[test]
    fn production_adapter_compose_maps_config_parse_to_inspection_error() {
        let root = temporary_root("compose-invalid-frontmatter");
        let template_path = root.join("main.j2");
        let raw = b"---\nrequired_variables: [\n---\nbody";
        fs::write(&template_path, raw).expect("write invalid frontmatter template");
        let canonical_root = fs::canonicalize(&root).expect("canonical root");
        let canonical_template = fs::canonicalize(&template_path).expect("canonical template");
        let source = TemplateSource::file_backed(raw.to_vec(), canonical_template);

        let error = ScComposeTemplateComposer::new()
            .compose_file(
                &source,
                &Map::new(),
                &TemplateRoot {
                    canonical_path: canonical_root,
                },
            )
            .expect_err("config parse failures must fail composition");
        fs::remove_dir_all(&root).expect("remove isolated template root");

        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TemplateInspectionParseFailed
        );
        assert!(error.message().contains("inspection parser"));
        assert!(
            error
                .cause()
                .is_some_and(|cause| cause.contains("failed to parse YAML frontmatter")),
            "the typed ATM error must retain the upstream config diagnostic"
        );
    }
}
