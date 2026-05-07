use super::*;

pub(crate) fn analyze_cycles(graph: &GraphExport) -> Vec<Finding> {
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
            if component_allows_recursive_value_container(&component, &node_map) {
                continue;
            }
            let mut owners = component.clone();
            owners.sort();
            findings.push(Finding {
                rule_id: RuleId::ScbCycle001,
                kind: "multi_owner_architectural_cycle".to_string(),
                message: format!(
                    "architectural cycle across owners: {}",
                    owners
                        .iter()
                        .map(OwnerId::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
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
        let mut per_source: BTreeMap<NodeId, Vec<&OwnerRefEdge>> = BTreeMap::new();
        for edge in self_refs {
            per_source
                .entry(edge.source_node_id.clone())
                .or_default()
                .push(edge);
        }
        let mut inherent_nodes = BTreeSet::new();
        let mut trait_nodes: BTreeMap<String, BTreeSet<NodeId>> = BTreeMap::new();

        for (source_node_id, source_edges) in per_source {
            let allow_rule = source_edges.iter().any(|edge| {
                edge.node_ids.iter().any(|node_id| {
                    node_map
                        .get(node_id)
                        .map(|node| graph::node_has_allow_rule(node, "cycle.type_method_self_loop"))
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
                    && edge.source_impl_kind != Some(ImplKind::Trait)
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
                    && edge.source_impl_kind == Some(ImplKind::Trait)
            });
            if is_trait_impl_self_loop {
                let trait_name = node_map
                    .get(&source_node_id)
                    .and_then(|node| node.impl_trait.clone())
                    .unwrap_or_else(|| "unknown_trait".to_string());
                if is_non_architectural_trait_impl_self_loop(&trait_name) {
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
                rule_id: RuleId::ScbCycle002,
                kind: "type_method_self_loop".to_string(),
                message: format!("type/method self-loop on owner {owner_id}"),
                owner_ids: vec![owner_id.clone()],
                node_ids: inherent_nodes.into_iter().collect(),
            });
        }

        for (trait_name, node_ids) in trait_nodes {
            findings.push(Finding {
                rule_id: RuleId::ScbCycle003,
                kind: "trait_impl_self_loop".to_string(),
                message: format!("trait-impl self-loop on owner {owner_id} via {trait_name}"),
                owner_ids: vec![owner_id.clone()],
                node_ids: node_ids.into_iter().collect(),
            });
        }
    }

    findings
}

fn component_allows_recursive_value_container(
    owners: &[OwnerId],
    node_map: &BTreeMap<NodeId, &GraphNode>,
) -> bool {
    owners.iter().all(|owner_id| {
        node_map
            .get(&NodeId::new(owner_id.as_str()))
            .map(|node| {
                node.kind == "type"
                    && graph::node_has_allow_rule(node, "cycle.recursive_value_container")
            })
            .unwrap_or(false)
    })
}

pub(crate) fn is_non_architectural_trait_impl_self_loop(trait_path: &str) -> bool {
    let defaults = &graph::default_rule_defaults().trait_self_loop;
    defaults
        .ignored_trait_paths
        .iter()
        .any(|ignored| ignored == trait_path)
        || defaults
            .ignored_trait_names
            .iter()
            .any(|ignored| ignored == graph::trait_terminal_name(trait_path))
}

pub(crate) fn analyze_internal_only(graph: &GraphExport) -> Vec<Finding> {
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
                rule_id: RuleId::ScbBoundary001,
                kind: "internal_only_visibility_violation".to_string(),
                message: format!(
                    "internal_only item {} must not be externally visible (visibility={})",
                    node.id.as_str(),
                    node.visibility.unwrap_or("unknown")
                ),
                owner_ids: vec![OwnerId::new(node.id.as_str())],
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
                .unwrap_or_else(|| OwnerId::new(source_node.id.as_str()));
            if !seen_external_sources.insert(source_owner_id.clone()) {
                continue;
            }
            findings.push(Finding {
                rule_id: RuleId::ScbBoundary002,
                kind: "internal_only_external_reference".to_string(),
                message: format!(
                    "internal_only item {} referenced from {}",
                    node.id.as_str(),
                    source_owner_id.as_str()
                ),
                owner_ids: vec![OwnerId::new(node.id.as_str())],
                node_ids: vec![NodeId::new(source_owner_id.as_str()), node.id.clone()],
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

pub(crate) fn analyze_forbid_external_impls(graph: &GraphExport) -> Vec<Finding> {
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
                rule_id: RuleId::ScbBoundary003,
                kind: "forbid_external_impls_violation".to_string(),
                message: format!(
                    "trait {} forbids external impls but is implemented from module {}",
                    trait_node.id.as_str(),
                    impl_node.module_path.as_deref().unwrap_or("unknown_module")
                ),
                owner_ids: vec![OwnerId::new(trait_node.id.as_str())],
                node_ids: vec![impl_node.id.clone(), trait_node.id.clone()],
            });
        }
    }

    findings.sort_by(|left, right| left.message.cmp(&right.message));
    findings
}

pub(crate) fn finding_is_failure(finding: &Finding) -> bool {
    matches!(
        finding.rule_id,
        RuleId::ScbCycle001
            | RuleId::ScbBoundary001
            | RuleId::ScbBoundary002
            | RuleId::ScbBoundary003
            | RuleId::Port001
            | RuleId::Port002
            | RuleId::Port003
    )
}

pub(crate) fn finding_sort_key(finding: &Finding) -> (u8, RuleId) {
    let severity = if finding_is_failure(finding) { 0 } else { 1 };
    (severity, finding.rule_id)
}

#[derive(Debug, Clone)]
struct OwnerRefEdge {
    source_owner_id: OwnerId,
    target_owner_id: OwnerId,
    owner_kind: &'static str,
    source_kind: &'static str,
    source_node_id: NodeId,
    source_impl_kind: Option<ImplKind>,
    reference_kind: ReferenceKind,
    node_ids: Vec<NodeId>,
}

#[derive(Default)]
struct OwnerGraph {
    adjacency: BTreeMap<OwnerId, BTreeSet<OwnerId>>,
    self_refs: BTreeMap<OwnerId, Vec<OwnerRefEdge>>,
    ref_edges: Vec<OwnerRefEdge>,
}

fn build_owner_graph<'a>(
    graph: &'a GraphExport,
    node_map: &BTreeMap<NodeId, &'a GraphNode>,
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

fn owner_contributors(owners: &[OwnerId], owner_graph: &OwnerGraph) -> Vec<NodeId> {
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

fn owner_kind_for_node_id(
    owner_id: &OwnerId,
    node_map: &BTreeMap<NodeId, &GraphNode>,
) -> Option<&'static str> {
    node_map
        .get(&NodeId::new(owner_id.as_str()))
        .map(|node| node.kind)
}

fn owner_id_for_node_id(node_id: &NodeId, node_kind: &str) -> Option<OwnerId> {
    match node_kind {
        "module" | "type" | "trait" => Some(OwnerId::new(node_id.as_str())),
        "function" => node_id
            .rsplit_once("::")
            .map(|(parent, _)| OwnerId::new(parent)),
        "method" => node_id
            .rsplit_once("::")
            .map(|(parent, _)| OwnerId::new(parent)),
        "variant" => node_id
            .rsplit_once("::variant::")
            .map(|(parent, _)| OwnerId::new(parent)),
        "field" => {
            let (parent, _) = node_id.rsplit_once("::field::")?;
            if let Some((enum_parent, _variant)) = parent.rsplit_once("::variant::") {
                Some(OwnerId::new(enum_parent))
            } else {
                Some(OwnerId::new(parent))
            }
        }
        _ => None,
    }
}

fn strongly_connected_components(owner_graph: &OwnerGraph) -> Vec<Vec<OwnerId>> {
    fn visit(
        node: &OwnerId,
        owner_graph: &OwnerGraph,
        visited: &mut BTreeSet<OwnerId>,
        order: &mut Vec<OwnerId>,
    ) {
        if !visited.insert(node.clone()) {
            return;
        }
        if let Some(neighbors) = owner_graph.adjacency.get(node) {
            for neighbor in neighbors {
                visit(neighbor, owner_graph, visited, order);
            }
        }
        order.push(node.clone());
    }

    fn reverse_graph(owner_graph: &OwnerGraph) -> BTreeMap<OwnerId, BTreeSet<OwnerId>> {
        let mut reversed: BTreeMap<OwnerId, BTreeSet<OwnerId>> = BTreeMap::new();
        for (source, targets) in &owner_graph.adjacency {
            reversed.entry(source.clone()).or_default();
            for target in targets {
                reversed
                    .entry(target.clone())
                    .or_default()
                    .insert(source.clone());
            }
        }
        reversed
    }

    fn collect_component(
        node: &OwnerId,
        reversed: &BTreeMap<OwnerId, BTreeSet<OwnerId>>,
        visited: &mut BTreeSet<OwnerId>,
        component: &mut Vec<OwnerId>,
    ) {
        if !visited.insert(node.clone()) {
            return;
        }
        component.push(node.clone());
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
