use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use cargo_metadata::MetadataCommand;
use serde::Serialize;
use syn::Attribute;
use syn::File;
use syn::Ident;
use syn::ImplItem;
use syn::Item;
use syn::LitStr;
use syn::Token;
use syn::Type;
use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::visit::Visit;

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
    pub owner_ids: Vec<String>,
    pub node_ids: Vec<String>,
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
    pub target: Option<String>,
    pub manifest_path: String,
    pub source_path: Option<String>,
    pub module_path: Option<String>,
    pub attributes: Vec<LintAttribute>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GraphEdge {
    pub kind: &'static str,
    pub from: String,
    pub to: String,
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
        let crate_id = crate_id(package_name, target_name);
        self.add_node(GraphNode {
            id: crate_id,
            kind: "crate",
            label: target_name.to_string(),
            package: package_name.to_string(),
            target: Some(target_name.to_string()),
            manifest_path: manifest_path.to_string(),
            source_path: Some(source_path.display().to_string()),
            module_path: Some("crate".to_string()),
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
            nodes: self.nodes,
            edges: self.edges,
        }
    }
}

#[derive(Debug)]
enum ParsedLintDirective {
    BoundaryAllow(Vec<String>),
    BoundaryInternalOnly,
}

#[derive(Debug)]
struct ParsedLintInput {
    directives: Vec<ParsedLintDirective>,
}

impl Parse for ParsedLintInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut directives = Vec::new();
        while !input.is_empty() {
            directives.push(parse_directive(input)?);
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }
        Ok(Self { directives })
    }
}

pub fn analyze_workspace(options: &AnalyzeOptions) -> Result<FindingsReport> {
    let graph = build_workspace_graph(&options.root)?;
    let findings = analyze_cycles(&graph, options.rule.as_deref());
    let scanned_crates = graph
        .nodes
        .iter()
        .filter(|node| node.kind == "crate")
        .count();
    let status = if findings
        .iter()
        .any(|finding| finding.rule_id == "SCB-CYCLE-001")
    {
        "fail"
    } else {
        "pass"
    };

    Ok(FindingsReport {
        tool: "sc-lint-boundary",
        version: env!("CARGO_PKG_VERSION"),
        status,
        scanned_crates,
        findings,
    })
}

pub fn export_workspace_graph(options: &ExportGraphOptions) -> Result<GraphExport> {
    build_workspace_graph(&options.root)
}

fn collect_owner_names(items: &[Item]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for item in items {
        match item {
            Item::Struct(item_struct) => {
                names.insert(item_struct.ident.to_string());
            }
            Item::Enum(item_enum) => {
                names.insert(item_enum.ident.to_string());
            }
            Item::Union(item_union) => {
                names.insert(item_union.ident.to_string());
            }
            Item::Type(item_type) => {
                names.insert(item_type.ident.to_string());
            }
            Item::Trait(item_trait) => {
                names.insert(item_trait.ident.to_string());
            }
            _ => {}
        }
    }
    names
}

#[derive(Default)]
struct ReferenceCollector {
    owner_name: Option<String>,
    local_owner_names: BTreeSet<String>,
    references: BTreeSet<String>,
}

impl ReferenceCollector {
    fn new(local_owner_names: &BTreeSet<String>, owner_name: Option<&str>) -> Self {
        Self {
            owner_name: owner_name.map(ToOwned::to_owned),
            local_owner_names: local_owner_names.clone(),
            references: BTreeSet::new(),
        }
    }

    fn into_references(self) -> BTreeSet<String> {
        self.references
    }

    fn maybe_insert(&mut self, ident: &Ident) {
        let name = ident.to_string();
        if name == "Self" {
            if let Some(owner_name) = &self.owner_name {
                self.references.insert(owner_name.clone());
            }
            return;
        }

        if self.local_owner_names.contains(&name) {
            self.references.insert(name);
        }
    }
}

impl<'ast> Visit<'ast> for ReferenceCollector {
    fn visit_type_path(&mut self, type_path: &'ast syn::TypePath) {
        if let Some(segment) = type_path.path.segments.last() {
            self.maybe_insert(&segment.ident);
        }
        syn::visit::visit_type_path(self, type_path);
    }

    fn visit_expr_path(&mut self, expr_path: &'ast syn::ExprPath) {
        if let Some(segment) = expr_path.path.segments.last() {
            self.maybe_insert(&segment.ident);
        }
        syn::visit::visit_expr_path(self, expr_path);
    }
}

fn collect_references_with(
    local_owner_names: &BTreeSet<String>,
    owner_name: Option<&str>,
    visit: impl FnOnce(&mut ReferenceCollector),
) -> BTreeSet<String> {
    let mut collector = ReferenceCollector::new(local_owner_names, owner_name);
    visit(&mut collector);
    collector.into_references()
}

fn build_workspace_graph(root: &Path) -> Result<GraphExport> {
    let metadata = load_metadata(root)?;
    let workspace_members = metadata.workspace_members.clone();
    let mut builder = GraphBuilder::default();

    for package in &metadata.packages {
        if !workspace_members.iter().any(|id| id == &package.id) {
            continue;
        }

        let manifest_path = package.manifest_path.as_std_path().display().to_string();
        for target in &package.targets {
            if !is_supported_target(target) {
                continue;
            }

            let source_path = target.src_path.as_std_path().to_path_buf();
            let context = TargetContext {
                package_name: package.name.to_string(),
                target_name: target.name.clone(),
                manifest_path: manifest_path.clone(),
                crate_id: crate_id(&package.name, &target.name),
            };

            builder.add_workspace_target(
                &context.package_name,
                &context.manifest_path,
                &context.target_name,
                &source_path,
            );

            let root_module_id = format!("{}::module::crate", context.crate_id);
            let root_attributes = Vec::new();
            builder.add_node(GraphNode {
                id: root_module_id.clone(),
                kind: "module",
                label: "crate".to_string(),
                package: context.package_name.clone(),
                target: Some(context.target_name.clone()),
                manifest_path: context.manifest_path.clone(),
                source_path: Some(source_path.display().to_string()),
                module_path: Some("crate".to_string()),
                attributes: root_attributes,
            });
            builder.add_edge("contains", context.crate_id.clone(), root_module_id.clone());

            let root_dir = source_path
                .parent()
                .with_context(|| format!("missing parent dir for {}", source_path.display()))?
                .to_path_buf();
            ingest_module_items(
                &mut builder,
                &context,
                &root_module_id,
                "crate",
                &root_dir,
                &source_path,
                parse_rust_file(&source_path)?,
            )?;
        }
    }

    Ok(builder.finish())
}

fn ingest_module_items(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_module_id: &str,
    module_path: &str,
    module_dir: &Path,
    source_path: &Path,
    file: File,
) -> Result<()> {
    let local_owner_names = collect_owner_names(&file.items);

    for item in file.items {
        match item {
            Item::Mod(item_mod) => {
                let name = item_mod.ident.to_string();
                let child_module_path = format!("{module_path}::{name}");
                let child_module_id = format!("{}::module::{child_module_path}", context.crate_id);
                let child_module_dir = module_dir.join(&name);
                let attributes = parse_lint_attributes(&item_mod.attrs)?;

                builder.add_node(GraphNode {
                    id: child_module_id.clone(),
                    kind: "module",
                    label: name.clone(),
                    package: context.package_name.clone(),
                    target: Some(context.target_name.clone()),
                    manifest_path: context.manifest_path.clone(),
                    source_path: Some(source_path.display().to_string()),
                    module_path: Some(child_module_path.clone()),
                    attributes,
                });
                builder.add_edge(
                    "contains",
                    parent_module_id.to_string(),
                    child_module_id.clone(),
                );

                if let Some((_, items)) = item_mod.content {
                    let inline_file = File {
                        shebang: None,
                        attrs: Vec::new(),
                        items,
                    };
                    ingest_module_items(
                        builder,
                        context,
                        &child_module_id,
                        &child_module_path,
                        &child_module_dir,
                        source_path,
                        inline_file,
                    )?;
                } else {
                    let child_source_path = resolve_module_source(module_dir, &name)
                        .with_context(|| format!("while resolving module `{child_module_path}`"))?;
                    let child_file = parse_rust_file(&child_source_path)?;
                    ingest_module_items(
                        builder,
                        context,
                        &child_module_id,
                        &child_module_path,
                        &child_module_dir,
                        &child_source_path,
                        child_file,
                    )?;
                }
            }
            Item::Struct(item_struct) => {
                let owner_name = item_struct.ident.to_string();
                let node_id = add_item_node(
                    builder,
                    context,
                    parent_module_id,
                    module_path,
                    source_path,
                    item_struct.ident.clone(),
                    "type",
                    parse_lint_attributes(&item_struct.attrs)?,
                );
                add_reference_edges(
                    builder,
                    &node_id,
                    module_path,
                    collect_references_with(&local_owner_names, Some(&owner_name), |collector| {
                        collector.visit_fields(&item_struct.fields);
                    }),
                );
            }
            Item::Enum(item_enum) => {
                let owner_name = item_enum.ident.to_string();
                let node_id = add_item_node(
                    builder,
                    context,
                    parent_module_id,
                    module_path,
                    source_path,
                    item_enum.ident.clone(),
                    "type",
                    parse_lint_attributes(&item_enum.attrs)?,
                );
                add_reference_edges(
                    builder,
                    &node_id,
                    module_path,
                    collect_references_with(&local_owner_names, Some(&owner_name), |collector| {
                        for variant in &item_enum.variants {
                            collector.visit_fields(&variant.fields);
                        }
                    }),
                );
            }
            Item::Union(item_union) => {
                let owner_name = item_union.ident.to_string();
                let node_id = add_item_node(
                    builder,
                    context,
                    parent_module_id,
                    module_path,
                    source_path,
                    item_union.ident.clone(),
                    "type",
                    parse_lint_attributes(&item_union.attrs)?,
                );
                add_reference_edges(
                    builder,
                    &node_id,
                    module_path,
                    collect_references_with(&local_owner_names, Some(&owner_name), |collector| {
                        collector.visit_fields_named(&item_union.fields);
                    }),
                );
            }
            Item::Type(item_type) => {
                let owner_name = item_type.ident.to_string();
                let node_id = add_item_node(
                    builder,
                    context,
                    parent_module_id,
                    module_path,
                    source_path,
                    item_type.ident.clone(),
                    "type",
                    parse_lint_attributes(&item_type.attrs)?,
                );
                add_reference_edges(
                    builder,
                    &node_id,
                    module_path,
                    collect_references_with(&local_owner_names, Some(&owner_name), |collector| {
                        collector.visit_type(&item_type.ty);
                    }),
                );
            }
            Item::Trait(item_trait) => {
                let owner_name = item_trait.ident.to_string();
                let node_id = add_item_node(
                    builder,
                    context,
                    parent_module_id,
                    module_path,
                    source_path,
                    item_trait.ident.clone(),
                    "trait",
                    parse_lint_attributes(&item_trait.attrs)?,
                );
                add_reference_edges(
                    builder,
                    &node_id,
                    module_path,
                    collect_references_with(&local_owner_names, Some(&owner_name), |collector| {
                        for trait_item in &item_trait.items {
                            collector.visit_trait_item(trait_item);
                        }
                    }),
                );
            }
            Item::Fn(item_fn) => {
                let function_ident = item_fn.sig.ident.clone();
                let node_id = add_item_node(
                    builder,
                    context,
                    parent_module_id,
                    module_path,
                    source_path,
                    function_ident,
                    "function",
                    parse_lint_attributes(&item_fn.attrs)?,
                );
                add_reference_edges(
                    builder,
                    &node_id,
                    module_path,
                    collect_references_with(&local_owner_names, None, |collector| {
                        collector.visit_item_fn(&item_fn);
                    }),
                );
            }
            Item::Impl(item_impl) => {
                let owner_name = impl_owner_name(&item_impl.self_ty);
                let owner_node_id = format!("{parent_module_id}::{owner_name}");

                if !builder.nodes.iter().any(|node| node.id == owner_node_id) {
                    builder.add_node(GraphNode {
                        id: owner_node_id.clone(),
                        kind: "type",
                        label: owner_name.to_string(),
                        package: context.package_name.clone(),
                        target: Some(context.target_name.clone()),
                        manifest_path: context.manifest_path.clone(),
                        source_path: Some(source_path.display().to_string()),
                        module_path: Some(module_path.to_string()),
                        attributes: Vec::new(),
                    });
                    builder.add_edge(
                        "contains",
                        parent_module_id.to_string(),
                        owner_node_id.clone(),
                    );
                }

                for impl_item in item_impl.items {
                    if let ImplItem::Fn(method) = impl_item {
                        let method_id = format!("{owner_node_id}::{}", method.sig.ident);
                        builder.add_node(GraphNode {
                            id: method_id.clone(),
                            kind: "method",
                            label: method.sig.ident.to_string(),
                            package: context.package_name.clone(),
                            target: Some(context.target_name.clone()),
                            manifest_path: context.manifest_path.clone(),
                            source_path: Some(source_path.display().to_string()),
                            module_path: Some(module_path.to_string()),
                            attributes: parse_lint_attributes(&method.attrs)?,
                        });
                        builder.add_edge("declares", owner_node_id.clone(), method_id.clone());
                        add_reference_edges(
                            builder,
                            &method_id,
                            module_path,
                            collect_references_with(
                                &local_owner_names,
                                Some(&owner_name),
                                |collector| {
                                    collector.visit_impl_item_fn(&method);
                                },
                            ),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn add_item_node(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_module_id: &str,
    module_path: &str,
    source_path: &Path,
    ident: Ident,
    kind: &'static str,
    attributes: Vec<LintAttribute>,
) -> String {
    let id = format!("{parent_module_id}::{ident}");
    builder.add_node(GraphNode {
        id: id.clone(),
        kind,
        label: ident.to_string(),
        package: context.package_name.clone(),
        target: Some(context.target_name.clone()),
        manifest_path: context.manifest_path.clone(),
        source_path: Some(source_path.display().to_string()),
        module_path: Some(module_path.to_string()),
        attributes,
    });
    builder.add_edge("contains", parent_module_id.to_string(), id.clone());
    id
}

fn add_reference_edges(
    builder: &mut GraphBuilder,
    source_node_id: &str,
    module_path: &str,
    referenced_names: BTreeSet<String>,
) {
    let module_node_id = source_node_id
        .split("::module::")
        .next()
        .map(|crate_prefix| format!("{crate_prefix}::module::{module_path}"))
        .unwrap_or_else(|| source_node_id.to_string());

    for referenced_name in referenced_names {
        let target_node_id = format!("{module_node_id}::{referenced_name}");
        builder.add_edge("references", source_node_id.to_string(), target_node_id);
    }
}

fn parse_lint_attributes(attrs: &[Attribute]) -> Result<Vec<LintAttribute>> {
    let mut parsed = Vec::new();

    for attr in attrs {
        if !attr.path().is_ident("sc_lint") {
            continue;
        }
        let input = attr.parse_args::<ParsedLintInput>()?;
        for directive in input.directives {
            match directive {
                ParsedLintDirective::BoundaryAllow(values) => {
                    if values.iter().any(|value| value.trim().is_empty()) {
                        anyhow::bail!("boundary.allow rule ids must not be empty");
                    }
                    parsed.push(LintAttribute {
                        scope: "boundary",
                        name: "allow",
                        values,
                    });
                }
                ParsedLintDirective::BoundaryInternalOnly => {
                    parsed.push(LintAttribute {
                        scope: "boundary",
                        name: "internal_only",
                        values: Vec::new(),
                    });
                }
            }
        }
    }

    Ok(parsed)
}

fn parse_directive(input: ParseStream<'_>) -> syn::Result<ParsedLintDirective> {
    let scope = input.parse::<Ident>()?;
    let scope_name = scope.to_string();
    input.parse::<Token![.]>()?;
    let action = input.parse::<Ident>()?;
    let action_name = action.to_string();

    match (scope_name.as_str(), action_name.as_str()) {
        ("boundary", "allow") => {
            let content;
            syn::parenthesized!(content in input);
            let mut values = Vec::new();
            while !content.is_empty() {
                values.push(content.parse::<LitStr>()?.value());
                if content.is_empty() {
                    break;
                }
                content.parse::<Token![,]>()?;
            }
            if values.is_empty() {
                return Err(syn::Error::new(
                    action.span(),
                    "boundary.allow requires at least one rule id string",
                ));
            }
            Ok(ParsedLintDirective::BoundaryAllow(values))
        }
        ("boundary", "internal_only") => Ok(ParsedLintDirective::BoundaryInternalOnly),
        ("boundary", _) => Err(syn::Error::new(
            action.span(),
            format!(
                "unsupported boundary directive `{action_name}`; supported: allow(...), internal_only"
            ),
        )),
        _ => Err(syn::Error::new(
            scope.span(),
            format!("unsupported sc_lint scope `{scope_name}`; supported: boundary"),
        )),
    }
}

fn resolve_module_source(module_dir: &Path, module_name: &str) -> Result<PathBuf> {
    let flat = module_dir.join(format!("{module_name}.rs"));
    let nested = module_dir.join(module_name).join("mod.rs");

    let flat_exists = flat.is_file();
    let nested_exists = nested.is_file();

    match (flat_exists, nested_exists) {
        (true, false) => Ok(flat),
        (false, true) => Ok(nested),
        (true, true) => anyhow::bail!(
            "ambiguous module `{module_name}`: found both {} and {}",
            flat.display(),
            nested.display()
        ),
        (false, false) => anyhow::bail!(
            "module `{module_name}` not found; expected {} or {}",
            flat.display(),
            nested.display()
        ),
    }
}

fn parse_rust_file(path: &Path) -> Result<File> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read Rust source {}", path.display()))?;
    syn::parse_file(&source)
        .with_context(|| format!("failed to parse Rust source {}", path.display()))
}

fn impl_owner_name(self_ty: &Type) -> String {
    match self_ty {
        Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_else(|| "impl_owner".to_string()),
        _ => "impl_owner".to_string(),
    }
}

fn is_supported_target(target: &cargo_metadata::Target) -> bool {
    target.kind.iter().any(|kind| {
        matches!(
            kind,
            cargo_metadata::TargetKind::Lib
                | cargo_metadata::TargetKind::Bin
                | cargo_metadata::TargetKind::Example
        )
    })
}

fn crate_id(package_name: &str, target_name: &str) -> String {
    format!("crate::{package_name}::{target_name}")
}

fn load_metadata(root: &Path) -> Result<cargo_metadata::Metadata> {
    MetadataCommand::new()
        .current_dir(root)
        .exec()
        .with_context(|| format!("failed to load cargo metadata for {}", root.display()))
}

fn analyze_cycles(graph: &GraphExport, rule_filter: Option<&str>) -> Vec<Finding> {
    if let Some(rule) = rule_filter {
        if rule != "cycles" {
            return Vec::new();
        }
    }

    let node_map: BTreeMap<_, _> = graph
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect();
    let owner_graph = build_owner_graph(graph, &node_map);
    let sccs = strongly_connected_components(&owner_graph);

    let mut findings = Vec::new();
    for component in sccs {
        if component.len() > 1 {
            let mut owners = component.clone();
            owners.sort();
            findings.push(Finding {
                rule_id: "SCB-CYCLE-001".to_string(),
                kind: "multi_owner_architectural_cycle".to_string(),
                message: format!("architectural cycle across owners: {}", owners.join(", ")),
                owner_ids: owners.clone(),
                node_ids: owner_contributors(&owners, &owner_graph),
            });
            continue;
        }

        let owner_id = &component[0];
        let Some(self_refs) = owner_graph.self_refs.get(owner_id) else {
            continue;
        };
        if self_refs.is_empty() {
            continue;
        }

        let allow_rule = self_refs.iter().any(|edge| {
            edge.node_ids.iter().any(|node_id| {
                node_map
                    .get(node_id)
                    .map(|node| {
                        node.attributes.iter().any(|attr| {
                            attr.scope == "boundary"
                                && attr.name == "allow"
                                && attr
                                    .values
                                    .iter()
                                    .any(|value| value == "cycle.type_method_self_loop")
                        })
                    })
                    .unwrap_or(false)
            })
        });

        let is_type_method_self_loop = self_refs.iter().all(|edge| {
            edge.source_kind == "method"
                && edge.owner_kind == "type"
                && edge.target_owner_id == edge.source_owner_id
        });

        if is_type_method_self_loop && !allow_rule {
            findings.push(Finding {
                rule_id: "SCB-CYCLE-002".to_string(),
                kind: "type_method_self_loop".to_string(),
                message: format!("type/method self-loop on owner {owner_id}"),
                owner_ids: vec![owner_id.clone()],
                node_ids: self_refs
                    .iter()
                    .flat_map(|edge| edge.node_ids.clone())
                    .collect(),
            });
        }
    }

    findings
}

#[derive(Debug, Clone)]
struct OwnerRefEdge {
    source_owner_id: String,
    target_owner_id: String,
    owner_kind: &'static str,
    source_kind: &'static str,
    node_ids: Vec<String>,
}

#[derive(Default)]
struct OwnerGraph {
    adjacency: BTreeMap<String, BTreeSet<String>>,
    self_refs: BTreeMap<String, Vec<OwnerRefEdge>>,
    ref_edges: Vec<OwnerRefEdge>,
}

fn build_owner_graph<'a>(
    graph: &'a GraphExport,
    node_map: &BTreeMap<String, &'a GraphNode>,
) -> OwnerGraph {
    let mut owner_graph = OwnerGraph::default();

    for edge in graph.edges.iter().filter(|edge| edge.kind == "references") {
        let Some(source_node) = node_map.get(&edge.from) else {
            continue;
        };
        let Some(target_node) = node_map.get(&edge.to) else {
            continue;
        };

        let Some(source_owner_id) = owner_id_for_node_id(&source_node.id, source_node.kind) else {
            continue;
        };
        let Some(target_owner_id) = owner_id_for_node_id(&target_node.id, target_node.kind) else {
            continue;
        };

        let owner_edge = OwnerRefEdge {
            source_owner_id: source_owner_id.clone(),
            target_owner_id: target_owner_id.clone(),
            owner_kind: owner_kind_for_node_id(&source_owner_id, node_map).unwrap_or("module"),
            source_kind: source_node.kind,
            node_ids: vec![source_node.id.clone(), target_node.id.clone()],
        };

        owner_graph
            .adjacency
            .entry(source_owner_id.clone())
            .or_default()
            .insert(target_owner_id.clone());
        owner_graph
            .adjacency
            .entry(target_owner_id.clone())
            .or_default();

        if source_owner_id == target_owner_id {
            owner_graph
                .self_refs
                .entry(source_owner_id.clone())
                .or_default()
                .push(owner_edge.clone());
        }

        owner_graph.ref_edges.push(owner_edge);
    }

    owner_graph
}

fn owner_contributors(owners: &[String], owner_graph: &OwnerGraph) -> Vec<String> {
    let owner_set: BTreeSet<_> = owners.iter().cloned().collect();
    let mut nodes = BTreeSet::new();
    for edge in &owner_graph.ref_edges {
        if owner_set.contains(&edge.source_owner_id) && owner_set.contains(&edge.target_owner_id) {
            for node_id in &edge.node_ids {
                nodes.insert(node_id.clone());
            }
        }
    }
    nodes.into_iter().collect()
}

fn owner_kind_for_node_id<'a>(
    owner_id: &str,
    node_map: &BTreeMap<String, &'a GraphNode>,
) -> Option<&'static str> {
    node_map.get(owner_id).map(|node| node.kind)
}

fn owner_id_for_node_id(node_id: &str, node_kind: &str) -> Option<String> {
    match node_kind {
        "module" | "type" | "trait" => Some(node_id.to_string()),
        "function" => node_id
            .rsplit_once("::")
            .map(|(parent, _)| parent.to_string()),
        "method" => node_id
            .rsplit_once("::")
            .map(|(parent, _)| parent.to_string()),
        _ => None,
    }
}

fn strongly_connected_components(owner_graph: &OwnerGraph) -> Vec<Vec<String>> {
    fn visit(
        node: &str,
        owner_graph: &OwnerGraph,
        visited: &mut BTreeSet<String>,
        order: &mut Vec<String>,
    ) {
        if !visited.insert(node.to_string()) {
            return;
        }
        if let Some(neighbors) = owner_graph.adjacency.get(node) {
            for neighbor in neighbors {
                visit(neighbor, owner_graph, visited, order);
            }
        }
        order.push(node.to_string());
    }

    fn reverse_graph(owner_graph: &OwnerGraph) -> BTreeMap<String, BTreeSet<String>> {
        let mut reversed = BTreeMap::new();
        for (source, targets) in &owner_graph.adjacency {
            reversed.entry(source.clone()).or_insert_with(BTreeSet::new);
            for target in targets {
                reversed
                    .entry(target.clone())
                    .or_insert_with(BTreeSet::new)
                    .insert(source.clone());
            }
        }
        reversed
    }

    fn collect_component(
        node: &str,
        reversed: &BTreeMap<String, BTreeSet<String>>,
        visited: &mut BTreeSet<String>,
        component: &mut Vec<String>,
    ) {
        if !visited.insert(node.to_string()) {
            return;
        }
        component.push(node.to_string());
        if let Some(neighbors) = reversed.get(node) {
            for neighbor in neighbors {
                collect_component(neighbor, reversed, visited, component);
            }
        }
    }

    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    for node in owner_graph.adjacency.keys() {
        visit(node, owner_graph, &mut visited, &mut order);
    }

    let reversed = reverse_graph(owner_graph);
    let mut assigned = BTreeSet::new();
    let mut components = Vec::new();
    while let Some(node) = order.pop() {
        if assigned.contains(&node) {
            continue;
        }
        let mut component = Vec::new();
        collect_component(&node, &reversed, &mut assigned, &mut component);
        components.push(component);
    }
    components
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
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::AnalyzeOptions;
    use super::ExportGraphOptions;
    use super::GraphExport;
    use super::LintAttribute;
    use super::OutputFormat;
    use super::analyze_workspace;
    use super::export_workspace_graph;
    use super::render_findings_report;

    #[test]
    fn findings_report_text_is_stable() {
        let report = super::FindingsReport {
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

    #[test]
    fn exports_graph_for_inline_and_file_modules_and_attributes() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                #[sc_lint(boundary.internal_only)]
                pub struct Root;

                mod file_mod;

                mod inline_mod {
                    #[sc_lint(boundary.allow("cycle.type_method_self_loop"))]
                    pub struct InlineType;

                    impl InlineType {
                        #[sc_lint(boundary.allow("cycle.type_method_self_loop"))]
                        pub fn helper(&self) {}
                    }
                }
            "#,
        );
        fixture.write_source(
            "example",
            "file_mod.rs",
            r#"
                pub mod nested;
                pub trait Worker {}
            "#,
        );
        fixture.write_source("example", "file_mod/nested.rs", "pub struct FileType;");

        let graph = export_workspace_graph(&ExportGraphOptions {
            root: fixture.root().to_path_buf(),
        })
        .unwrap();

        assert!(
            graph
                .nodes
                .iter()
                .any(|node| node.id == "crate::example::example")
        );
        assert!(
            graph
                .nodes
                .iter()
                .any(|node| node.id == "crate::example::example::module::crate::inline_mod")
        );
        assert!(
            graph
                .nodes
                .iter()
                .any(|node| node.id == "crate::example::example::module::crate::file_mod::nested")
        );

        let root_type = graph
            .nodes
            .iter()
            .find(|node| node.id == "crate::example::example::module::crate::Root")
            .unwrap();
        assert_eq!(
            root_type.attributes,
            vec![LintAttribute {
                scope: "boundary",
                name: "internal_only",
                values: Vec::new(),
            }]
        );

        let inline_type = graph
            .nodes
            .iter()
            .find(|node| {
                node.id == "crate::example::example::module::crate::inline_mod::InlineType"
            })
            .unwrap();
        assert_eq!(
            inline_type.attributes,
            vec![LintAttribute {
                scope: "boundary",
                name: "allow",
                values: vec!["cycle.type_method_self_loop".to_string()],
            }]
        );

        let helper_method = graph
            .nodes
            .iter()
            .find(|node| {
                node.id == "crate::example::example::module::crate::inline_mod::InlineType::helper"
            })
            .unwrap();
        assert_eq!(helper_method.kind, "method");
        assert_eq!(
            helper_method.attributes,
            vec![LintAttribute {
                scope: "boundary",
                name: "allow",
                values: vec!["cycle.type_method_self_loop".to_string()],
            }]
        );
    }

    #[test]
    fn resolves_mod_rs_module_layout() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source("example", "lib.rs", "mod outer;");
        fixture.write_source("example", "outer/mod.rs", "mod child;");
        fixture.write_source("example", "outer/child.rs", "pub struct Nested;");

        let graph = export_workspace_graph(&ExportGraphOptions {
            root: fixture.root().to_path_buf(),
        })
        .unwrap();

        assert!(graph.nodes.iter().any(|node| {
            node.id == "crate::example::example::module::crate::outer::child::Nested"
        }));
    }

    #[test]
    fn analyze_workspace_counts_crate_targets() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source("example", "lib.rs", "pub struct Example;");

        let report = analyze_workspace(&AnalyzeOptions {
            root: fixture.root().to_path_buf(),
            format: OutputFormat::Json,
            rule: None,
        })
        .unwrap();

        assert_eq!(report.scanned_crates, 1);
        assert_eq!(report.status, "pass");
        assert!(report.findings.is_empty());
    }

    #[test]
    fn reports_type_method_self_loop_as_non_fatal_signal() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Loop;

                impl Loop {
                    pub fn build() -> Loop {
                        Loop
                    }
                }
            "#,
        );

        let report = analyze_workspace(&AnalyzeOptions {
            root: fixture.root().to_path_buf(),
            format: OutputFormat::Json,
            rule: Some("cycles".to_string()),
        })
        .unwrap();

        assert_eq!(report.status, "pass");
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].rule_id, "SCB-CYCLE-002");
        assert_eq!(report.findings[0].kind, "type_method_self_loop");
        assert_eq!(
            report.findings[0].owner_ids,
            vec!["crate::example::example::module::crate::Loop".to_string()]
        );
    }

    #[test]
    fn suppresses_type_method_self_loop_when_allowed() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                #[sc_lint(boundary.allow("cycle.type_method_self_loop"))]
                pub struct Loop;

                impl Loop {
                    pub fn build() -> Loop {
                        Loop
                    }
                }
            "#,
        );

        let report = analyze_workspace(&AnalyzeOptions {
            root: fixture.root().to_path_buf(),
            format: OutputFormat::Json,
            rule: Some("cycles".to_string()),
        })
        .unwrap();

        assert_eq!(report.status, "pass");
        assert!(report.findings.is_empty());
    }

    #[test]
    fn reports_multi_owner_architectural_cycle_as_failure() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Alpha {
                    pub beta: Beta,
                }

                pub struct Beta {
                    pub alpha: Alpha,
                }
            "#,
        );

        let report = analyze_workspace(&AnalyzeOptions {
            root: fixture.root().to_path_buf(),
            format: OutputFormat::Json,
            rule: Some("cycles".to_string()),
        })
        .unwrap();

        assert_eq!(report.status, "fail");
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].rule_id, "SCB-CYCLE-001");
        assert_eq!(report.findings[0].kind, "multi_owner_architectural_cycle");
        assert_eq!(
            report.findings[0].owner_ids,
            vec![
                "crate::example::example::module::crate::Alpha".to_string(),
                "crate::example::example::module::crate::Beta".to_string(),
            ]
        );
    }

    #[test]
    fn fails_when_external_module_is_missing() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source("example", "lib.rs", "mod missing;");

        let error = export_workspace_graph(&ExportGraphOptions {
            root: fixture.root().to_path_buf(),
        })
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("while resolving module `crate::missing`"));
    }

    #[test]
    fn fails_when_module_resolution_is_ambiguous() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source("example", "lib.rs", "mod dup;");
        fixture.write_source("example", "dup.rs", "pub struct A;");
        fixture.write_source("example", "dup/mod.rs", "pub struct B;");

        let error = export_workspace_graph(&ExportGraphOptions {
            root: fixture.root().to_path_buf(),
        })
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("while resolving module `crate::dup`"));
    }

    #[test]
    fn fails_when_sc_lint_attribute_is_invalid() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                #[sc_lint(boundary.allow(""))]
                pub struct Example;
            "#,
        );

        let error = export_workspace_graph(&ExportGraphOptions {
            root: fixture.root().to_path_buf(),
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("boundary.allow rule ids must not be empty")
        );
    }

    struct WorkspaceFixture {
        tempdir: TempDir,
    }

    impl WorkspaceFixture {
        fn new() -> Self {
            Self {
                tempdir: TempDir::new().unwrap(),
            }
        }

        fn root(&self) -> &Path {
            self.tempdir.path()
        }

        fn write_workspace_root(&self) {
            self.write(
                "Cargo.toml",
                r#"
                    [workspace]
                    members = ["crates/example"]
                    resolver = "2"
                "#,
            );
        }

        fn write_package_manifest(&self, package_name: &str) {
            self.write(
                &format!("crates/{package_name}/Cargo.toml"),
                &format!(
                    r#"
                        [package]
                        name = "{package_name}"
                        version = "0.1.0"
                        edition = "2024"
                    "#
                ),
            );
        }

        fn write_source(&self, package_name: &str, relative_path: &str, contents: &str) {
            self.write(
                &format!("crates/{package_name}/src/{relative_path}"),
                contents,
            );
        }

        fn write(&self, relative_path: &str, contents: &str) {
            let path = self.root().join(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, trim_indentation(contents)).unwrap();
        }
    }

    fn trim_indentation(input: &str) -> String {
        let lines: Vec<_> = input.lines().collect();
        let first_content = lines
            .iter()
            .find(|line| !line.trim().is_empty())
            .map(|line| line.chars().take_while(|ch| ch.is_whitespace()).count())
            .unwrap_or(0);

        let mut output = String::new();
        for line in lines {
            let trimmed = if line.len() >= first_content {
                &line[first_content..]
            } else {
                line.trim_end()
            };
            output.push_str(trimmed);
            output.push('\n');
        }
        output
    }
}
