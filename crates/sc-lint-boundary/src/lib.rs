use std::fmt;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use cargo_metadata::MetadataCommand;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text => f.write_str("text"),
            Self::Json => f.write_str("json"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzeOptions {
    pub root: PathBuf,
    pub format: OutputFormat,
    pub rule: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportGraphOptions {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FindingsReport {
    pub tool: &'static str,
    pub version: &'static str,
    pub status: &'static str,
    pub scanned_crates: usize,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Finding {
    pub rule_id: String,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GraphExport {
    pub tool: &'static str,
    pub version: &'static str,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GraphNode {
    pub id: String,
    pub kind: &'static str,
    pub label: String,
    pub package: String,
    pub manifest_path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GraphEdge {
    pub kind: &'static str,
    pub from: String,
    pub to: String,
}

pub fn analyze_workspace(options: &AnalyzeOptions) -> Result<FindingsReport> {
    let metadata = load_metadata(&options.root)?;
    let scanned_crates = metadata.workspace_packages().len();

    Ok(FindingsReport {
        tool: "sc-lint-boundary",
        version: env!("CARGO_PKG_VERSION"),
        status: "pass",
        scanned_crates,
        findings: Vec::new(),
    })
}

pub fn export_workspace_graph(options: &ExportGraphOptions) -> Result<GraphExport> {
    let metadata = load_metadata(&options.root)?;
    let workspace_members = metadata.workspace_members.clone();

    let mut nodes = Vec::new();
    for package in metadata.packages {
        if workspace_members.iter().any(|id| id == &package.id) {
            nodes.push(GraphNode {
                id: package.name.to_string(),
                kind: "crate",
                label: package.name.to_string(),
                package: package.name.to_string(),
                manifest_path: package.manifest_path.as_std_path().display().to_string(),
            });
        }
    }

    nodes.sort_by(|left, right| left.id.cmp(&right.id));

    Ok(GraphExport {
        tool: "sc-lint-boundary",
        version: env!("CARGO_PKG_VERSION"),
        nodes,
        edges: Vec::new(),
    })
}

fn load_metadata(root: &Path) -> Result<cargo_metadata::Metadata> {
    MetadataCommand::new()
        .current_dir(root)
        .exec()
        .with_context(|| format!("failed to load cargo metadata for {}", root.display()))
}

pub fn render_findings_report(report: &FindingsReport) -> String {
    format!(
        "{} {} status={} scanned_crates={} findings={}",
        report.tool,
        report.version,
        report.status,
        report.scanned_crates,
        report.findings.len()
    )
}

#[cfg(test)]
mod tests {
    use super::FindingsReport;
    use super::GraphExport;
    use super::render_findings_report;

    #[test]
    fn findings_report_text_is_stable() {
        let report = FindingsReport {
            tool: "sc-lint-boundary",
            version: "0.1.0",
            status: "pass",
            scanned_crates: 2,
            findings: Vec::new(),
        };
        assert_eq!(
            render_findings_report(&report),
            "sc-lint-boundary 0.1.0 status=pass scanned_crates=2 findings=0"
        );
    }

    #[test]
    fn graph_export_serializes_tool_metadata() {
        let graph = GraphExport {
            tool: "sc-lint-boundary",
            version: "0.1.0",
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        let json = serde_json::to_string(&graph).unwrap();
        assert!(json.contains("\"tool\":\"sc-lint-boundary\""));
        assert!(json.contains("\"version\":\"0.1.0\""));
    }
}
