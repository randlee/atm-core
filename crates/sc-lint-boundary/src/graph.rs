use super::*;
use crate::render::hex_encode;

mod ingest;
mod reference_collector;

use self::ingest::ingest_module_items;
use self::reference_collector::collect_references_with;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    Module,
    Type,
    Trait,
    Function,
    Method,
    Impl,
    Variant,
    Field,
    TraitRef,
}

impl NodeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Type => "type",
            Self::Trait => "trait",
            Self::Function => "function",
            Self::Method => "method",
            Self::Impl => "impl",
            Self::Variant => "variant",
            Self::Field => "field",
            Self::TraitRef => "trait_ref",
        }
    }
}

pub(crate) fn build_workspace_graph(root: &Path) -> Result<GraphExport> {
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
                id: NodeId::new(root_module_id.clone()),
                kind: NodeKind::Module.as_str(),
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
                &NodeId::new(root_module_id.clone()),
                "crate",
                &root_dir,
                &source_path,
                parse_rust_file(&source_path)?,
            )?;
        }
    }

    Ok(builder.finish())
}

fn ensure_trait_reference_node(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    source_path: &Path,
    module_path: &str,
    trait_node_id: &NodeId,
    trait_label: &str,
) {
    if builder
        .nodes
        .iter()
        .any(|node| node.id == trait_node_id.as_str())
    {
        return;
    }

    builder.add_node(GraphNode {
        id: trait_node_id.clone(),
        kind: NodeKind::TraitRef.as_str(),
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
    source_node_id: &NodeId,
    module_path: &str,
    referenced_paths: BTreeSet<CollectedReference>,
) {
    for referenced in referenced_paths {
        let target_node_id =
            resolve_reference_target(source_node_id, module_path, &referenced.path);
        let target_node_id = if referenced.kind == ReferenceKind::Expr {
            resolve_existing_expression_target(builder, target_node_id)
        } else {
            target_node_id
        };
        builder.add_reference_edge(
            referenced.kind.edge_kind(),
            source_node_id.clone(),
            target_node_id.clone(),
            referenced.call_callee,
        );
        builder.add_edge("references", source_node_id.clone(), target_node_id);
    }
}

/// Associated item paths resolve at their enclosing concrete owner when the
/// graph does not model the item itself. Methods are impl-qualified nodes, so
/// `Self::helper` resolves to its enclosing type. Do not fall back to a module
/// or crate target:
/// module ingestion is ordered, and reducing a forward `crate::send::write()`
/// reference to `crate` would permanently erase its cross-module edge.
fn resolve_existing_expression_target(builder: &GraphBuilder, target_node_id: NodeId) -> NodeId {
    let mut candidate = target_node_id.clone();
    loop {
        if let Some((parent, label)) = candidate.rsplit_once("::") {
            let variant = NodeId::new(format!("{parent}::variant::{label}"));
            if builder.nodes.iter().any(|node| node.id == variant) {
                return variant;
            }
        }
        if builder
            .nodes
            .iter()
            .any(|node| node.id == candidate && !matches!(node.kind, "module" | "crate"))
        {
            return candidate;
        }
        let Some((parent, _)) = candidate.rsplit_once("::") else {
            return target_node_id;
        };
        candidate = NodeId::new(parent);
    }
}

fn resolve_reference_target(
    source_node_id: &NodeId,
    module_path: &str,
    referenced_path: &str,
) -> NodeId {
    let crate_prefix = source_node_id
        .split("::module::")
        .next()
        .unwrap_or(source_node_id);

    if let Some(rest) = referenced_path.strip_prefix("crate::") {
        return NodeId::new(format!("{crate_prefix}::module::crate::{rest}"));
    }

    if let Some(rest) = referenced_path.strip_prefix("self::") {
        return NodeId::new(format!("{crate_prefix}::module::{module_path}::{rest}"));
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
        return NodeId::new(format!(
            "{crate_prefix}::module::{}::{rest}",
            module_segments.join("::")
        ));
    }

    NodeId::new(format!(
        "{crate_prefix}::module::{module_path}::{referenced_path}"
    ))
}

fn parse_lint_attributes(attrs: &[Attribute]) -> Result<Vec<LintAttribute>> {
    let mut parsed = Vec::new();

    for attr in attrs {
        if !attr.path().is_ident("sc_lint") {
            continue;
        }
        let input = attr.parse_args::<AttributeInput>()?;
        for directive in input.directives {
            match directive {
                Directive::Allow(values) => {
                    parsed.push(LintAttribute {
                        scope: "boundary",
                        name: "allow",
                        values,
                    });
                }
                Directive::InternalOnly => {
                    parsed.push(LintAttribute {
                        scope: "boundary",
                        name: "internal_only",
                        values: Vec::new(),
                    });
                }
                Directive::ForbidExternalImpls => {
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

pub(crate) fn node_has_allow_rule(node: &GraphNode, rule_id: &str) -> bool {
    node.attributes.iter().any(|attr| {
        attr.scope == "boundary"
            && attr.name == "allow"
            && attr.values.iter().any(|value| value == rule_id)
    })
}

fn resolve_module_source(
    declaring_source_path: &Path,
    module_dir: &Path,
    module_name: &str,
    attrs: &[Attribute],
) -> Result<PathBuf> {
    if let Some(explicit_path) = explicit_module_source(declaring_source_path, attrs)? {
        if explicit_path.is_file() {
            return Ok(explicit_path);
        }
        anyhow::bail!(
            "module `{module_name}` path attribute resolved to missing file {}",
            explicit_path.display()
        );
    }

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

fn has_explicit_module_path(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("path"))
}

fn explicit_module_source(
    declaring_source_path: &Path,
    attrs: &[Attribute],
) -> Result<Option<PathBuf>> {
    for attr in attrs {
        if !attr.path().is_ident("path") {
            continue;
        }

        match &attr.meta {
            syn::Meta::NameValue(name_value) => match &name_value.value {
                syn::Expr::Lit(expr_lit) => match &expr_lit.lit {
                    syn::Lit::Str(lit) => {
                        let declaring_dir = declaring_source_path.parent().ok_or_else(|| {
                            anyhow::anyhow!(
                                "declaring source path has no parent: {}",
                                declaring_source_path.display()
                            )
                        })?;
                        // Absolute #[path = "..."] values intentionally bypass the
                        // declaring source directory because PathBuf::join preserves
                        // an absolute right-hand operand unchanged.
                        return Ok(Some(declaring_dir.join(lit.value())));
                    }
                    _ => anyhow::bail!(
                        "path attribute must use a string literal: {}",
                        attr.to_token_stream()
                    ),
                },
                _ => anyhow::bail!(
                    "path attribute must use a string literal: {}",
                    attr.to_token_stream()
                ),
            },
            _ => anyhow::bail!(
                "unsupported path attribute syntax: {}",
                attr.to_token_stream()
            ),
        }
    }

    Ok(None)
}

fn parse_rust_file(path: &Path) -> Result<File> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read Rust source {}", path.display()))?;
    syn::parse_file(&source)
        .with_context(|| format!("failed to parse Rust source {}", path.display()))
}

fn impl_owner_name(self_ty: &Type) -> Result<String> {
    match self_ty {
        Type::Path(type_path) => {
            // `syn::Type::Path` always stores at least one segment for a valid path type.
            if let Some(segment) = type_path.path.segments.last() {
                Ok(segment.ident.to_string())
            } else {
                Err(anyhow::anyhow!(
                    "impl owner path is missing a terminal segment"
                ))
            }
        }
        _ => anyhow::bail!(
            "unsupported impl owner type `{}`; only path owners are supported",
            self_ty.to_token_stream()
        ),
    }
}

pub(crate) fn trait_path_key(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

pub(crate) fn trait_terminal_name(trait_path: &str) -> &str {
    trait_path.rsplit("::").next().unwrap_or(trait_path)
}

pub(crate) fn default_rule_defaults() -> &'static RuleDefaults {
    static DEFAULTS: OnceLock<RuleDefaults> = OnceLock::new();
    DEFAULTS.get_or_init(|| {
        toml::from_str(DEFAULT_RULES_TOML)
            .expect("embedded sc-lint-boundary default rule config must parse")
    })
}

pub(crate) fn is_supported_target(target: &cargo_metadata::Target) -> bool {
    target.kind.iter().any(|kind| {
        matches!(
            kind,
            cargo_metadata::TargetKind::Lib
                | cargo_metadata::TargetKind::Bin
                | cargo_metadata::TargetKind::Example
        )
    })
}

pub(crate) fn crate_id(package_name: &str, target_name: &str) -> String {
    format!("crate::{package_name}::{target_name}")
}

pub(crate) fn load_metadata(root: &Path) -> Result<cargo_metadata::Metadata> {
    MetadataCommand::new()
        .current_dir(root)
        .exec()
        .with_context(|| format!("failed to load cargo metadata for {}", root.display()))
}
