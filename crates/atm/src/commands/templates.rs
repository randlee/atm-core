//! Immutable template catalog introspection commands.

use anyhow::Result;
use atm_core::{TemplateSha, with_default_local_service_runtime};
use atm_storage::{TemplateListFilter, TemplateSummary};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::composition::install_local_runtime_for_read_only_command;

/// Inspect immutable templates registered by decomposed-message admission.
#[derive(Debug, Args)]
pub struct TemplatesCommand {
    #[command(subcommand)]
    command: TemplatesSubcommand,
}

#[derive(Debug, Subcommand)]
enum TemplatesSubcommand {
    /// List every known immutable template revision, optionally by metadata type.
    List {
        #[arg(long = "type")]
        template_type: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show the stored schema/frontmatter for one exact immutable SHA.
    Schema {
        sha: TemplateSha,
        #[arg(long)]
        json: bool,
    },
}

impl TemplatesCommand {
    pub async fn run(self, _observability: &crate::observability::CliObservability) -> Result<()> {
        install_local_runtime_for_read_only_command()?;
        match self.command {
            TemplatesSubcommand::List {
                template_type,
                json,
            } => list(template_type, json),
            TemplatesSubcommand::Schema { sha, json } => schema(sha, json),
        }
    }
}

#[derive(Debug, Serialize)]
struct TemplateSummaryOutput<'a> {
    template_sha: &'a str,
    template_type: Option<&'a str>,
    template_name: Option<&'a str>,
    first_seen_at: String,
    first_seen_by: &'a str,
}

fn list(template_type: Option<String>, json: bool) -> Result<()> {
    let templates = with_default_local_service_runtime(|runtime| {
        runtime
            .template_catalog_store()
            .ok_or_else(|| {
                atm_core::error::AtmError::daemon_unavailable(
                    "template catalog is not installed in the local runtime",
                )
            })?
            .list(TemplateListFilter { template_type })
    })?;
    if json {
        let output = templates.iter().map(summary_output).collect::<Vec<_>>();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for template in &templates {
            println!(
                "{} {} {} {} {}",
                template.sha,
                template.template_type.as_deref().unwrap_or("-"),
                template.template_name.as_deref().unwrap_or("-"),
                template.first_seen.at,
                template.first_seen.by,
            );
        }
    }
    Ok(())
}

fn schema(sha: TemplateSha, json: bool) -> Result<()> {
    let template = with_default_local_service_runtime(|runtime| {
        runtime
            .template_catalog_store()
            .ok_or_else(|| {
                atm_core::error::AtmError::daemon_unavailable(
                    "template catalog is not installed in the local runtime",
                )
            })?
            .load(&sha)
    })?
    .ok_or_else(|| anyhow::anyhow!("no immutable template is registered for SHA {sha}"))?;
    let value = serde_json::json!({
        "template_sha": template.sha,
        "template_type": template.template_type,
        "template_name": template.template_name,
        "first_seen_at": template.first_seen.at.to_string(),
        "first_seen_by": template.first_seen.by,
        "schema_json": template.frontmatter,
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("template_sha: {}", template.sha);
        println!(
            "template_type: {}",
            template.template_type.as_deref().unwrap_or("-")
        );
        println!(
            "template_name: {}",
            template.template_name.as_deref().unwrap_or("-")
        );
        println!("first_seen_at: {}", template.first_seen.at);
        println!("first_seen_by: {}", template.first_seen.by);
        println!("schema_json:");
        println!("{}", serde_json::to_string_pretty(&template.frontmatter)?);
    }
    Ok(())
}

fn summary_output(template: &TemplateSummary) -> TemplateSummaryOutput<'_> {
    TemplateSummaryOutput {
        template_sha: template.sha.as_str(),
        template_type: template.template_type.as_deref(),
        template_name: template.template_name.as_deref(),
        first_seen_at: template.first_seen.at.to_string(),
        first_seen_by: &template.first_seen.by,
    }
}

#[cfg(test)]
mod tests {
    use atm_core::test_support::TEST_ARCH_CTM;
    use atm_storage::{TemplateFirstSeen, TemplateSummary};
    use clap::Parser;

    use super::summary_output;

    #[test]
    fn documented_template_introspection_surface_parses() {
        crate::commands::Cli::try_parse_from([
            "atm",
            "templates",
            "list",
            "--type",
            "dev",
            "--json",
        ])
        .expect("list");
        crate::commands::Cli::try_parse_from([
            "atm",
            "templates",
            "schema",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--json",
        ])
        .expect("schema");
    }

    #[test]
    fn list_output_carries_every_field_needed_to_construct_a_query() {
        let summary = TemplateSummary {
            sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .parse()
                .expect("SHA"),
            template_type: Some("qa-report".to_owned()),
            template_name: Some("qa".to_owned()),
            first_seen: TemplateFirstSeen::new(
                "2026-08-12T00:00:00Z".parse().expect("timestamp"),
                TEST_ARCH_CTM,
            )
            .expect("first seen"),
        };
        let value = serde_json::to_value(summary_output(&summary)).expect("JSON");
        assert_eq!(value["template_sha"], summary.sha.as_str());
        assert_eq!(value["template_type"], "qa-report");
        assert_eq!(value["template_name"], "qa");
        assert_eq!(value["first_seen_by"], TEST_ARCH_CTM);
    }
}
