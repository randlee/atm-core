use super::*;

pub fn render_findings_report(report: &FindingsReport) -> String {
    format!(
        "{} {} status={} scanned_crates={} findings={}",
        report.tool,
        report.version,
        report.status.as_str(),
        report.scanned_crates,
        report.findings.len()
    )
}

/// Render a graph export to the requested wire format.
///
/// JSON and Turtle rendering are both treated as infallible for this graph
/// model. JSON uses `serde_json` over an already-built in-memory structure that
/// contains only stringly metadata and enums, while Turtle is assembled
/// directly into a string.
pub fn render_graph_export(graph: &GraphExport, format: GraphOutputFormat) -> String {
    match format {
        GraphOutputFormat::Json => render_graph_export_json(graph),
        GraphOutputFormat::Turtle => render_graph_export_turtle(graph),
    }
}

pub fn render_graph_export_json(graph: &GraphExport) -> String {
    serde_json::to_string_pretty(graph)
        .expect("GraphExport serialization is structurally infallible")
}

pub fn render_graph_export_turtle(graph: &GraphExport) -> String {
    let mut lines = turtle_header(graph);
    for node in &graph.nodes {
        lines.extend(render_turtle_node(node));
    }
    for edge in &graph.edges {
        lines.push(render_turtle_edge(edge));
    }
    lines.join("\n")
}

fn turtle_header(graph: &GraphExport) -> Vec<String> {
    vec![
        "@prefix sc: <urn:sc-lint-boundary:predicate:> .".to_string(),
        "@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .".to_string(),
        format!(
            "<urn:sc-lint-boundary:graph> sc:schemaVersion {} .",
            turtle_string_literal(graph.schema_version)
        ),
        String::new(),
    ]
}

fn render_turtle_node(node: &GraphNode) -> Vec<String> {
    let subject = node_iri(&node.id);
    let mut lines = vec![
        format!("{subject} rdf:type sc:{} .", node.kind),
        format!(
            "{subject} sc:id {} .",
            turtle_string_literal(node.id.as_str())
        ),
        format!(
            "{subject} sc:label {} .",
            turtle_string_literal(&node.label)
        ),
        format!(
            "{subject} sc:package {} .",
            turtle_string_literal(&node.package)
        ),
        format!(
            "{subject} sc:manifestPath {} .",
            turtle_string_literal(&node.manifest_path)
        ),
    ];
    push_optional_node_fields(&mut lines, &subject, node);
    push_attribute_lines(&mut lines, &subject, &node.attributes);
    lines.push(String::new());
    lines
}

fn push_optional_node_fields(lines: &mut Vec<String>, subject: &str, node: &GraphNode) {
    if let Some(visibility) = node.visibility {
        lines.push(format!(
            "{subject} sc:visibility {} .",
            turtle_string_literal(visibility)
        ));
    }
    if let Some(target) = &node.target {
        lines.push(format!(
            "{subject} sc:target {} .",
            turtle_string_literal(target)
        ));
    }
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
            turtle_string_literal(impl_kind.as_str())
        ));
    }
    if let Some(impl_trait) = &node.impl_trait {
        lines.push(format!(
            "{subject} sc:implTrait {} .",
            turtle_string_literal(impl_trait)
        ));
    }
}

fn push_attribute_lines(lines: &mut Vec<String>, subject: &str, attributes: &[LintAttribute]) {
    for attr in attributes {
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
}

fn render_turtle_edge(edge: &GraphEdge) -> String {
    format!(
        "{} sc:{} {} .",
        node_iri(&edge.from),
        edge.kind,
        node_iri(&edge.to)
    )
}

fn node_iri(node_id: &NodeId) -> String {
    format!(
        "<urn:sc-lint-boundary:node:{}>",
        hex_encode(node_id.as_str().as_bytes())
    )
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
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
