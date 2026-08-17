//! Template-composition port and transport-neutral value objects.
//!
//! The approved `atm-template-sc-compose` adapter is the only production
//! implementation. Core code depends on this port, never on the upstream
//! renderer, hashing, YAML, or filesystem libraries behind it.

use std::path::PathBuf;

use atm_storage::{TemplateFrontmatter, TemplateOutputFormat, TemplateSha};
use serde_json::{Map, Value};

use crate::boundary::sealed;
use crate::error::AtmError;

/// Core-owned port for inspected and rendered template source.
///
/// The implementation set is controlled by the ATM workspace-convention seal
/// in ADR-001. The production adapter derives a platform-independent identity
/// by strictly decoding source bytes and normalizing line endings before
/// hashing; it retains the original bytes for inspection/rendering. It also
/// performs frontmatter, dependency inspection, and loader confinement through
/// the exact-pinned upstream renderer.
pub trait TemplateComposer: sealed::Sealed + Send + Sync {
    /// Inspect a complete template source.
    ///
    /// The adapter preserves these original bytes for source handling but owns
    /// the strict UTF-8 and LF-normalized identity calculation.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the source cannot be decoded or inspected by
    /// the approved template implementation.
    fn inspect(&self, source: &TemplateSource) -> Result<TemplateInspection, AtmError>;

    /// Render a file-backed template while confining every dependency to `root`.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the template cannot be rendered or a
    /// dependency is missing, escapes `root`, or violates the adapter's
    /// renderer contract.
    fn render_within_root(
        &self,
        template: &TemplateSource,
        vars: &Map<String, Value>,
        root: &TemplateRoot,
    ) -> Result<RenderedBody, AtmError>;

    /// Compose a caller-owned file through the adapter's native composition
    /// pipeline.  The default keeps older adapters source-compatible by using
    /// the already-inspected root render seam; the production adapter
    /// overrides this to preserve sc-compose's validation and diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when validation, include confinement, or rendering
    /// fails.
    fn compose_file(
        &self,
        template: &TemplateSource,
        vars: &Map<String, Value>,
        root: &TemplateRoot,
    ) -> Result<RenderedBody, AtmError> {
        self.render_within_root(template, vars, root)
    }

    /// Render a stored source only after parser-backed dependency rejection.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] before loader or filesystem access when the source
    /// contains an include, import, or from-import statement. Returns an error
    /// when a dependency-free source cannot be rendered.
    fn render_without_includes(
        &self,
        source: &TemplateSource,
        vars: &Map<String, Value>,
    ) -> Result<RenderedBody, AtmError>;
}

/// Complete raw source of a template file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSource {
    /// The unchanged source-file bytes used for inspection and rendering.
    pub raw_file_bytes: Vec<u8>,
    /// Canonical path of this source while it is being rendered from a local
    /// file. Stored/decomposed templates deliberately omit this: they must use
    /// [`TemplateComposer::render_without_includes`] and may never trigger
    /// filesystem loading.
    pub canonical_file_path: Option<PathBuf>,
    /// Persisted catalog classification for a stored source. A source being
    /// admitted from a local file deliberately leaves this unset because the
    /// adapter must classify its canonical path exactly once.
    pub output_format: Option<TemplateOutputFormat>,
}

impl TemplateSource {
    /// Creates a source obtained from a canonical local file.
    #[must_use]
    pub fn file_backed(raw_file_bytes: Vec<u8>, canonical_file_path: PathBuf) -> Self {
        Self {
            raw_file_bytes,
            canonical_file_path: Some(canonical_file_path),
            output_format: None,
        }
    }

    /// Creates a source retained in storage without a filesystem identity.
    #[must_use]
    pub fn stored(raw_file_bytes: Vec<u8>, output_format: Option<TemplateOutputFormat>) -> Self {
        Self {
            raw_file_bytes,
            canonical_file_path: None,
            output_format,
        }
    }
}

/// Canonical filesystem root that constrains file-backed template dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateRoot {
    /// Canonical absolute path of the approved template root.
    pub canonical_path: PathBuf,
}

/// Parser-backed result for one raw template file.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateInspection {
    /// Platform-independent, LF-normalized SHA-256 identity from the approved
    /// renderer adapter.
    pub sha: TemplateSha,
    /// Storage-ready frontmatter extracted by the approved renderer adapter.
    pub frontmatter: TemplateFrontmatter,
    /// Every template-loading directive identified by the upstream parser.
    pub include_references: Vec<TemplateReference>,
    /// Output contract classified by the approved adapter for this file.
    pub output_format: TemplateOutputFormat,
}

/// One upstream template-loading statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateReference {
    /// The dependency-statement form classified by the upstream parser.
    pub directive: TemplateReferenceKind,
    /// Exact UTF-8 byte span of the statement within the source file.
    pub source_span: SourceSpan,
}

/// UTF-8 byte span in a raw template source string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    /// Inclusive start offset in UTF-8 bytes.
    pub byte_start: usize,
    /// Exclusive end offset in UTF-8 bytes.
    pub byte_end: usize,
}

/// Forms of template statements that need another template to be loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateReferenceKind {
    /// A template include statement.
    Include,
    /// A module import statement.
    Import,
    /// A from-import statement.
    FromImport,
}

/// Rendered UTF-8 body returned from a template-composition operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedBody {
    /// The complete rendered text.
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::{SourceSpan, TemplateReference, TemplateReferenceKind};

    #[test]
    fn template_reference_preserves_upstream_classification_and_span() {
        let reference = TemplateReference {
            directive: TemplateReferenceKind::FromImport,
            source_span: SourceSpan {
                byte_start: 4,
                byte_end: 42,
            },
        };

        assert_eq!(reference.directive, TemplateReferenceKind::FromImport);
        assert_eq!(reference.source_span.byte_start, 4);
        assert_eq!(reference.source_span.byte_end, 42);
    }
}
