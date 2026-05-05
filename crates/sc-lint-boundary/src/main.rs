use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use sc_lint_boundary::AnalyzeOptions;
use sc_lint_boundary::ExportGraphOptions;
use sc_lint_boundary::GraphOutputFormat;
use sc_lint_boundary::OutputFormat;
use sc_lint_boundary::analyze_workspace;
use sc_lint_boundary::export_workspace_graph;
use sc_lint_boundary::render_findings_report;
use sc_lint_boundary::render_graph_export;

#[derive(Debug, Parser)]
#[command(name = "sc-lint-boundary")]
#[command(about = "AST-sensitive Rust boundary analyzer")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Analyze {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value = "text")]
        format: FormatArg,
        #[arg(long)]
        rule: Option<String>,
    },
    ExportGraph {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value = "json")]
        format: GraphFormatArg,
    },
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum FormatArg {
    Text,
    Json,
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum GraphFormatArg {
    Json,
    Turtle,
}

impl From<FormatArg> for OutputFormat {
    fn from(value: FormatArg) -> Self {
        match value {
            FormatArg::Text => OutputFormat::Text,
            FormatArg::Json => OutputFormat::Json,
        }
    }
}

impl From<GraphFormatArg> for GraphOutputFormat {
    fn from(value: GraphFormatArg) -> Self {
        match value {
            GraphFormatArg::Json => GraphOutputFormat::Json,
            GraphFormatArg::Turtle => GraphOutputFormat::Turtle,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Analyze { root, format, rule } => {
            let options = AnalyzeOptions {
                root,
                format: format.clone().into(),
                rule,
            };
            let report = analyze_workspace(&options)?;
            match OutputFormat::from(format) {
                OutputFormat::Text => {
                    println!("{}", render_findings_report(&report));
                }
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                }
            }
        }
        Command::ExportGraph { root, format } => {
            let graph = export_workspace_graph(&ExportGraphOptions { root })?;
            println!("{}", render_graph_export(&graph, format.into()));
        }
    }

    Ok(())
}
