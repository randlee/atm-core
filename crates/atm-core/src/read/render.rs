//! Core-owned render-on-read policy for decomposed mailbox records.
//!
//! The storage layer supplies immutable bytes and merged variables while the
//! composition root supplies the renderer port. Keeping this operation here
//! prevents read surfaces from depending on SQLite or the sc-compose adapter.

use atm_storage::{MergedVarsJson, StoredTemplate, TemplateCatalogStore};

use crate::boundary::{RenderedBody, TemplateComposer, TemplateSource};
use crate::error::AtmError;
use crate::schema::{AtmMessageId, InboxMessage};

/// Render one stored template without making filesystem or resolver calls.
///
/// This is intentionally a pure function of the immutable catalog row and the
/// already-merged variables captured at admission time.
pub fn render_decomposed(
    composer: &dyn TemplateComposer,
    template: &StoredTemplate,
    vars: &MergedVarsJson,
) -> Result<RenderedBody, AtmError> {
    let source = TemplateSource {
        raw_file_bytes: template.content_bytes.clone(),
        canonical_file_path: None,
    };
    composer.render_without_includes(&source, vars.as_map())
}

/// Render a durable envelope when its message key has decomposition columns.
///
/// The returned envelope is suitable for every presentation surface. For a
/// decomposed row its summary is refreshed from the rendered prefix so list
/// and peek snippets never rely on the nullable storage body column.
pub(crate) fn render_message_body(
    catalog: &dyn TemplateCatalogStore,
    composer: Option<&dyn TemplateComposer>,
    key: &crate::boundary::MessageKey,
    message_id: Option<AtmMessageId>,
    envelope: &InboxMessage,
) -> Result<InboxMessage, AtmError> {
    let Some(decomposed) = catalog.load_decomposed_message(key)? else {
        return Ok(envelope.clone());
    };
    let message_label = message_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "<none>".to_string());
    let template_sha = decomposed.template_sha.clone();
    let composer = composer.ok_or_else(|| {
        AtmError::mailbox_read(format!(
            "decomposed message_id {message_label} template_sha {template_sha} cannot be rendered because no TemplateComposer is installed"
        ))
    })?;
    let template = catalog.load(&template_sha)?.ok_or_else(|| {
        AtmError::mailbox_read(format!(
            "decomposed message_id {message_label} template_sha {template_sha} is missing; re-register the same template SHA"
        ))
    })?;
    let rendered = render_decomposed(composer, &template, &decomposed.vars).map_err(|error| {
        // Preserve the renderer's stable typed code (notably
        // DECOMPOSED_TEMPLATE_INCLUDE_FORBIDDEN) while adding the durable
        // identifiers needed to diagnose a corrupt row.
        AtmError::new(
            error.code(),
            format!(
                "decomposed message_id {message_label} template_sha {template_sha}: {}",
                error.detail()
            ),
        )
        .with_cause(error.message())
    })?;
    let mut envelope = envelope.clone();
    envelope.text = rendered.text;
    envelope.summary = Some(rendered_prefix(&envelope.text));
    Ok(envelope)
}

fn rendered_prefix(text: &str) -> String {
    text.chars().take(160).collect()
}

#[cfg(test)]
mod tests {
    use atm_storage::{TemplateFrontmatter, TemplateSha};
    use serde_json::{Map, Value};

    use super::{render_decomposed, rendered_prefix};
    use crate::boundary::sealed;
    use crate::boundary::{
        RenderedBody, TemplateComposer, TemplateInspection, TemplateRoot, TemplateSource,
    };

    struct FixtureComposer {
        includes: bool,
    }

    impl sealed::Sealed for FixtureComposer {}

    impl TemplateComposer for FixtureComposer {
        fn inspect(
            &self,
            _raw_file_bytes: &[u8],
        ) -> Result<TemplateInspection, crate::error::AtmError> {
            Ok(TemplateInspection {
                sha: TemplateSha::new("a".repeat(64)).expect("fixture SHA"),
                frontmatter: TemplateFrontmatter::default(),
                include_references: Vec::new(),
            })
        }

        fn render_within_root(
            &self,
            _template: &TemplateSource,
            _vars: &Map<String, Value>,
            _root: &TemplateRoot,
        ) -> Result<RenderedBody, crate::error::AtmError> {
            unreachable!("render-on-read never uses a loader-backed operation")
        }

        fn render_without_includes(
            &self,
            _source: &TemplateSource,
            _vars: &Map<String, Value>,
        ) -> Result<RenderedBody, crate::error::AtmError> {
            if self.includes {
                return Err(crate::error::AtmError::decomposed_template_include_forbidden());
            }
            Ok(RenderedBody {
                text: "stable rendered body".to_string(),
            })
        }
    }

    fn template() -> atm_storage::StoredTemplate {
        atm_storage::StoredTemplate {
            sha: TemplateSha::new("a".repeat(64)).expect("fixture SHA"),
            template_type: None,
            template_name: None,
            content_bytes: b"stable source".to_vec(),
            content_text: "stable source".to_string(),
            frontmatter: TemplateFrontmatter::default(),
            first_seen: atm_storage::TemplateFirstSeen::new(
                crate::types::IsoTimestamp::now(),
                "test",
            )
            .expect("first seen"),
        }
    }

    #[test]
    fn rendered_prefix_is_character_bounded() {
        assert_eq!(rendered_prefix(&"é".repeat(161)).chars().count(), 160);
    }

    #[test]
    fn decomposed_render_is_stable_and_uses_the_no_include_port() {
        let vars = atm_storage::MergedVarsJson::try_from_merged_object(Map::from_iter([(
            "name".to_string(),
            Value::String("cipher".to_string()),
        )]))
        .expect("merged vars");
        let composer = FixtureComposer { includes: false };
        let template = template();
        let first = render_decomposed(&composer, &template, &vars).expect("render");
        let second = render_decomposed(&composer, &template, &vars).expect("repeat render");
        assert_eq!(first, second);
        assert_eq!(first.text, "stable rendered body");
    }

    #[test]
    fn include_rejection_keeps_the_typed_error_contract() {
        let composer = FixtureComposer { includes: true };
        let error = render_decomposed(
            &composer,
            &template(),
            &atm_storage::MergedVarsJson::default(),
        )
        .expect_err("include-bearing stored source");
        assert_eq!(
            error.code(),
            crate::error_codes::AtmErrorCode::DecomposedTemplateIncludeForbidden
        );
    }
}
