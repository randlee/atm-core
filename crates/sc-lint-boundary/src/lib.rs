use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::Context;
use anyhow::Result;
use cargo_metadata::MetadataCommand;
use quote::ToTokens;
use sc_lint_directives::AttributeInput;
use sc_lint_directives::Directive;
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
mod graph;
mod portability;
mod render;
#[cfg(test)]
mod tests;

const SC_LINT_SCHEMA_VERSION: &str = "0.1.0";
const DEFAULT_RULES_TOML: &str = include_str!("../config/defaults.toml");

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct RuleDefaults {
    trait_self_loop: TraitSelfLoopDefaults,
    portability: PortabilityDefaults,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct TraitSelfLoopDefaults {
    ignored_trait_paths: Vec<String>,
    ignored_trait_names: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct PortabilityDefaults {
    unix_path_prefixes: Vec<String>,
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for NodeId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for NodeId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for NodeId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for NodeId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl PartialEq<&str> for NodeId {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for NodeId {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct OwnerId(String);

impl OwnerId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for OwnerId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for OwnerId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for OwnerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for OwnerId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for OwnerId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl PartialEq<&str> for OwnerId {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for OwnerId {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
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
    pub status: ReportStatus,
    pub scanned_crates: usize,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Pass,
    Fail,
}

impl ReportStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Finding {
    pub rule_id: RuleId,
    pub kind: String,
    pub message: String,
    pub owner_ids: Vec<OwnerId>,
    pub node_ids: Vec<NodeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuleId {
    ScbCycle001,
    ScbCycle002,
    ScbCycle003,
    ScbBoundary001,
    ScbBoundary002,
    ScbBoundary003,
    Port001,
    Port002,
    Port003,
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
            Self::Port001 => "PORT-001",
            Self::Port002 => "PORT-002",
            Self::Port003 => "PORT-003",
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
    Portability,
}

impl RuleFilter {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cycles => "cycles",
            Self::Boundaries => "boundaries",
            Self::InternalOnly => "internal_only",
            Self::ForbidExternalImpls => "forbid_external_impls",
            Self::Portability => "portability",
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
            "unsupported rule filter `{}`; supported: cycles, boundaries, internal_only, forbid_external_impls, portability",
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
            "portability" => Ok(Self::Portability),
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
    pub id: NodeId,
    pub kind: &'static str,
    pub label: String,
    pub visibility: Option<&'static str>,
    pub package: String,
    pub target: Option<String>,
    pub manifest_path: String,
    pub source_path: Option<String>,
    pub module_path: Option<String>,
    pub impl_kind: Option<ImplKind>,
    pub impl_trait: Option<String>,
    pub attributes: Vec<LintAttribute>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GraphEdge {
    pub kind: &'static str,
    pub from: NodeId,
    pub to: NodeId,
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

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImplKind {
    Trait,
    Inherent,
}

impl ImplKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trait => "trait",
            Self::Inherent => "inherent",
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

    fn add_edge(&mut self, kind: &'static str, from: impl Into<NodeId>, to: impl Into<NodeId>) {
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
            id: NodeId::new(crate_id),
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
    if options.rule == Some(RuleFilter::Portability) {
        let findings = portability::analyze_portability(&options.root).with_context(|| {
            format!(
                "failed to analyze portability for root: {}",
                options.root.display()
            )
        })?;
        let scanned_crates =
            portability::count_scanned_crates(&options.root).with_context(|| {
                format!(
                    "failed to count scanned crates for root: {}",
                    options.root.display()
                )
            })?;
        let status = if findings.iter().any(analysis::finding_is_failure) {
            ReportStatus::Fail
        } else {
            ReportStatus::Pass
        };
        return Ok(FindingsReport {
            tool: "sc-lint-boundary",
            version: env!("CARGO_PKG_VERSION"),
            schema_version: SC_LINT_SCHEMA_VERSION,
            status,
            scanned_crates,
            findings,
        });
    }

    let graph = graph::build_workspace_graph(&options.root).with_context(|| {
        format!(
            "failed to build workspace graph for root: {}",
            options.root.display()
        )
    })?;
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
        ReportStatus::Fail
    } else {
        ReportStatus::Pass
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
    graph::build_workspace_graph(&options.root).with_context(|| {
        format!(
            "failed to build workspace graph for root: {}",
            options.root.display()
        )
    })
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

pub fn render_graph_export_json(
    graph: &GraphExport,
) -> std::result::Result<String, serde_json::Error> {
    render::render_graph_export_json(graph)
}

pub fn render_graph_export_turtle(graph: &GraphExport) -> String {
    render::render_graph_export_turtle(graph)
}
