use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::Context;
use anyhow::Result;
use cargo_metadata::MetadataCommand;
use quote::ToTokens;
use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;
use syn::Attribute;
use syn::File;
use syn::Ident;
use syn::ImplItem;
use syn::Item;
use syn::Receiver;
use syn::Type;
use syn::visit::Visit;

mod analysis;
// The attribute parser lives in the proc-macro crate source because proc-macro
// crates cannot expose normal library APIs. We include the shared parser file
// directly here so the analyzer and attribute macro validate the same directive
// syntax. If directives.rs changes, both crates must be kept in sync.
#[path = "../../sc-lint-attributes/src/directives.rs"]
mod directive_parser;
mod graph;
mod render;
#[cfg(test)]
mod tests;

const SC_LINT_SCHEMA_VERSION: &str = "0.1.0";
const DEFAULT_RULES_TOML: &str = include_str!("../config/defaults.toml");

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct RuleDefaults {
    trait_self_loop: TraitSelfLoopDefaults,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct TraitSelfLoopDefaults {
    ignored_trait_paths: Vec<String>,
    ignored_trait_names: Vec<String>,
}

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
    pub rule: Option<RuleFilter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportGraphOptions {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphOutputFormat {
    Json,
    Turtle,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FindingsReport {
    pub tool: &'static str,
    pub version: &'static str,
    pub schema_version: &'static str,
    pub status: &'static str,
    pub scanned_crates: usize,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Finding {
    pub rule_id: RuleId,
    pub kind: String,
    pub message: String,
    pub owner_ids: Vec<String>,
    pub node_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuleId {
    ScbCycle001,
    ScbCycle002,
    ScbCycle003,
    ScbBoundary001,
    ScbBoundary002,
    ScbBoundary003,
}

impl RuleId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ScbCycle001 => "SCB-CYCLE-001",
            Self::ScbCycle002 => "SCB-CYCLE-002",
            Self::ScbCycle003 => "SCB-CYCLE-003",
            Self::ScbBoundary001 => "SCB-BOUNDARY-001",
            Self::ScbBoundary002 => "SCB-BOUNDARY-002",
            Self::ScbBoundary003 => "SCB-BOUNDARY-003",
        }
    }
}

impl Serialize for RuleId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleFilter {
    Cycles,
    Boundaries,
    InternalOnly,
    ForbidExternalImpls,
}

impl RuleFilter {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cycles => "cycles",
            Self::Boundaries => "boundaries",
            Self::InternalOnly => "internal_only",
            Self::ForbidExternalImpls => "forbid_external_impls",
        }
    }
}

impl fmt::Display for RuleFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleFilterParseError {
    invalid_value: String,
}

impl RuleFilterParseError {
    fn new(invalid_value: impl Into<String>) -> Self {
        Self {
            invalid_value: invalid_value.into(),
        }
    }
}

impl fmt::Display for RuleFilterParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unsupported rule filter `{}`; supported: cycles, boundaries, internal_only, forbid_external_impls",
            self.invalid_value
        )
    }
}

impl std::error::Error for RuleFilterParseError {}

impl TryFrom<&str> for RuleFilter {
    type Error = RuleFilterParseError;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match value {
            "cycles" => Ok(Self::Cycles),
            "boundaries" => Ok(Self::Boundaries),
            "internal_only" => Ok(Self::InternalOnly),
            "forbid_external_impls" => Ok(Self::ForbidExternalImpls),
            other => Err(RuleFilterParseError::new(other)),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GraphExport {
    pub tool: &'static str,
    pub version: &'static str,
    pub schema_version: &'static str,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GraphNode {
    pub id: String,
    pub kind: &'static str,
    pub label: String,
    pub visibility: Option<&'static str>,
    pub package: String,
    pub target: Option<String>,
    pub manifest_path: String,
    pub source_path: Option<String>,
    pub module_path: Option<String>,
    pub impl_kind: Option<&'static str>,
    pub impl_trait: Option<String>,
    pub attributes: Vec<LintAttribute>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GraphEdge {
    pub kind: &'static str,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ReferenceKind {
    Type,
    Expr,
}

impl ReferenceKind {
    fn edge_kind(self) -> &'static str {
        match self {
            Self::Type => "references_type",
            Self::Expr => "references_expr",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CollectedReference {
    path: String,
    kind: ReferenceKind,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LintAttribute {
    pub scope: &'static str,
    pub name: &'static str,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetContext {
    package_name: String,
    target_name: String,
    manifest_path: String,
    crate_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemVisibility {
    Private,
    Public,
    Crate,
    Restricted,
}

impl ItemVisibility {
    fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Public => "public",
            Self::Crate => "crate",
            Self::Restricted => "restricted",
        }
    }
}

#[derive(Debug, Default)]
struct GraphBuilder {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

impl GraphBuilder {
    fn add_node(&mut self, node: GraphNode) {
        if !self.nodes.iter().any(|existing| existing.id == node.id) {
            self.nodes.push(node);
        }
    }

    fn add_edge(&mut self, kind: &'static str, from: impl Into<String>, to: impl Into<String>) {
        let edge = GraphEdge {
            kind,
            from: from.into(),
            to: to.into(),
        };
        if !self.edges.contains(&edge) {
            self.edges.push(edge);
        }
    }

    fn add_workspace_target(
        &mut self,
        package_name: &str,
        manifest_path: &str,
        target_name: &str,
        source_path: &Path,
    ) {
        let crate_id = graph::crate_id(package_name, target_name);
        self.add_node(GraphNode {
            id: crate_id,
            kind: "crate",
            label: target_name.to_string(),
            visibility: None,
            package: package_name.to_string(),
            target: Some(target_name.to_string()),
            manifest_path: manifest_path.to_string(),
            source_path: Some(source_path.display().to_string()),
            module_path: Some("crate".to_string()),
            impl_kind: None,
            impl_trait: None,
            attributes: Vec::new(),
        });
    }

    fn finish(mut self) -> GraphExport {
        self.nodes.sort_by(|left, right| left.id.cmp(&right.id));
        self.edges.sort_by(|left, right| {
            left.kind
                .cmp(right.kind)
                .then_with(|| left.from.cmp(&right.from))
                .then_with(|| left.to.cmp(&right.to))
        });

        GraphExport {
            tool: "sc-lint-boundary",
            version: env!("CARGO_PKG_VERSION"),
            schema_version: SC_LINT_SCHEMA_VERSION,
            nodes: self.nodes,
            edges: self.edges,
        }
    }
}

pub fn analyze_workspace(options: &AnalyzeOptions) -> Result<FindingsReport> {
    let graph = graph::build_workspace_graph(&options.root)?;
    let mut findings = Vec::new();
    let filter = options.rule;
    if filter.is_none() || filter == Some(RuleFilter::Cycles) {
        findings.extend(analysis::analyze_cycles(&graph));
    }
    if filter.is_none()
        || filter == Some(RuleFilter::Boundaries)
        || filter == Some(RuleFilter::InternalOnly)
    {
        findings.extend(analysis::analyze_internal_only(&graph));
    }
    if filter.is_none()
        || filter == Some(RuleFilter::Boundaries)
        || filter == Some(RuleFilter::ForbidExternalImpls)
    {
        findings.extend(analysis::analyze_forbid_external_impls(&graph));
    }
    findings.sort_by(|left, right| {
        analysis::finding_sort_key(left)
            .cmp(&analysis::finding_sort_key(right))
            .then_with(|| left.message.cmp(&right.message))
    });
    let scanned_crates = graph
        .nodes
        .iter()
        .filter(|node| node.kind == "crate")
        .count();
    let status = if findings.iter().any(analysis::finding_is_failure) {
        "fail"
    } else {
        "pass"
    };

    Ok(FindingsReport {
        tool: "sc-lint-boundary",
        version: env!("CARGO_PKG_VERSION"),
        schema_version: SC_LINT_SCHEMA_VERSION,
        status,
        scanned_crates,
        findings,
    })
}

pub fn export_workspace_graph(options: &ExportGraphOptions) -> Result<GraphExport> {
    graph::build_workspace_graph(&options.root)
}

pub fn render_findings_report(report: &FindingsReport) -> String {
    render::render_findings_report(report)
}

pub fn render_graph_export(
    graph: &GraphExport,
    format: GraphOutputFormat,
) -> std::result::Result<String, serde_json::Error> {
    render::render_graph_export(graph, format)
}
