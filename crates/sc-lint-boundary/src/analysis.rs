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
            if let Some(finding) = multi_owner_cycle_finding(&component, &node_map, &owner_graph) {
                findings.push(finding);
            }
            continue;
        }
        findings.extend(analyze_self_cycle_component(
            &component[0],
            &node_map,
            &owner_graph,
        ));
    }

    findings
}

fn multi_owner_cycle_finding(
    component: &[OwnerId],
    node_map: &BTreeMap<NodeId, &GraphNode>,
    owner_graph: &OwnerGraph,
) -> Option<Finding> {
    if component_allows_recursive_value_container(component, node_map) {
        return None;
    }
    let mut owners = component.to_vec();
    owners.sort();
    Some(Finding {
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
        node_ids: owner_contributors(&owners, owner_graph),
    })
}

fn analyze_self_cycle_component(
    owner_id: &OwnerId,
    node_map: &BTreeMap<NodeId, &GraphNode>,
    owner_graph: &OwnerGraph,
) -> Vec<Finding> {
    let Some(self_refs) = owner_graph.self_refs.get(owner_id) else {
        return Vec::new();
    };
    if self_refs.is_empty() {
        return Vec::new();
    }
    let mut per_source: BTreeMap<NodeId, Vec<&OwnerRefEdge>> = BTreeMap::new();
    for edge in self_refs {
        per_source
            .entry(edge.source_node_id.clone())
            .or_default()
            .push(edge);
    }
    let classified = classify_self_cycle_edges(per_source, node_map, owner_graph);
    let mut findings = Vec::new();
    if !classified.inherent_nodes.is_empty() {
        findings.push(Finding {
            rule_id: RuleId::ScbCycle002,
            kind: "type_method_self_loop".to_string(),
            message: format!("type/method self-loop on owner {owner_id}"),
            owner_ids: vec![owner_id.clone()],
            node_ids: classified.inherent_nodes.into_iter().collect(),
        });
    }
    findings.extend(
        classified
            .trait_nodes
            .into_iter()
            .map(|(trait_name, node_ids)| Finding {
                rule_id: RuleId::ScbCycle003,
                kind: "trait_impl_self_loop".to_string(),
                message: format!("trait-impl self-loop on owner {owner_id} via {trait_name}"),
                owner_ids: vec![owner_id.clone()],
                node_ids: node_ids.into_iter().collect(),
            }),
    );
    findings
}

struct ClassifiedSelfCycles {
    inherent_nodes: BTreeSet<NodeId>,
    trait_nodes: BTreeMap<String, BTreeSet<NodeId>>,
}

fn classify_self_cycle_edges(
    per_source: BTreeMap<NodeId, Vec<&OwnerRefEdge>>,
    node_map: &BTreeMap<NodeId, &GraphNode>,
    owner_graph: &OwnerGraph,
) -> ClassifiedSelfCycles {
    let mut classified = ClassifiedSelfCycles {
        inherent_nodes: BTreeSet::new(),
        trait_nodes: BTreeMap::new(),
    };
    for (source_node_id, source_edges) in per_source {
        if self_cycle_edges_are_allowed(&source_edges, node_map) {
            continue;
        }
        if is_type_method_self_loop(&source_edges, node_map, owner_graph) {
            collect_inherent_self_loop_nodes(&mut classified.inherent_nodes, source_edges);
            continue;
        }
        if let Some(trait_name) =
            classify_trait_impl_self_loop(&source_node_id, &source_edges, node_map, owner_graph)
        {
            let entry = classified.trait_nodes.entry(trait_name).or_default();
            for edge in source_edges {
                for node_id in &edge.node_ids {
                    entry.insert(node_id.clone());
                }
            }
        }
    }
    classified
}

fn self_cycle_edges_are_allowed(
    source_edges: &[&OwnerRefEdge],
    node_map: &BTreeMap<NodeId, &GraphNode>,
) -> bool {
    source_edges.iter().any(|edge| {
        edge.node_ids.iter().any(|node_id| {
            node_map
                .get(node_id)
                .map(|node| graph::node_has_allow_rule(node, "cycle.type_method_self_loop"))
                .unwrap_or(false)
        })
    })
}

fn is_type_method_self_loop(
    source_edges: &[&OwnerRefEdge],
    node_map: &BTreeMap<NodeId, &GraphNode>,
    owner_graph: &OwnerGraph,
) -> bool {
    let source_edges: Vec<_> = non_delegating_self_loop_edges(source_edges, node_map, owner_graph)
        .into_iter()
        .filter(|edge| !is_enum_variant_expression(edge))
        .collect();
    let is_type_method_self_loop = !source_edges.is_empty()
        && source_edges.iter().all(|edge| {
            edge.source_kind == "method"
                && edge.owner_kind == "type"
                && edge.target_owner_id == edge.source_owner_id
                && edge.source_impl_kind != Some(ImplKind::Trait)
        });
    if !is_type_method_self_loop {
        return false;
    }
    let has_expr_ref = source_edges
        .iter()
        .any(|edge| edge.reference_kind == ReferenceKind::Expr);
    let has_type_ref = source_edges
        .iter()
        .any(|edge| edge.reference_kind == ReferenceKind::Type);
    has_expr_ref && !has_type_ref
}

fn collect_inherent_self_loop_nodes(
    inherent_nodes: &mut BTreeSet<NodeId>,
    source_edges: Vec<&OwnerRefEdge>,
) {
    for edge in source_edges {
        for node_id in &edge.node_ids {
            inherent_nodes.insert(node_id.clone());
        }
    }
}

fn classify_trait_impl_self_loop(
    source_node_id: &NodeId,
    source_edges: &[&OwnerRefEdge],
    node_map: &BTreeMap<NodeId, &GraphNode>,
    owner_graph: &OwnerGraph,
) -> Option<String> {
    let source_edges: Vec<_> = source_edges
        .iter()
        .copied()
        .filter(|edge| !is_enum_variant_expression(edge))
        .filter(|edge| !is_trait_impl_delegating_call(edge, node_map, owner_graph))
        .collect();
    let is_trait_impl_self_loop = !source_edges.is_empty()
        && source_edges.iter().all(|edge| {
            edge.source_kind == "method"
                && edge.owner_kind == "type"
                && edge.target_owner_id == edge.source_owner_id
                && edge.source_impl_kind == Some(ImplKind::Trait)
        });
    if !is_trait_impl_self_loop {
        return None;
    }
    let trait_name = node_map
        .get(source_node_id)
        .and_then(|node| node.impl_trait.clone())
        .unwrap_or_else(|| "unknown_trait".to_string());
    (!is_non_architectural_trait_impl_self_loop(&trait_name)).then_some(trait_name)
}

fn is_trait_impl_delegating_call(
    edge: &OwnerRefEdge,
    node_map: &BTreeMap<NodeId, &GraphNode>,
    owner_graph: &OwnerGraph,
) -> bool {
    let Some(callee) = &edge.call_callee else {
        return false;
    };
    if edge.reference_kind != ReferenceKind::Expr || edge.source_owner_id != edge.target_owner_id {
        return false;
    }
    let Some(source_method) = node_map
        .get(&edge.source_node_id)
        .map(|node| node.label.as_str())
    else {
        return false;
    };
    let same_name_inherent_forwarder = callee.kind == CallCalleeKind::Associated
        && source_method == callee.ident
        && has_inherent_method(edge, callee, node_map);
    if source_method == callee.ident && !same_name_inherent_forwarder {
        return false;
    }
    let is_delegating_shape = match callee.kind {
        CallCalleeKind::Receiver => true,
        CallCalleeKind::Associated => {
            same_name_inherent_forwarder || is_non_public_associated_helper(edge, callee, node_map)
        }
    };
    is_delegating_shape && !call_edge_participates_in_cycle(edge, node_map, owner_graph)
}

fn has_inherent_method(
    edge: &OwnerRefEdge,
    callee: &CallCallee,
    node_map: &BTreeMap<NodeId, &GraphNode>,
) -> bool {
    node_map.values().any(|node| {
        node.kind == "method"
            && node.label == callee.ident
            && node.impl_kind == Some(ImplKind::Inherent)
            && owner_id_for_node_id(&node.id, node.kind).as_ref() == Some(&edge.target_owner_id)
    })
}

fn is_non_public_associated_helper(
    edge: &OwnerRefEdge,
    callee: &CallCallee,
    node_map: &BTreeMap<NodeId, &GraphNode>,
) -> bool {
    node_map.values().any(|node| {
        node.kind == "method"
            && node.label == callee.ident
            && node.visibility != Some("public")
            && owner_id_for_node_id(&node.id, node.kind).as_ref() == Some(&edge.target_owner_id)
    })
}

fn non_delegating_self_loop_edges<'a>(
    source_edges: &[&'a OwnerRefEdge],
    node_map: &BTreeMap<NodeId, &GraphNode>,
    owner_graph: &OwnerGraph,
) -> Vec<&'a OwnerRefEdge> {
    source_edges
        .iter()
        .copied()
        .filter(|edge| !is_delegating_call(edge, node_map, owner_graph))
        .collect()
}

fn is_delegating_call(
    edge: &OwnerRefEdge,
    node_map: &BTreeMap<NodeId, &GraphNode>,
    owner_graph: &OwnerGraph,
) -> bool {
    let Some(callee) = &edge.call_callee else {
        return false;
    };
    if edge.reference_kind != ReferenceKind::Expr || edge.source_owner_id != edge.target_owner_id {
        return false;
    }
    let Some(source_method) = node_map
        .get(&edge.source_node_id)
        .map(|node| node.label.as_str())
    else {
        return false;
    };
    source_method != callee.ident && !call_edge_participates_in_cycle(edge, node_map, owner_graph)
}

fn is_enum_variant_expression(edge: &OwnerRefEdge) -> bool {
    edge.reference_kind == ReferenceKind::Expr && edge.target_kind == "variant"
}

/// A same-owner helper call is safe to suppress only when it cannot take part
/// in a method-call cycle. This prevents the convenience-delegation classifier
/// from hiding indirect recursion such as `first -> second -> first`.
fn call_edge_participates_in_cycle(
    edge: &OwnerRefEdge,
    node_map: &BTreeMap<NodeId, &GraphNode>,
    owner_graph: &OwnerGraph,
) -> bool {
    let targets = call_target_methods(edge, node_map);
    if targets.is_empty() {
        return false;
    }
    let adjacency = owner_method_call_adjacency(&edge.source_owner_id, node_map, owner_graph);
    targets
        .into_iter()
        .any(|target| method_path_exists(&adjacency, &target, &edge.source_node_id))
}

fn owner_method_call_adjacency(
    owner_id: &OwnerId,
    node_map: &BTreeMap<NodeId, &GraphNode>,
    owner_graph: &OwnerGraph,
) -> BTreeMap<NodeId, BTreeSet<NodeId>> {
    let mut adjacency = BTreeMap::new();
    for edge in owner_graph.self_refs.get(owner_id).into_iter().flatten() {
        for target in call_target_methods(edge, node_map) {
            adjacency
                .entry(edge.source_node_id.clone())
                .or_insert_with(BTreeSet::new)
                .insert(target);
        }
    }
    adjacency
}

fn call_target_methods(
    edge: &OwnerRefEdge,
    node_map: &BTreeMap<NodeId, &GraphNode>,
) -> BTreeSet<NodeId> {
    let Some(callee) = &edge.call_callee else {
        return BTreeSet::new();
    };
    let Some(source) = node_map.get(&edge.source_node_id) else {
        return BTreeSet::new();
    };
    if source.kind != "method" || edge.source_owner_id != edge.target_owner_id {
        return BTreeSet::new();
    }
    let same_name_inherent_forwarder = callee.kind == CallCalleeKind::Associated
        && source.label == callee.ident
        && has_inherent_method(edge, callee, node_map);

    node_map
        .values()
        .filter(|node| {
            node.kind == "method"
                && node.label == callee.ident
                && owner_id_for_node_id(&node.id, node.kind).as_ref() == Some(&edge.target_owner_id)
                && (!same_name_inherent_forwarder || node.impl_kind == Some(ImplKind::Inherent))
        })
        .map(|node| node.id.clone())
        .collect()
}

fn method_path_exists(
    adjacency: &BTreeMap<NodeId, BTreeSet<NodeId>>,
    start: &NodeId,
    goal: &NodeId,
) -> bool {
    let mut pending = vec![start.clone()];
    let mut visited = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !visited.insert(node.clone()) {
            continue;
        }
        if &node == goal {
            return true;
        }
        if let Some(targets) = adjacency.get(&node) {
            pending.extend(targets.iter().cloned());
        }
    }
    false
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
            | RuleId::ScbRuntime001
            | RuleId::ScbRuntime002
            | RuleId::Port001
            | RuleId::Port002
            | RuleId::Port003
            | RuleId::Port004
            | RuleId::Port005
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
    target_kind: &'static str,
    source_node_id: NodeId,
    source_impl_kind: Option<ImplKind>,
    reference_kind: ReferenceKind,
    call_callee: Option<CallCallee>,
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
            target_kind: target_node.kind,
            source_node_id: source_node.id.clone(),
            source_impl_kind: source_node.impl_kind,
            reference_kind: match edge.kind {
                "references_type" => ReferenceKind::Type,
                "references_expr" => ReferenceKind::Expr,
                _ => continue,
            },
            call_callee: edge.call_callee.clone(),
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
            .split_once("::impl::")
            .map(|(owner, _)| OwnerId::new(owner))
            // Keep reading older graph fixtures while all producers converge
            // on impl-qualified method identities.
            .or_else(|| {
                node_id
                    .rsplit_once("::")
                    .map(|(parent, _)| OwnerId::new(parent))
            }),
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
