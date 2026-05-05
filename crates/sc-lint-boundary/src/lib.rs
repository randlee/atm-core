use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use cargo_metadata::MetadataCommand;
use quote::ToTokens;
use serde::Serialize;
use syn::Attribute;
use syn::File;
use syn::Ident;
use syn::ImplItem;
use syn::Item;
use syn::LitStr;
use syn::Receiver;
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
        let crate_id = crate_id(package_name, target_name);
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

#[derive(Debug)]
enum ParsedLintDirective {
    BoundaryAllow(Vec<String>),
    BoundaryInternalOnly,
    BoundaryForbidExternalImpls,
}

#[derive(Debug)]
struct ParsedLintInput {
    directives: Vec<ParsedLintDirective>,
}

const SC_LINT_SCHEMA_VERSION: &str = "0.1.0";

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
    validate_rule_filter(options.rule.as_deref())?;
    let graph = build_workspace_graph(&options.root)?;
    let mut findings = Vec::new();
    let filter = options.rule.as_deref();
    if filter.is_none() || filter == Some("cycles") {
        findings.extend(analyze_cycles(&graph, filter));
    }
    if filter.is_none() || matches!(filter, Some("boundaries" | "internal_only")) {
        findings.extend(analyze_internal_only(&graph));
        findings.extend(analyze_forbid_external_impls(&graph));
    }
    findings.sort_by(|left, right| {
        finding_sort_key(left)
            .cmp(&finding_sort_key(right))
            .then_with(|| left.message.cmp(&right.message))
    });
    let scanned_crates = graph
        .nodes
        .iter()
        .filter(|node| node.kind == "crate")
        .count();
    let status = if findings.iter().any(finding_is_failure) {
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

fn visibility_label(visibility: &syn::Visibility) -> ItemVisibility {
    match visibility {
        syn::Visibility::Inherited => ItemVisibility::Private,
        syn::Visibility::Public(_) => ItemVisibility::Public,
        syn::Visibility::Restricted(restricted) => {
            if restricted.path.is_ident("crate") {
                ItemVisibility::Crate
            } else {
                ItemVisibility::Restricted
            }
        }
    }
}

#[derive(Default)]
struct ReferenceCollector {
    owner_name: Option<String>,
    local_owner_names: BTreeSet<String>,
    references: BTreeSet<CollectedReference>,
}

impl ReferenceCollector {
    fn new(local_owner_names: &BTreeSet<String>, owner_name: Option<&str>) -> Self {
        Self {
            owner_name: owner_name.map(ToOwned::to_owned),
            local_owner_names: local_owner_names.clone(),
            references: BTreeSet::new(),
        }
    }

    fn into_references(self) -> BTreeSet<CollectedReference> {
        self.references
    }

    fn maybe_insert_path(&mut self, path: &syn::Path, kind: ReferenceKind) {
        let mut segments: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        if segments.is_empty() {
            return;
        }

        if segments[0] == "Self" {
            if let Some(owner_name) = &self.owner_name {
                segments[0] = owner_name.clone();
                self.references.insert(CollectedReference {
                    path: segments.join("::"),
                    kind,
                });
            }
            return;
        }

        if matches!(kind, ReferenceKind::Expr) && segments.len() == 1 && segments[0] == "self" {
            return;
        }

        let first = &segments[0];
        let last = segments.last().unwrap();
        let should_collect = matches!(first.as_str(), "crate" | "self" | "super")
            || self.local_owner_names.contains(first)
            || self.local_owner_names.contains(last);

        if should_collect {
            self.references.insert(CollectedReference {
                path: segments.join("::"),
                kind,
            });
        }
    }
}

impl<'ast> Visit<'ast> for ReferenceCollector {
    fn visit_type_path(&mut self, type_path: &'ast syn::TypePath) {
        self.maybe_insert_path(&type_path.path, ReferenceKind::Type);
        syn::visit::visit_type_path(self, type_path);
    }

    fn visit_expr_path(&mut self, expr_path: &'ast syn::ExprPath) {
        self.maybe_insert_path(&expr_path.path, ReferenceKind::Expr);
        syn::visit::visit_expr_path(self, expr_path);
    }

    fn visit_receiver(&mut self, _receiver: &'ast Receiver) {}
}

fn collect_references_with(
    local_owner_names: &BTreeSet<String>,
    owner_name: Option<&str>,
    visit: impl FnOnce(&mut ReferenceCollector),
) -> BTreeSet<CollectedReference> {
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
                visibility: None,
                package: context.package_name.clone(),
                target: Some(context.target_name.clone()),
                manifest_path: context.manifest_path.clone(),
                source_path: Some(source_path.display().to_string()),
                module_path: Some("crate".to_string()),
                impl_kind: None,
                impl_trait: None,
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

fn validate_rule_filter(rule_filter: Option<&str>) -> Result<()> {
    match rule_filter {
        None | Some("cycles" | "boundaries" | "internal_only") => Ok(()),
        Some(other) => anyhow::bail!(
            "unsupported rule filter `{other}`; supported: cycles, boundaries, internal_only"
        ),
    }
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
                    visibility: Some(visibility_label(&item_mod.vis).as_str()),
                    package: context.package_name.clone(),
                    target: Some(context.target_name.clone()),
                    manifest_path: context.manifest_path.clone(),
                    source_path: Some(source_path.display().to_string()),
                    module_path: Some(child_module_path.clone()),
                    impl_kind: None,
                    impl_trait: None,
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
                    visibility_label(&item_struct.vis),
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
                add_field_nodes(
                    builder,
                    context,
                    &node_id,
                    module_path,
                    source_path,
                    &local_owner_names,
                    Some(&owner_name),
                    &item_struct.fields,
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
                    visibility_label(&item_enum.vis),
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
                for variant in &item_enum.variants {
                    let variant_id = format!("{node_id}::variant::{}", variant.ident);
                    builder.add_node(GraphNode {
                        id: variant_id.clone(),
                        kind: "variant",
                        label: variant.ident.to_string(),
                        visibility: None,
                        package: context.package_name.clone(),
                        target: Some(context.target_name.clone()),
                        manifest_path: context.manifest_path.clone(),
                        source_path: Some(source_path.display().to_string()),
                        module_path: Some(module_path.to_string()),
                        impl_kind: None,
                        impl_trait: None,
                        attributes: Vec::new(),
                    });
                    builder.add_edge("contains", node_id.clone(), variant_id.clone());
                    add_field_nodes(
                        builder,
                        context,
                        &variant_id,
                        module_path,
                        source_path,
                        &local_owner_names,
                        Some(&owner_name),
                        &variant.fields,
                    );
                }
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
                    visibility_label(&item_union.vis),
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
                add_field_nodes(
                    builder,
                    context,
                    &node_id,
                    module_path,
                    source_path,
                    &local_owner_names,
                    Some(&owner_name),
                    &syn::Fields::Named(item_union.fields.clone()),
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
                    visibility_label(&item_type.vis),
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
                    visibility_label(&item_trait.vis),
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
                    visibility_label(&item_fn.vis),
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
                let owner_name = impl_owner_name(&item_impl.self_ty)?;
                let owner_node_id = format!("{parent_module_id}::{owner_name}");
                let trait_path = item_impl
                    .trait_
                    .as_ref()
                    .map(|(_, path, _)| path_to_string(path));
                let impl_node_id = if let Some(trait_path) = &trait_path {
                    format!(
                        "{owner_node_id}::impl::{}",
                        hex_encode(trait_path.as_bytes())
                    )
                } else {
                    format!("{owner_node_id}::impl::inherent")
                };

                if !builder.nodes.iter().any(|node| node.id == owner_node_id) {
                    builder.add_node(GraphNode {
                        id: owner_node_id.clone(),
                        kind: "type",
                        label: owner_name.to_string(),
                        visibility: None,
                        package: context.package_name.clone(),
                        target: Some(context.target_name.clone()),
                        manifest_path: context.manifest_path.clone(),
                        source_path: Some(source_path.display().to_string()),
                        module_path: Some(module_path.to_string()),
                        impl_kind: None,
                        impl_trait: None,
                        attributes: Vec::new(),
                    });
                    builder.add_edge(
                        "contains",
                        parent_module_id.to_string(),
                        owner_node_id.clone(),
                    );
                }

                builder.add_node(GraphNode {
                    id: impl_node_id.clone(),
                    kind: "impl",
                    label: trait_path
                        .as_ref()
                        .map(|path| format!("impl {path} for {owner_name}"))
                        .unwrap_or_else(|| format!("impl {owner_name}")),
                    visibility: None,
                    package: context.package_name.clone(),
                    target: Some(context.target_name.clone()),
                    manifest_path: context.manifest_path.clone(),
                    source_path: Some(source_path.display().to_string()),
                    module_path: Some(module_path.to_string()),
                    impl_kind: Some(if trait_path.is_some() {
                        "trait"
                    } else {
                        "inherent"
                    }),
                    impl_trait: trait_path.clone(),
                    attributes: Vec::new(),
                });
                builder.add_edge(
                    "contains",
                    parent_module_id.to_string(),
                    impl_node_id.clone(),
                );
                builder.add_edge("targets", impl_node_id.clone(), owner_node_id.clone());

                if let Some((_, path, _)) = &item_impl.trait_ {
                    let trait_reference_path = path_to_string(path);
                    let trait_target_node_id =
                        resolve_reference_target(&impl_node_id, module_path, &trait_reference_path);
                    ensure_trait_reference_node(
                        builder,
                        context,
                        source_path,
                        module_path,
                        &trait_target_node_id,
                        &trait_reference_path,
                    );
                    builder.add_edge("implements", impl_node_id.clone(), trait_target_node_id);
                }

                for impl_item in item_impl.items {
                    if let ImplItem::Fn(method) = impl_item {
                        let method_id = format!("{owner_node_id}::{}", method.sig.ident);
                        builder.add_node(GraphNode {
                            id: method_id.clone(),
                            kind: "method",
                            label: method.sig.ident.to_string(),
                            visibility: Some(visibility_label(&method.vis).as_str()),
                            package: context.package_name.clone(),
                            target: Some(context.target_name.clone()),
                            manifest_path: context.manifest_path.clone(),
                            source_path: Some(source_path.display().to_string()),
                            module_path: Some(module_path.to_string()),
                            impl_kind: Some(if item_impl.trait_.is_some() {
                                "trait"
                            } else {
                                "inherent"
                            }),
                            impl_trait: trait_path.clone(),
                            attributes: parse_lint_attributes(&method.attrs)?,
                        });
                        builder.add_edge("declares", owner_node_id.clone(), method_id.clone());
                        builder.add_edge("contains", impl_node_id.clone(), method_id.clone());
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
    visibility: ItemVisibility,
    attributes: Vec<LintAttribute>,
) -> String {
    let id = format!("{parent_module_id}::{ident}");
    builder.add_node(GraphNode {
        id: id.clone(),
        kind,
        label: ident.to_string(),
        visibility: Some(visibility.as_str()),
        package: context.package_name.clone(),
        target: Some(context.target_name.clone()),
        manifest_path: context.manifest_path.clone(),
        source_path: Some(source_path.display().to_string()),
        module_path: Some(module_path.to_string()),
        impl_kind: None,
        impl_trait: None,
        attributes,
    });
    builder.add_edge("contains", parent_module_id.to_string(), id.clone());
    id
}

fn add_field_nodes(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_id: &str,
    module_path: &str,
    source_path: &Path,
    local_owner_names: &BTreeSet<String>,
    owner_name: Option<&str>,
    fields: &syn::Fields,
) {
    match fields {
        syn::Fields::Named(named) => {
            for field in &named.named {
                let label = field
                    .ident
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "field".to_string());
                let field_id = format!("{parent_id}::field::{label}");
                builder.add_node(GraphNode {
                    id: field_id.clone(),
                    kind: "field",
                    label: label.clone(),
                    visibility: Some(visibility_label(&field.vis).as_str()),
                    package: context.package_name.clone(),
                    target: Some(context.target_name.clone()),
                    manifest_path: context.manifest_path.clone(),
                    source_path: Some(source_path.display().to_string()),
                    module_path: Some(module_path.to_string()),
                    impl_kind: None,
                    impl_trait: None,
                    attributes: Vec::new(),
                });
                builder.add_edge("contains", parent_id.to_string(), field_id.clone());
                add_reference_edges(
                    builder,
                    &field_id,
                    module_path,
                    collect_references_with(local_owner_names, owner_name, |collector| {
                        collector.visit_type(&field.ty);
                    }),
                );
            }
        }
        syn::Fields::Unnamed(unnamed) => {
            for (index, field) in unnamed.unnamed.iter().enumerate() {
                let label = index.to_string();
                let field_id = format!("{parent_id}::field::{label}");
                builder.add_node(GraphNode {
                    id: field_id.clone(),
                    kind: "field",
                    label: label.clone(),
                    visibility: Some(visibility_label(&field.vis).as_str()),
                    package: context.package_name.clone(),
                    target: Some(context.target_name.clone()),
                    manifest_path: context.manifest_path.clone(),
                    source_path: Some(source_path.display().to_string()),
                    module_path: Some(module_path.to_string()),
                    impl_kind: None,
                    impl_trait: None,
                    attributes: Vec::new(),
                });
                builder.add_edge("contains", parent_id.to_string(), field_id.clone());
                add_reference_edges(
                    builder,
                    &field_id,
                    module_path,
                    collect_references_with(local_owner_names, owner_name, |collector| {
                        collector.visit_type(&field.ty);
                    }),
                );
            }
        }
        syn::Fields::Unit => {}
    }
}

fn ensure_trait_reference_node(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    source_path: &Path,
    module_path: &str,
    trait_node_id: &str,
    trait_label: &str,
) {
    if builder.nodes.iter().any(|node| node.id == trait_node_id) {
        return;
    }

    builder.add_node(GraphNode {
        id: trait_node_id.to_string(),
        kind: "trait_ref",
        label: trait_label.to_string(),
        visibility: None,
        package: context.package_name.clone(),
        target: Some(context.target_name.clone()),
        manifest_path: context.manifest_path.clone(),
        source_path: Some(source_path.display().to_string()),
        module_path: Some(module_path.to_string()),
        impl_kind: None,
        impl_trait: None,
        attributes: Vec::new(),
    });
}

fn add_reference_edges(
    builder: &mut GraphBuilder,
    source_node_id: &str,
    module_path: &str,
    referenced_paths: BTreeSet<CollectedReference>,
) {
    for referenced in referenced_paths {
        let target_node_id =
            resolve_reference_target(source_node_id, module_path, &referenced.path);
        builder.add_edge(
            referenced.kind.edge_kind(),
            source_node_id.to_string(),
            target_node_id.clone(),
        );
        builder.add_edge("references", source_node_id.to_string(), target_node_id);
    }
}

fn resolve_reference_target(
    source_node_id: &str,
    module_path: &str,
    referenced_path: &str,
) -> String {
    let crate_prefix = source_node_id
        .split("::module::")
        .next()
        .unwrap_or(source_node_id);

    if let Some(rest) = referenced_path.strip_prefix("crate::") {
        return format!("{crate_prefix}::module::crate::{rest}");
    }

    if let Some(rest) = referenced_path.strip_prefix("self::") {
        return format!("{crate_prefix}::module::{module_path}::{rest}");
    }

    if referenced_path.starts_with("super::") {
        let mut module_segments: Vec<String> =
            module_path.split("::").map(ToOwned::to_owned).collect();
        let mut rest = referenced_path;
        while let Some(stripped) = rest.strip_prefix("super::") {
            if module_segments.len() > 1 {
                module_segments.pop();
            }
            rest = stripped;
        }
        return format!(
            "{crate_prefix}::module::{}::{rest}",
            module_segments.join("::")
        );
    }

    format!("{crate_prefix}::module::{module_path}::{referenced_path}")
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
                ParsedLintDirective::BoundaryForbidExternalImpls => {
                    parsed.push(LintAttribute {
                        scope: "boundary",
                        name: "forbid_external_impls",
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
        ("boundary", "forbid_external_impls") => {
            Ok(ParsedLintDirective::BoundaryForbidExternalImpls)
        }
        ("boundary", _) => Err(syn::Error::new(
            action.span(),
            format!(
                "unsupported boundary directive `{action_name}`; supported: allow(...), internal_only, forbid_external_impls"
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

fn impl_owner_name(self_ty: &Type) -> Result<String> {
    match self_ty {
        Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .ok_or_else(|| anyhow::anyhow!("impl owner path is missing a terminal segment")),
        _ => anyhow::bail!(
            "unsupported impl owner type `{}`; only path owners are supported",
            self_ty.to_token_stream()
        ),
    }
}

fn path_to_string(path: &syn::Path) -> String {
    path.to_token_stream().to_string().replace(' ', "")
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

fn analyze_cycles(graph: &GraphExport, _rule_filter: Option<&str>) -> Vec<Finding> {
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
        let mut per_source: BTreeMap<String, Vec<&OwnerRefEdge>> = BTreeMap::new();
        for edge in self_refs {
            per_source
                .entry(edge.source_node_id.clone())
                .or_default()
                .push(edge);
        }
        let mut inherent_nodes = BTreeSet::new();
        let mut trait_nodes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

        for (source_node_id, source_edges) in per_source {
            let allow_rule = source_edges.iter().any(|edge| {
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
            if allow_rule {
                continue;
            }

            let is_type_method_self_loop = source_edges.iter().all(|edge| {
                edge.source_kind == "method"
                    && edge.owner_kind == "type"
                    && edge.target_owner_id == edge.source_owner_id
                    && edge.source_impl_kind != Some("trait")
            });
            if is_type_method_self_loop {
                let has_expr_ref = source_edges
                    .iter()
                    .any(|edge| edge.reference_kind == ReferenceKind::Expr);
                let has_type_ref = source_edges
                    .iter()
                    .any(|edge| edge.reference_kind == ReferenceKind::Type);
                if !has_expr_ref || has_type_ref {
                    continue;
                }
                for edge in source_edges {
                    for node_id in &edge.node_ids {
                        inherent_nodes.insert(node_id.clone());
                    }
                }
                continue;
            }

            let is_trait_impl_self_loop = source_edges.iter().all(|edge| {
                edge.source_kind == "method"
                    && edge.owner_kind == "type"
                    && edge.target_owner_id == edge.source_owner_id
                    && edge.source_impl_kind == Some("trait")
            });
            if is_trait_impl_self_loop {
                let trait_name = node_map
                    .get(&source_node_id)
                    .and_then(|node| node.impl_trait.clone())
                    .unwrap_or_else(|| "unknown_trait".to_string());
                if is_conversion_like_trait_impl(&trait_name) {
                    continue;
                }
                let entry = trait_nodes.entry(trait_name).or_default();
                for edge in source_edges {
                    for node_id in &edge.node_ids {
                        entry.insert(node_id.clone());
                    }
                }
            }
        }

        if !inherent_nodes.is_empty() {
            findings.push(Finding {
                rule_id: "SCB-CYCLE-002".to_string(),
                kind: "type_method_self_loop".to_string(),
                message: format!("type/method self-loop on owner {owner_id}"),
                owner_ids: vec![owner_id.clone()],
                node_ids: inherent_nodes.into_iter().collect(),
            });
        }

        for (trait_name, node_ids) in trait_nodes {
            findings.push(Finding {
                rule_id: "SCB-CYCLE-003".to_string(),
                kind: "trait_impl_self_loop".to_string(),
                message: format!("trait-impl self-loop on owner {owner_id} via {trait_name}"),
                owner_ids: vec![owner_id.clone()],
                node_ids: node_ids.into_iter().collect(),
            });
        }
    }

    findings
}

fn is_conversion_like_trait_impl(trait_name: &str) -> bool {
    trait_name.contains("From<")
        || trait_name.contains("TryFrom<")
        || trait_name.ends_with("FromStr")
        || trait_name.ends_with("Default")
        || trait_name.starts_with("Parse")
        || trait_name.contains("Deserialize<")
}

fn analyze_internal_only(graph: &GraphExport) -> Vec<Finding> {
    let node_map: BTreeMap<_, _> = graph
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect();
    let mut findings = Vec::new();

    for node in &graph.nodes {
        let has_internal_only = node
            .attributes
            .iter()
            .any(|attr| attr.scope == "boundary" && attr.name == "internal_only");
        if !has_internal_only {
            continue;
        }

        if node.visibility != Some("private") {
            findings.push(Finding {
                rule_id: "SCB-BOUNDARY-001".to_string(),
                kind: "internal_only_visibility_violation".to_string(),
                message: format!(
                    "internal_only item {} must not be externally visible (visibility={})",
                    node.id,
                    node.visibility.unwrap_or("unknown")
                ),
                owner_ids: vec![node.id.clone()],
                node_ids: vec![node.id.clone()],
            });
        }

        let target_module_path = node.module_path.clone();
        let mut seen_external_sources = BTreeSet::new();
        for edge in graph.edges.iter().filter(|edge| {
            matches!(edge.kind, "references_type" | "references_expr") && edge.to == node.id
        }) {
            let Some(source_node) = node_map.get(&edge.from) else {
                continue;
            };
            if source_node.id == node.id {
                continue;
            }
            if source_node.module_path == target_module_path {
                continue;
            }
            let source_owner_id = owner_id_for_node_id(&source_node.id, source_node.kind)
                .unwrap_or_else(|| source_node.id.clone());
            if !seen_external_sources.insert(source_owner_id.clone()) {
                continue;
            }
            findings.push(Finding {
                rule_id: "SCB-BOUNDARY-002".to_string(),
                kind: "internal_only_external_reference".to_string(),
                message: format!(
                    "internal_only item {} referenced from {}",
                    node.id, source_owner_id
                ),
                owner_ids: vec![node.id.clone()],
                node_ids: vec![source_owner_id, node.id.clone()],
            });
        }
    }

    findings.sort_by(|left, right| {
        left.rule_id
            .cmp(&right.rule_id)
            .then_with(|| left.message.cmp(&right.message))
    });
    findings
}

fn analyze_forbid_external_impls(graph: &GraphExport) -> Vec<Finding> {
    let node_map: BTreeMap<_, _> = graph
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect();
    let mut findings = Vec::new();

    for trait_node in &graph.nodes {
        let has_forbid_external_impls = trait_node
            .attributes
            .iter()
            .any(|attr| attr.scope == "boundary" && attr.name == "forbid_external_impls");
        if !has_forbid_external_impls {
            continue;
        }

        for edge in graph
            .edges
            .iter()
            .filter(|edge| edge.kind == "implements" && edge.to == trait_node.id)
        {
            let Some(impl_node) = node_map.get(&edge.from) else {
                continue;
            };
            if impl_node.module_path == trait_node.module_path {
                continue;
            }
            findings.push(Finding {
                rule_id: "SCB-BOUNDARY-003".to_string(),
                kind: "forbid_external_impls_violation".to_string(),
                message: format!(
                    "trait {} forbids external impls but is implemented from module {}",
                    trait_node.id,
                    impl_node.module_path.as_deref().unwrap_or("unknown_module")
                ),
                owner_ids: vec![trait_node.id.clone()],
                node_ids: vec![impl_node.id.clone(), trait_node.id.clone()],
            });
        }
    }

    findings.sort_by(|left, right| left.message.cmp(&right.message));
    findings
}

fn finding_is_failure(finding: &Finding) -> bool {
    matches!(
        finding.rule_id.as_str(),
        "SCB-CYCLE-001" | "SCB-BOUNDARY-001" | "SCB-BOUNDARY-002" | "SCB-BOUNDARY-003"
    )
}

fn finding_sort_key(finding: &Finding) -> (u8, &str) {
    let severity = if finding_is_failure(finding) { 0 } else { 1 };
    (severity, finding.rule_id.as_str())
}

#[derive(Debug, Clone)]
struct OwnerRefEdge {
    source_owner_id: String,
    target_owner_id: String,
    owner_kind: &'static str,
    source_kind: &'static str,
    source_node_id: String,
    source_impl_kind: Option<&'static str>,
    reference_kind: ReferenceKind,
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

    for edge in graph
        .edges
        .iter()
        .filter(|edge| matches!(edge.kind, "references_type" | "references_expr"))
    {
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
            source_node_id: source_node.id.clone(),
            source_impl_kind: source_node.impl_kind,
            reference_kind: match edge.kind {
                "references_type" => ReferenceKind::Type,
                "references_expr" => ReferenceKind::Expr,
                _ => continue,
            },
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
        "variant" => node_id
            .rsplit_once("::variant::")
            .map(|(parent, _)| parent.to_string()),
        "field" => {
            let (parent, _) = node_id.rsplit_once("::field::")?;
            if let Some((enum_parent, _variant)) = parent.rsplit_once("::variant::") {
                Some(enum_parent.to_string())
            } else {
                Some(parent.to_string())
            }
        }
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

pub fn render_graph_export(graph: &GraphExport, format: GraphOutputFormat) -> String {
    match format {
        GraphOutputFormat::Json => serde_json::to_string_pretty(graph)
            .expect("graph export should always serialize to JSON"),
        GraphOutputFormat::Turtle => render_graph_turtle(graph),
    }
}

fn render_graph_turtle(graph: &GraphExport) -> String {
    let mut lines = vec![
        "@prefix sc: <urn:sc-lint-boundary:predicate:> .".to_string(),
        "@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .".to_string(),
        format!(
            "<urn:sc-lint-boundary:graph> sc:schemaVersion {} .",
            turtle_string_literal(graph.schema_version)
        ),
        "".to_string(),
    ];

    for node in &graph.nodes {
        let subject = node_iri(&node.id);
        lines.push(format!("{subject} rdf:type sc:{} .", node.kind));
        lines.push(format!(
            "{subject} sc:id {} .",
            turtle_string_literal(&node.id)
        ));
        lines.push(format!(
            "{subject} sc:label {} .",
            turtle_string_literal(&node.label)
        ));
        if let Some(visibility) = node.visibility {
            lines.push(format!(
                "{subject} sc:visibility {} .",
                turtle_string_literal(visibility)
            ));
        }
        lines.push(format!(
            "{subject} sc:package {} .",
            turtle_string_literal(&node.package)
        ));
        if let Some(target) = &node.target {
            lines.push(format!(
                "{subject} sc:target {} .",
                turtle_string_literal(target)
            ));
        }
        lines.push(format!(
            "{subject} sc:manifestPath {} .",
            turtle_string_literal(&node.manifest_path)
        ));
        if let Some(source_path) = &node.source_path {
            lines.push(format!(
                "{subject} sc:sourcePath {} .",
                turtle_string_literal(source_path)
            ));
        }
        if let Some(module_path) = &node.module_path {
            lines.push(format!(
                "{subject} sc:modulePath {} .",
                turtle_string_literal(module_path)
            ));
        }
        if let Some(impl_kind) = node.impl_kind {
            lines.push(format!(
                "{subject} sc:implKind {} .",
                turtle_string_literal(impl_kind)
            ));
        }
        if let Some(impl_trait) = &node.impl_trait {
            lines.push(format!(
                "{subject} sc:implTrait {} .",
                turtle_string_literal(impl_trait)
            ));
        }
        for attr in &node.attributes {
            lines.push(format!(
                "{subject} sc:attribute {} .",
                turtle_string_literal(&format!(
                    "{}.{}({})",
                    attr.scope,
                    attr.name,
                    attr.values.join(",")
                ))
            ));
        }
        lines.push(String::new());
    }

    for edge in &graph.edges {
        lines.push(format!(
            "{} sc:{} {} .",
            node_iri(&edge.from),
            edge.kind,
            node_iri(&edge.to)
        ));
    }

    lines.join("\n")
}

fn node_iri(node_id: &str) -> String {
    format!(
        "<urn:sc-lint-boundary:node:{}>",
        hex_encode(node_id.as_bytes())
    )
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn turtle_string_literal(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::AnalyzeOptions;
    use super::ExportGraphOptions;
    use super::GraphExport;
    use super::GraphOutputFormat;
    use super::LintAttribute;
    use super::OutputFormat;
    use super::analyze_workspace;
    use super::export_workspace_graph;
    use super::render_findings_report;
    use super::render_graph_export;

    #[test]
    fn findings_report_text_is_stable() {
        let report = super::FindingsReport {
            tool: "sc-lint-boundary",
            version: "0.1.0",
            schema_version: "0.1.0",
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
            schema_version: "0.1.0",
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        let json = serde_json::to_string(&graph).unwrap();
        assert!(json.contains("\"tool\":\"sc-lint-boundary\""));
        assert!(json.contains("\"version\":\"0.1.0\""));
        assert!(json.contains("\"schema_version\":\"0.1.0\""));
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
                        pub fn helper(&self) -> InlineType { InlineType }
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
        assert!(graph.edges.iter().any(|edge| {
            edge.kind == "declares"
                && edge.from == "crate::example::example::module::crate::inline_mod::InlineType"
                && edge.to
                    == "crate::example::example::module::crate::inline_mod::InlineType::helper"
        }));
        assert!(graph.edges.iter().any(|edge| {
            edge.kind == "references"
                && edge.from
                    == "crate::example::example::module::crate::inline_mod::InlineType::helper"
                && edge.to == "crate::example::example::module::crate::inline_mod::InlineType"
        }));
        assert!(graph.edges.iter().any(|edge| {
            edge.kind == "references_expr"
                && edge.from
                    == "crate::example::example::module::crate::inline_mod::InlineType::helper"
                && edge.to == "crate::example::example::module::crate::inline_mod::InlineType"
        }));
        assert!(!graph.edges.iter().any(|edge| {
            edge.from == "crate::example::example::module::crate::inline_mod::InlineType::helper"
                && edge.to == "crate::example::example::module::crate::inline_mod::self"
        }));
    }

    #[test]
    fn exports_field_and_variant_nodes_for_type_graph() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Wrapper {
                    pub value: Inner,
                }

                pub struct Inner;

                pub enum Choice {
                    Unit,
                    Pair(Inner),
                }
            "#,
        );

        let graph = export_workspace_graph(&ExportGraphOptions {
            root: fixture.root().to_path_buf(),
        })
        .unwrap();

        assert!(graph.nodes.iter().any(|node| {
            node.id == "crate::example::example::module::crate::Wrapper::field::value"
                && node.kind == "field"
                && node.visibility == Some("public")
        }));
        assert!(graph.nodes.iter().any(|node| {
            node.id == "crate::example::example::module::crate::Choice::variant::Pair"
                && node.kind == "variant"
        }));
        assert!(graph.nodes.iter().any(|node| {
            node.id == "crate::example::example::module::crate::Choice::variant::Pair::field::0"
                && node.kind == "field"
        }));
        assert!(graph.edges.iter().any(|edge| {
            edge.kind == "references_type"
                && edge.from == "crate::example::example::module::crate::Wrapper::field::value"
                && edge.to == "crate::example::example::module::crate::Inner"
        }));
        assert!(graph.edges.iter().any(|edge| {
            edge.kind == "contains"
                && edge.from == "crate::example::example::module::crate::Choice::variant::Pair"
                && edge.to
                    == "crate::example::example::module::crate::Choice::variant::Pair::field::0"
        }));
    }

    #[test]
    fn renders_graph_as_turtle() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source("example", "lib.rs", "pub struct Example;");

        let graph = export_workspace_graph(&ExportGraphOptions {
            root: fixture.root().to_path_buf(),
        })
        .unwrap();
        let turtle = render_graph_export(&graph, GraphOutputFormat::Turtle);

        assert!(turtle.contains("@prefix sc: <urn:sc-lint-boundary:predicate:> ."));
        assert!(turtle.contains("rdf:type sc:type ."));
        assert!(turtle.contains("sc:visibility \"public\" ."));
        assert!(turtle.contains("sc:label \"Example\" ."));
        assert!(turtle.contains("sc:schemaVersion \"0.1.0\" ."));
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
    fn rejects_unknown_rule_filter() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source("example", "lib.rs", "pub struct Example;");

        let error = analyze_workspace(&AnalyzeOptions {
            root: fixture.root().to_path_buf(),
            format: OutputFormat::Json,
            rule: Some("unknown".to_string()),
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("unsupported rule filter `unknown`")
        );
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
                    pub fn metric() -> usize {
                        let _ = Loop;
                        1
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
    fn does_not_flag_constructor_factory_self_loop() {
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
        assert!(report.findings.is_empty());
    }

    #[test]
    fn does_not_flag_receiver_only_method_as_self_loop() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Wrapper(String);

                impl Wrapper {
                    pub fn as_str(&self) -> &str {
                        &self.0
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
    fn does_not_flag_signature_only_self_return_as_self_loop() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Loop;

                impl Loop {
                    pub fn placeholder() -> Self {
                        todo!()
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
                    pub fn metric() -> usize {
                        let _ = Loop;
                        1
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
    fn suppresses_type_method_self_loop_when_allowed_on_method() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Loop;

                impl Loop {
                    #[sc_lint(boundary.allow("cycle.type_method_self_loop"))]
                    pub fn metric() -> usize {
                        let _ = Loop;
                        1
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
    fn keeps_unsuppressed_method_flagged_when_other_method_is_allowed() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Loop;

                impl Loop {
                    #[sc_lint(boundary.allow("cycle.type_method_self_loop"))]
                    pub fn allowed() -> usize {
                        let _ = Loop;
                        1
                    }

                    pub fn flagged() -> usize {
                        let _ = Loop;
                        2
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
        assert!(
            report.findings[0]
                .node_ids
                .iter()
                .any(|id| id.ends_with("::flagged"))
        );
        assert!(
            !report.findings[0]
                .node_ids
                .iter()
                .any(|id| id.ends_with("::allowed"))
        );
    }

    #[test]
    fn emits_both_inherent_and_trait_self_loop_findings_for_same_owner() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Loop;

                impl Loop {
                    pub fn metric() -> usize {
                        let _ = Loop;
                        1
                    }
                }

                impl core::fmt::Display for Loop {
                    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                        let _ = Loop;
                        write!(f, "loop")
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
        assert_eq!(report.findings.len(), 2);
        assert!(report.findings.iter().any(|f| f.rule_id == "SCB-CYCLE-002"));
        assert!(report.findings.iter().any(|f| f.rule_id == "SCB-CYCLE-003"));
    }

    #[test]
    fn downgrades_trait_impl_self_loop() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Loop;

                impl core::fmt::Display for Loop {
                    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                        let _ = Loop;
                        write!(f, "loop")
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
        assert_eq!(report.findings[0].rule_id, "SCB-CYCLE-003");
        assert_eq!(report.findings[0].kind, "trait_impl_self_loop");
    }

    #[test]
    fn does_not_flag_conversion_like_trait_impl_self_loop() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Loop;

                impl core::str::FromStr for Loop {
                    type Err = ();

                    fn from_str(_s: &str) -> Result<Self, Self::Err> {
                        Ok(Loop)
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
    fn reports_multi_owner_cycle_across_modules_as_failure() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                mod left;
                mod right;
            "#,
        );
        fixture.write_source(
            "example",
            "left.rs",
            "pub struct Alpha { pub beta: crate::right::Beta }",
        );
        fixture.write_source(
            "example",
            "right.rs",
            "pub struct Beta { pub alpha: crate::left::Alpha }",
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
        assert_eq!(
            report.findings[0].owner_ids,
            vec![
                "crate::example::example::module::crate::left::Alpha".to_string(),
                "crate::example::example::module::crate::right::Beta".to_string(),
            ]
        );
    }

    #[test]
    fn fails_when_internal_only_item_is_public() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                #[sc_lint(boundary.internal_only)]
                pub struct Secret;
            "#,
        );

        let report = analyze_workspace(&AnalyzeOptions {
            root: fixture.root().to_path_buf(),
            format: OutputFormat::Json,
            rule: Some("internal_only".to_string()),
        })
        .unwrap();

        assert_eq!(report.status, "fail");
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].rule_id, "SCB-BOUNDARY-001");
    }

    #[test]
    fn fails_when_internal_only_item_is_referenced_from_other_module() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source("example", "lib.rs", "mod owner; mod user;");
        fixture.write_source(
            "example",
            "owner.rs",
            r#"
                #[sc_lint(boundary.internal_only)]
                struct Secret;
            "#,
        );
        fixture.write_source(
            "example",
            "user.rs",
            r#"
                pub struct Uses(crate::owner::Secret);
            "#,
        );

        let report = analyze_workspace(&AnalyzeOptions {
            root: fixture.root().to_path_buf(),
            format: OutputFormat::Json,
            rule: Some("boundaries".to_string()),
        })
        .unwrap();

        assert_eq!(report.status, "fail");
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].rule_id, "SCB-BOUNDARY-002");
        assert!(report.findings[0].message.contains("crate::owner::Secret"));
    }

    #[test]
    fn allows_internal_only_item_inside_own_module() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                #[sc_lint(boundary.internal_only)]
                struct Secret;

                struct Wrapper(Secret);
            "#,
        );

        let report = analyze_workspace(&AnalyzeOptions {
            root: fixture.root().to_path_buf(),
            format: OutputFormat::Json,
            rule: Some("boundaries".to_string()),
        })
        .unwrap();

        assert_eq!(report.status, "pass");
        assert!(report.findings.is_empty());
    }

    #[test]
    fn fails_when_forbid_external_impls_trait_is_implemented_elsewhere() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source("example", "lib.rs", "mod api; mod impls;");
        fixture.write_source(
            "example",
            "api.rs",
            r#"
                #[sc_lint(boundary.forbid_external_impls)]
                pub trait Tokenize {
                    fn tokenize(&self) -> usize;
                }

                pub struct Thing;
            "#,
        );
        fixture.write_source(
            "example",
            "impls.rs",
            r#"
                impl crate::api::Tokenize for crate::api::Thing {
                    fn tokenize(&self) -> usize {
                        1
                    }
                }
            "#,
        );

        let report = analyze_workspace(&AnalyzeOptions {
            root: fixture.root().to_path_buf(),
            format: OutputFormat::Json,
            rule: Some("boundaries".to_string()),
        })
        .unwrap();

        assert_eq!(report.status, "fail");
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule_id == "SCB-BOUNDARY-003")
        );
    }

    #[test]
    fn allows_forbid_external_impls_trait_impl_in_same_module() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                #[sc_lint(boundary.forbid_external_impls)]
                pub trait Tokenize {
                    fn tokenize(&self) -> usize;
                }

                pub struct Thing;

                impl Tokenize for Thing {
                    fn tokenize(&self) -> usize {
                        1
                    }
                }
            "#,
        );

        let report = analyze_workspace(&AnalyzeOptions {
            root: fixture.root().to_path_buf(),
            format: OutputFormat::Json,
            rule: Some("boundaries".to_string()),
        })
        .unwrap();

        assert_eq!(report.status, "pass");
        assert!(report.findings.is_empty());
    }

    #[test]
    fn does_not_flag_acyclic_chain() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Alpha { pub beta: Beta }
                pub struct Beta { pub value: usize }
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
    fn does_not_flag_cross_module_acyclic_chain() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                mod left;
                mod right;
            "#,
        );
        fixture.write_source(
            "example",
            "left.rs",
            "pub struct Alpha { pub beta: crate::right::Beta }",
        );
        fixture.write_source(
            "example",
            "right.rs",
            "pub struct Beta { pub value: usize }",
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
    fn does_not_flag_newtype_factory_self_loop() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Wrapper(String);

                impl Wrapper {
                    pub fn into_inner(self) -> String {
                        self.0
                    }

                    pub fn from_inner(inner: String) -> Wrapper {
                        Wrapper(inner)
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
    fn resolves_self_prefixed_references() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Alpha;
                pub struct Beta(self::Alpha);
            "#,
        );

        let graph = export_workspace_graph(&ExportGraphOptions {
            root: fixture.root().to_path_buf(),
        })
        .unwrap();

        assert!(graph.edges.iter().any(|edge| {
            edge.kind == "references"
                && edge.from == "crate::example::example::module::crate::Beta"
                && edge.to == "crate::example::example::module::crate::Alpha"
        }));
    }

    #[test]
    fn resolves_super_prefixed_references() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Alpha;
                mod inner;
            "#,
        );
        fixture.write_source("example", "inner.rs", "pub struct Beta(super::Alpha);");

        let graph = export_workspace_graph(&ExportGraphOptions {
            root: fixture.root().to_path_buf(),
        })
        .unwrap();

        assert!(graph.edges.iter().any(|edge| {
            edge.kind == "references"
                && edge.from == "crate::example::example::module::crate::inner::Beta"
                && edge.to == "crate::example::example::module::crate::Alpha"
        }));
    }

    #[test]
    fn does_not_promote_function_owned_references_into_module_cycles() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                mod left;
                mod right;
            "#,
        );
        fixture.write_source(
            "example",
            "left.rs",
            "pub fn use_right() -> crate::right::Beta { todo!() }\npub struct Alpha;",
        );
        fixture.write_source(
            "example",
            "right.rs",
            "pub fn use_left() -> crate::left::Alpha { todo!() }\npub struct Beta;",
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
    fn preserves_full_trait_path_in_trait_impl_self_loop_messages() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                mod one { pub trait Display { fn render(&self); } }
                pub struct Loop;

                impl one::Display for Loop {
                    fn render(&self) {
                        let _ = Loop;
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

        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].rule_id, "SCB-CYCLE-003");
        assert!(report.findings[0].message.contains("one::Display"));
    }

    #[test]
    fn rejects_non_path_impl_owners() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            r#"
                pub struct Loop;

                impl core::fmt::Display for &Loop {
                    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                        write!(f, "loop")
                    }
                }
            "#,
        );

        let error = export_workspace_graph(&ExportGraphOptions {
            root: fixture.root().to_path_buf(),
        })
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("unsupported impl owner type"));
        assert!(message.contains("&Loop") || message.contains("& Loop"));
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

    #[test]
    fn fails_when_sc_lint_attribute_has_no_allow_args() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            "#[sc_lint(boundary.allow())] pub struct Example;",
        );

        let error = export_workspace_graph(&ExportGraphOptions {
            root: fixture.root().to_path_buf(),
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("boundary.allow requires at least one rule id string")
        );
    }

    #[test]
    fn fails_when_sc_lint_attribute_uses_unknown_boundary_directive() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            "#[sc_lint(boundary.unknown(\"x\"))] pub struct Example;",
        );

        let error = export_workspace_graph(&ExportGraphOptions {
            root: fixture.root().to_path_buf(),
        })
        .unwrap_err();

        assert!(error.to_string().contains("unsupported boundary directive"));
    }

    #[test]
    fn fails_when_sc_lint_attribute_uses_unknown_scope() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            "#[sc_lint(other.internal_only)] pub struct Example;",
        );

        let error = export_workspace_graph(&ExportGraphOptions {
            root: fixture.root().to_path_buf(),
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("unsupported sc_lint scope `other`")
        );
    }

    #[test]
    fn fails_when_sc_lint_attribute_has_mixed_valid_and_invalid_directives() {
        let fixture = WorkspaceFixture::new();
        fixture.write_workspace_root();
        fixture.write_package_manifest("example");
        fixture.write_source(
            "example",
            "lib.rs",
            "#[sc_lint(boundary.internal_only, boundary.unknown(\"x\"))] pub struct Example;",
        );

        let error = export_workspace_graph(&ExportGraphOptions {
            root: fixture.root().to_path_buf(),
        })
        .unwrap_err();

        assert!(error.to_string().contains("unsupported boundary directive"));
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
