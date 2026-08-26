use super::*;

pub(super) fn ingest_module_items(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_module_id: &NodeId,
    module_path: &str,
    module_dir: &Path,
    source_path: &Path,
    file: File,
) -> Result<()> {
    let local_owner_names = collect_owner_names(&file.items);
    let args = IngestModuleItemArgs {
        parent_module_id,
        module_path,
        module_dir,
        source_path,
        local_owner_names: &local_owner_names,
    };

    for item in file.items {
        ingest_module_item(builder, context, &args, item)?;
    }

    Ok(())
}

struct IngestModuleItemArgs<'a> {
    parent_module_id: &'a NodeId,
    module_path: &'a str,
    module_dir: &'a Path,
    source_path: &'a Path,
    local_owner_names: &'a BTreeSet<String>,
}

fn ingest_module_item(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    args: &IngestModuleItemArgs<'_>,
    item: Item,
) -> Result<()> {
    match item {
        Item::Mod(item_mod) => handle_mod_item(
            builder,
            context,
            args.parent_module_id,
            args.module_path,
            args.module_dir,
            args.source_path,
            item_mod,
        ),
        other => dispatch_non_mod_item(
            builder,
            context,
            args.parent_module_id,
            args.module_path,
            args.source_path,
            args.local_owner_names,
            other,
        ),
    }
}

fn dispatch_non_mod_item(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_module_id: &NodeId,
    module_path: &str,
    source_path: &Path,
    local_owner_names: &BTreeSet<String>,
    item: Item,
) -> Result<()> {
    match item {
        Item::Struct(item_struct) => handle_struct_item(
            builder,
            context,
            parent_module_id,
            module_path,
            source_path,
            local_owner_names,
            item_struct,
        ),
        Item::Enum(item_enum) => handle_enum_item(
            builder,
            context,
            parent_module_id,
            module_path,
            source_path,
            local_owner_names,
            item_enum,
        ),
        Item::Union(item_union) => handle_union_item(
            builder,
            context,
            parent_module_id,
            module_path,
            source_path,
            local_owner_names,
            item_union,
        ),
        Item::Type(item_type) => handle_type_item(
            builder,
            context,
            parent_module_id,
            module_path,
            source_path,
            local_owner_names,
            item_type,
        ),
        Item::Trait(item_trait) => handle_trait_item(
            builder,
            context,
            parent_module_id,
            module_path,
            source_path,
            local_owner_names,
            item_trait,
        ),
        Item::Fn(item_fn) => handle_fn_item(
            builder,
            context,
            parent_module_id,
            module_path,
            source_path,
            local_owner_names,
            item_fn,
        ),
        Item::Impl(item_impl) => handle_impl_item(
            builder,
            context,
            parent_module_id,
            module_path,
            source_path,
            local_owner_names,
            item_impl,
        ),
        _ => Ok(()),
    }
}

fn handle_mod_item(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_module_id: &NodeId,
    module_path: &str,
    module_dir: &Path,
    source_path: &Path,
    item_mod: syn::ItemMod,
) -> Result<()> {
    let name = item_mod.ident.to_string();
    let child_module_path = format!("{module_path}::{name}");
    let child_module_id = format!("{}::module::{child_module_path}", context.crate_id);
    let child_node_id = NodeId::new(child_module_id.clone());
    let attributes = parse_lint_attributes(&item_mod.attrs)?;
    builder.add_node(GraphNode {
        id: child_node_id.clone(),
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
        let inline_file = File {
            shebang: None,
            attrs: Vec::new(),
            items,
        };
        return ingest_module_items(
            builder,
            context,
            &child_node_id,
            &child_module_path,
            &module_dir.join(&name),
            source_path,
            inline_file,
        );
    }
    let child_source_path = resolve_module_source(source_path, module_dir, &name, &item_mod.attrs)
        .with_context(|| format!("while resolving module `{child_module_path}`"))?;
    let child_module_dir =
        resolved_child_module_dir(module_dir, &name, &item_mod.attrs, &child_source_path);
    ingest_module_items(
        builder,
        context,
        &child_node_id,
        &child_module_path,
        &child_module_dir,
        &child_source_path,
        parse_rust_file(&child_source_path)?,
    )
}

fn resolved_child_module_dir(
    module_dir: &Path,
    name: &str,
    attrs: &[Attribute],
    child_source_path: &Path,
) -> PathBuf {
    if has_explicit_module_path(attrs)
        || child_source_path.file_name().and_then(|name| name.to_str()) == Some("mod.rs")
    {
        child_source_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| module_dir.join(name))
    } else {
        module_dir.join(name)
    }
}

fn handle_struct_item(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_module_id: &NodeId,
    module_path: &str,
    source_path: &Path,
    local_owner_names: &BTreeSet<String>,
    item_struct: syn::ItemStruct,
) -> Result<()> {
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
        collect_references_with(local_owner_names, Some(&owner_name), |collector| {
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
            local_owner_names,
            owner_name: Some(&owner_name),
            fields: &item_struct.fields,
        },
    );
    Ok(())
}

fn handle_enum_item(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_module_id: &NodeId,
    module_path: &str,
    source_path: &Path,
    local_owner_names: &BTreeSet<String>,
    item_enum: syn::ItemEnum,
) -> Result<()> {
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
        collect_references_with(local_owner_names, Some(&owner_name), |collector| {
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
                local_owner_names,
                owner_name: Some(&owner_name),
                fields: &variant.fields,
            },
        );
    }
    Ok(())
}

fn handle_union_item(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_module_id: &NodeId,
    module_path: &str,
    source_path: &Path,
    local_owner_names: &BTreeSet<String>,
    item_union: syn::ItemUnion,
) -> Result<()> {
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
        collect_references_with(local_owner_names, Some(&owner_name), |collector| {
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
            local_owner_names,
            owner_name: Some(&owner_name),
            fields: &union_fields,
        },
    );
    Ok(())
}

fn handle_type_item(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_module_id: &NodeId,
    module_path: &str,
    source_path: &Path,
    local_owner_names: &BTreeSet<String>,
    item_type: syn::ItemType,
) -> Result<()> {
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
        collect_references_with(local_owner_names, Some(&owner_name), |collector| {
            collector.visit_type(&item_type.ty);
        }),
    );
    Ok(())
}

fn handle_trait_item(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_module_id: &NodeId,
    module_path: &str,
    source_path: &Path,
    local_owner_names: &BTreeSet<String>,
    item_trait: syn::ItemTrait,
) -> Result<()> {
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
        collect_references_with(local_owner_names, Some(&owner_name), |collector| {
            for trait_item in &item_trait.items {
                collector.visit_trait_item(trait_item);
            }
        }),
    );
    Ok(())
}

fn handle_fn_item(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_module_id: &NodeId,
    module_path: &str,
    source_path: &Path,
    local_owner_names: &BTreeSet<String>,
    item_fn: syn::ItemFn,
) -> Result<()> {
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
        collect_references_with(local_owner_names, None, |collector| {
            collector.visit_item_fn(&item_fn);
        }),
    );
    Ok(())
}

fn handle_impl_item(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_module_id: &NodeId,
    module_path: &str,
    source_path: &Path,
    local_owner_names: &BTreeSet<String>,
    item_impl: syn::ItemImpl,
) -> Result<()> {
    let owner_name = impl_owner_name(&item_impl.self_ty)?;
    let owner_node_id = ensure_impl_owner_node(
        builder,
        context,
        parent_module_id,
        module_path,
        source_path,
        &owner_name,
    );
    let trait_path = item_impl
        .trait_
        .as_ref()
        .map(|(_, path, _)| trait_path_key(path));
    let impl_node_id = add_impl_node(
        builder,
        context,
        ImplNodeArgs {
            parent_module_id,
            module_path,
            source_path,
            owner_node_id: &owner_node_id,
            owner_name: &owner_name,
            trait_path: trait_path.clone(),
        },
    );
    if let Some((_, path, _)) = &item_impl.trait_ {
        add_impl_trait_edge(
            builder,
            context,
            module_path,
            source_path,
            &impl_node_id,
            path,
        );
    }
    for impl_item in item_impl.items {
        if let ImplItem::Fn(method) = impl_item {
            add_impl_method_node(
                builder,
                context,
                ImplMethodNodeArgs {
                    module_path,
                    source_path,
                    local_owner_names,
                    owner_name: &owner_name,
                    owner_node_id: &owner_node_id,
                    impl_node_id: &impl_node_id,
                    trait_path: trait_path.clone(),
                },
                method,
            )?;
        }
    }
    Ok(())
}

fn ensure_impl_owner_node(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    parent_module_id: &NodeId,
    module_path: &str,
    source_path: &Path,
    owner_name: &str,
) -> NodeId {
    let owner_node_id = NodeId::new(format!("{parent_module_id}::{owner_name}"));
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
    owner_node_id
}

struct ImplNodeArgs<'a> {
    parent_module_id: &'a NodeId,
    module_path: &'a str,
    source_path: &'a Path,
    owner_node_id: &'a NodeId,
    owner_name: &'a str,
    trait_path: Option<String>,
}

fn add_impl_node(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    args: ImplNodeArgs<'_>,
) -> NodeId {
    let impl_node_id = if let Some(trait_path) = &args.trait_path {
        NodeId::new(format!(
            "{}::impl::{}",
            args.owner_node_id,
            hex_encode(trait_path.as_bytes())
        ))
    } else {
        NodeId::new(format!("{}::impl::inherent", args.owner_node_id))
    };
    builder.add_node(GraphNode {
        id: impl_node_id.clone(),
        kind: NodeKind::Impl.as_str(),
        label: args
            .trait_path
            .as_ref()
            .map(|path| format!("impl {path} for {}", args.owner_name))
            .unwrap_or_else(|| format!("impl {}", args.owner_name)),
        visibility: None,
        package: context.package_name.clone(),
        target: Some(context.target_name.clone()),
        manifest_path: context.manifest_path.clone(),
        source_path: Some(args.source_path.display().to_string()),
        module_path: Some(args.module_path.to_string()),
        impl_kind: Some(if args.trait_path.is_some() {
            ImplKind::Trait
        } else {
            ImplKind::Inherent
        }),
        impl_trait: args.trait_path,
        attributes: Vec::new(),
    });
    builder.add_edge(
        "contains",
        args.parent_module_id.clone(),
        impl_node_id.clone(),
    );
    builder.add_edge("targets", impl_node_id.clone(), args.owner_node_id.clone());
    impl_node_id
}

fn add_impl_trait_edge(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    module_path: &str,
    source_path: &Path,
    impl_node_id: &NodeId,
    path: &syn::Path,
) {
    let trait_reference_path = trait_path_key(path);
    let trait_target_node_id =
        resolve_reference_target(impl_node_id, module_path, &trait_reference_path);
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

struct ImplMethodNodeArgs<'a> {
    module_path: &'a str,
    source_path: &'a Path,
    local_owner_names: &'a BTreeSet<String>,
    owner_name: &'a str,
    owner_node_id: &'a NodeId,
    impl_node_id: &'a NodeId,
    trait_path: Option<String>,
}

fn add_impl_method_node(
    builder: &mut GraphBuilder,
    context: &TargetContext,
    args: ImplMethodNodeArgs<'_>,
    method: syn::ImplItemFn,
) -> Result<()> {
    let method_id = NodeId::new(format!("{}::{}", args.impl_node_id, method.sig.ident));
    builder.add_node(GraphNode {
        id: method_id.clone(),
        kind: NodeKind::Method.as_str(),
        label: method.sig.ident.to_string(),
        visibility: Some(visibility_label(&method.vis).as_str()),
        package: context.package_name.clone(),
        target: Some(context.target_name.clone()),
        manifest_path: context.manifest_path.clone(),
        source_path: Some(args.source_path.display().to_string()),
        module_path: Some(args.module_path.to_string()),
        impl_kind: Some(if args.trait_path.is_some() {
            ImplKind::Trait
        } else {
            ImplKind::Inherent
        }),
        impl_trait: args.trait_path,
        attributes: parse_lint_attributes(&method.attrs)?,
    });
    builder.add_edge("declares", args.owner_node_id.clone(), method_id.clone());
    builder.add_edge("contains", args.impl_node_id.clone(), method_id.clone());
    add_reference_edges(
        builder,
        &method_id,
        args.module_path,
        collect_references_with(args.local_owner_names, Some(args.owner_name), |collector| {
            collector.visit_impl_item_fn(&method);
        }),
    );
    Ok(())
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

struct FieldNodeArgs<'a> {
    parent_id: &'a NodeId,
    module_path: &'a str,
    source_path: &'a Path,
    local_owner_names: &'a BTreeSet<String>,
    owner_name: Option<&'a str>,
    fields: &'a syn::Fields,
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
