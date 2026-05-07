use super::*;
use crate::render::hex_encode;

mod reference_collector;

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

fn ingest_module_items(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_module_id: &NodeId,
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
                let attributes = parse_lint_attributes(&item_mod.attrs)?;

                builder.add_node(GraphNode {
                    id: NodeId::new(child_module_id.clone()),
                    kind: NodeKind::Module.as_str(),
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
                    parent_module_id.clone(),
                    child_module_id.clone(),
                );

                if let Some((_, items)) = item_mod.content {
                    let child_module_dir = module_dir.join(&name);
                    let inline_file = File {
                        shebang: None,
                        attrs: Vec::new(),
                        items,
                    };
                    ingest_module_items(
                        builder,
                        context,
                        &NodeId::new(child_module_id.clone()),
                        &child_module_path,
                        &child_module_dir,
                        source_path,
                        inline_file,
                    )?;
                } else {
                    let child_source_path =
                        resolve_module_source(source_path, module_dir, &name, &item_mod.attrs)
                            .with_context(|| {
                                format!("while resolving module `{child_module_path}`")
                            })?;
                    let child_module_dir = if has_explicit_module_path(&item_mod.attrs) {
                        // #[path = "..."] points at the real child source file, so the child
                        // module directory must be derived from that resolved location.
                        child_source_path
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_else(|| module_dir.join(&name))
                    } else if child_source_path.file_name().and_then(|name| name.to_str())
                        == Some("mod.rs")
                    {
                        // mod.rs is itself the child module root, so sibling lookups should
                        // start from the file's parent directory.
                        child_source_path
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_else(|| module_dir.join(&name))
                    } else {
                        module_dir.join(&name)
                    };
                    let child_file = parse_rust_file(&child_source_path)?;
                    ingest_module_items(
                        builder,
                        context,
                        &NodeId::new(child_module_id.clone()),
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
                    ItemNodeArgs {
                        parent_module_id,
                        module_path,
                        source_path,
                        ident: &item_struct.ident,
                        kind: NodeKind::Type,
                        visibility: visibility_label(&item_struct.vis),
                        attributes: parse_lint_attributes(&item_struct.attrs)?,
                    },
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
                    FieldNodeArgs {
                        parent_id: &node_id,
                        module_path,
                        source_path,
                        local_owner_names: &local_owner_names,
                        owner_name: Some(&owner_name),
                        fields: &item_struct.fields,
                    },
                );
            }
            Item::Enum(item_enum) => {
                let owner_name = item_enum.ident.to_string();
                let node_id = add_item_node(
                    builder,
                    context,
                    ItemNodeArgs {
                        parent_module_id,
                        module_path,
                        source_path,
                        ident: &item_enum.ident,
                        kind: NodeKind::Type,
                        visibility: visibility_label(&item_enum.vis),
                        attributes: parse_lint_attributes(&item_enum.attrs)?,
                    },
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
                    let variant_id = NodeId::new(format!("{node_id}::variant::{}", variant.ident));
                    builder.add_node(GraphNode {
                        id: variant_id.clone(),
                        kind: NodeKind::Variant.as_str(),
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
                        FieldNodeArgs {
                            parent_id: &variant_id,
                            module_path,
                            source_path,
                            local_owner_names: &local_owner_names,
                            owner_name: Some(&owner_name),
                            fields: &variant.fields,
                        },
                    );
                }
            }
            Item::Union(item_union) => {
                let owner_name = item_union.ident.to_string();
                let node_id = add_item_node(
                    builder,
                    context,
                    ItemNodeArgs {
                        parent_module_id,
                        module_path,
                        source_path,
                        ident: &item_union.ident,
                        kind: NodeKind::Type,
                        visibility: visibility_label(&item_union.vis),
                        attributes: parse_lint_attributes(&item_union.attrs)?,
                    },
                );
                add_reference_edges(
                    builder,
                    &node_id,
                    module_path,
                    collect_references_with(&local_owner_names, Some(&owner_name), |collector| {
                        collector.visit_fields_named(&item_union.fields);
                    }),
                );
                let union_fields = syn::Fields::Named(item_union.fields.clone());
                add_field_nodes(
                    builder,
                    context,
                    FieldNodeArgs {
                        parent_id: &node_id,
                        module_path,
                        source_path,
                        local_owner_names: &local_owner_names,
                        owner_name: Some(&owner_name),
                        fields: &union_fields,
                    },
                );
            }
            Item::Type(item_type) => {
                let owner_name = item_type.ident.to_string();
                let node_id = add_item_node(
                    builder,
                    context,
                    ItemNodeArgs {
                        parent_module_id,
                        module_path,
                        source_path,
                        ident: &item_type.ident,
                        kind: NodeKind::Type,
                        visibility: visibility_label(&item_type.vis),
                        attributes: parse_lint_attributes(&item_type.attrs)?,
                    },
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
                    ItemNodeArgs {
                        parent_module_id,
                        module_path,
                        source_path,
                        ident: &item_trait.ident,
                        kind: NodeKind::Trait,
                        visibility: visibility_label(&item_trait.vis),
                        attributes: parse_lint_attributes(&item_trait.attrs)?,
                    },
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
                    ItemNodeArgs {
                        parent_module_id,
                        module_path,
                        source_path,
                        ident: &function_ident,
                        kind: NodeKind::Function,
                        visibility: visibility_label(&item_fn.vis),
                        attributes: parse_lint_attributes(&item_fn.attrs)?,
                    },
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
                let owner_node_id = NodeId::new(format!("{parent_module_id}::{owner_name}"));
                let trait_path = item_impl
                    .trait_
                    .as_ref()
                    .map(|(_, path, _)| trait_path_key(path));
                let impl_node_id = if let Some(trait_path) = &trait_path {
                    NodeId::new(format!(
                        "{owner_node_id}::impl::{}",
                        hex_encode(trait_path.as_bytes())
                    ))
                } else {
                    NodeId::new(format!("{owner_node_id}::impl::inherent"))
                };

                if !builder
                    .nodes
                    .iter()
                    .any(|node| node.id == owner_node_id.as_str())
                {
                    builder.add_node(GraphNode {
                        id: owner_node_id.clone(),
                        kind: NodeKind::Type.as_str(),
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
                    builder.add_edge("contains", parent_module_id.clone(), owner_node_id.clone());
                }

                builder.add_node(GraphNode {
                    id: impl_node_id.clone(),
                    kind: NodeKind::Impl.as_str(),
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
                        ImplKind::Trait
                    } else {
                        ImplKind::Inherent
                    }),
                    impl_trait: trait_path.clone(),
                    attributes: Vec::new(),
                });
                builder.add_edge("contains", parent_module_id.clone(), impl_node_id.clone());
                builder.add_edge("targets", impl_node_id.clone(), owner_node_id.clone());

                if let Some((_, path, _)) = &item_impl.trait_ {
                    let trait_reference_path = trait_path_key(path);
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
                        let method_id =
                            NodeId::new(format!("{owner_node_id}::{}", method.sig.ident));
                        builder.add_node(GraphNode {
                            id: method_id.clone(),
                            kind: NodeKind::Method.as_str(),
                            label: method.sig.ident.to_string(),
                            visibility: Some(visibility_label(&method.vis).as_str()),
                            package: context.package_name.clone(),
                            target: Some(context.target_name.clone()),
                            manifest_path: context.manifest_path.clone(),
                            source_path: Some(source_path.display().to_string()),
                            module_path: Some(module_path.to_string()),
                            impl_kind: Some(if item_impl.trait_.is_some() {
                                ImplKind::Trait
                            } else {
                                ImplKind::Inherent
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
    args: ItemNodeArgs<'_>,
) -> NodeId {
    let id = format!("{}::{}", args.parent_module_id, args.ident);
    builder.add_node(GraphNode {
        id: NodeId::new(id.clone()),
        kind: args.kind.as_str(),
        label: args.ident.to_string(),
        visibility: Some(args.visibility.as_str()),
        package: context.package_name.clone(),
        target: Some(context.target_name.clone()),
        manifest_path: context.manifest_path.clone(),
        source_path: Some(args.source_path.display().to_string()),
        module_path: Some(args.module_path.to_string()),
        impl_kind: None,
        impl_trait: None,
        attributes: args.attributes,
    });
    builder.add_edge(
        "contains",
        args.parent_module_id.clone(),
        NodeId::new(id.clone()),
    );
    NodeId::new(id)
}

fn add_field_nodes(builder: &mut GraphBuilder, context: &TargetContext, args: FieldNodeArgs<'_>) {
    match args.fields {
        syn::Fields::Named(named) => {
            for field in &named.named {
                let label = field
                    .ident
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "field".to_string());
                let field_id = format!("{}::field::{label}", args.parent_id);
                let field_id = NodeId::new(field_id);
                builder.add_node(GraphNode {
                    id: field_id.clone(),
                    kind: NodeKind::Field.as_str(),
                    label: label.clone(),
                    visibility: Some(visibility_label(&field.vis).as_str()),
                    package: context.package_name.clone(),
                    target: Some(context.target_name.clone()),
                    manifest_path: context.manifest_path.clone(),
                    source_path: Some(args.source_path.display().to_string()),
                    module_path: Some(args.module_path.to_string()),
                    impl_kind: None,
                    impl_trait: None,
                    attributes: Vec::new(),
                });
                builder.add_edge("contains", args.parent_id.clone(), field_id.clone());
                add_reference_edges(
                    builder,
                    &field_id,
                    args.module_path,
                    collect_references_with(args.local_owner_names, args.owner_name, |collector| {
                        collector.visit_type(&field.ty);
                    }),
                );
            }
        }
        syn::Fields::Unnamed(unnamed) => {
            for (index, field) in unnamed.unnamed.iter().enumerate() {
                let label = index.to_string();
                let field_id = format!("{}::field::{label}", args.parent_id);
                let field_id = NodeId::new(field_id);
                builder.add_node(GraphNode {
                    id: field_id.clone(),
                    kind: NodeKind::Field.as_str(),
                    label: label.clone(),
                    visibility: Some(visibility_label(&field.vis).as_str()),
                    package: context.package_name.clone(),
                    target: Some(context.target_name.clone()),
                    manifest_path: context.manifest_path.clone(),
                    source_path: Some(args.source_path.display().to_string()),
                    module_path: Some(args.module_path.to_string()),
                    impl_kind: None,
                    impl_trait: None,
                    attributes: Vec::new(),
                });
                builder.add_edge("contains", args.parent_id.clone(), field_id.clone());
                add_reference_edges(
                    builder,
                    &field_id,
                    args.module_path,
                    collect_references_with(args.local_owner_names, args.owner_name, |collector| {
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
        builder.add_edge(
            referenced.kind.edge_kind(),
            source_node_id.clone(),
            target_node_id.clone(),
        );
        builder.add_edge("references", source_node_id.clone(), target_node_id);
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
struct ItemNodeArgs<'a> {
    parent_module_id: &'a NodeId,
    module_path: &'a str,
    source_path: &'a Path,
    ident: &'a Ident,
    kind: NodeKind,
    visibility: ItemVisibility,
    attributes: Vec<LintAttribute>,
}

struct FieldNodeArgs<'a> {
    parent_id: &'a NodeId,
    module_path: &'a str,
    source_path: &'a Path,
    local_owner_names: &'a BTreeSet<String>,
    owner_name: Option<&'a str>,
    fields: &'a syn::Fields,
}
