//! Local template composition without mailbox or daemon interaction.

use std::path::{Path, PathBuf};

use anyhow::Result;
use atm_core::boundary::{TemplateRoot, TemplateSource};
use atm_core::send::input;
use clap::Args;
use serde_json::{Map, Value};

use crate::commands::send::{capture_environment_values, parse_assignment_values};
use crate::composition::resolve_command_runtime_context;

/// Render a template through the core renderer port and print the exact body.
#[derive(Debug, Args)]
#[command(
    after_help = "Composition is local and never reads or writes the mailbox. Use `atm send <agent> --template <path> --vars <file>` to deliver the same template after previewing it."
)]
pub struct ComposeCommand {
    /// Template file to validate and render.
    #[arg(long, value_name = "PATH", required = true)]
    template: PathBuf,

    /// JSON object providing template variables; `-` reads stdin.
    #[arg(long, value_name = "FILE|-")]
    vars: Option<String>,

    /// One template variable; may be repeated.
    #[arg(long = "var", value_name = "KEY=VALUE")]
    var: Vec<String>,

    /// Capture environment variables with this prefix.
    #[arg(long = "env-prefix", value_name = "PREFIX")]
    env_prefix: Option<String>,

    /// Validate and render without any side effects (the default operation is
    /// already side-effect free; the flag makes scripts self-documenting).
    #[arg(long)]
    dry_run: bool,

    /// Emit a structured result instead of the byte-identical rendered body.
    #[arg(long)]
    json: bool,
}

impl ComposeCommand {
    /// Execute local composition through the bootstrap-owned renderer port.
    pub fn run(self) -> Result<()> {
        let (_home_dir, current_dir) = resolve_command_runtime_context("compose")?;
        let source = load_template_source(
            &self.template,
            &current_dir,
            self.vars.as_deref(),
            &self.var,
            self.env_prefix.as_deref(),
        )?;
        let root = TemplateRoot {
            canonical_path: source
                .source
                .canonical_file_path
                .as_deref()
                .and_then(Path::parent)
                .ok_or_else(|| anyhow::anyhow!("template path has no parent directory"))?
                .to_path_buf(),
        };
        let composer = atm_daemon_bootstrap::template_composer();
        let rendered = composer
            .compose_file(&source.source, &source.vars, &root)
            .map_err(anyhow::Error::from)?;
        let max_bytes = atm_core::load_atm_config(&current_dir)?
            .map(|config| {
                config.max_message_bytes.as_usize().ok_or_else(|| {
                    anyhow::anyhow!("configured max_message_bytes does not fit this platform")
                })
            })
            .transpose()?
            .unwrap_or(input::default_message_max_bytes());
        let rendered = input::validate_message_text_with_limit(rendered.text, max_bytes)?;

        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "template": source.display_path,
                    "dry_run": self.dry_run,
                    "body": rendered,
                }))?
            );
        } else {
            // sc-compose's stdout contract terminates a rendered document
            // with one newline even when the source template has none.  Do
            // the same without adding a second newline to templates that
            // already carry their final line ending.
            print!("{rendered}");
            if !rendered.ends_with('\n') {
                println!();
            }
        }
        Ok(())
    }
}

struct LoadedTemplate {
    source: TemplateSource,
    vars: Map<String, Value>,
    display_path: String,
}

fn load_template_source(
    template: &Path,
    current_dir: &Path,
    vars_path: Option<&str>,
    explicit: &[String],
    env_prefix: Option<&str>,
) -> Result<LoadedTemplate> {
    let template_path = if template.is_absolute() {
        template.to_path_buf()
    } else {
        current_dir.join(template)
    };
    let canonical_path = std::fs::canonicalize(&template_path)
        .map_err(|error| anyhow::anyhow!("template could not be resolved: {error}"))?;
    let raw_file_bytes = std::fs::read(&canonical_path)
        .map_err(|error| anyhow::anyhow!("template could not be read: {error}"))?;
    let mut vars = capture_environment_values(env_prefix)?;
    vars.extend(read_vars_file(vars_path, current_dir)?);
    vars.extend(parse_assignment_values(explicit)?);
    Ok(LoadedTemplate {
        source: TemplateSource::file_backed(raw_file_bytes, canonical_path.clone()),
        vars,
        display_path: canonical_path.display().to_string(),
    })
}

fn read_vars_file(source: Option<&str>, current_dir: &Path) -> Result<Map<String, Value>> {
    let Some(source) = source else {
        return Ok(Map::new());
    };
    let contents = if source == "-" {
        use std::io::Read as _;
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input)?;
        input
    } else {
        let path = Path::new(source);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            current_dir.join(path)
        };
        std::fs::read_to_string(path)?
    };
    let value: Value = serde_json::from_str(&contents)?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("--vars must contain a JSON object"))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[test]
    fn compose_help_surface_accepts_documented_flags() {
        crate::commands::Cli::try_parse_from([
            "atm",
            "compose",
            "--template",
            "notice.j2",
            "--vars",
            "vars.json",
            "--var",
            "name=Rand",
            "--env-prefix",
            "ATM_TEMPLATE_",
            "--dry-run",
        ])
        .expect("documented compose command must parse");
    }
}
