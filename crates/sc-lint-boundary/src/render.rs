use super::*;
use serde_json::Error as JsonError;

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
/// JSON rendering is fallible because it relies on `serde_json` serialization.
/// Turtle rendering is currently infallible because it formats the already-built
/// graph export into a string without additional fallible I/O or encoding work.
pub fn render_graph_export(
    graph: &GraphExport,
    format: GraphOutputFormat,
) -> Result<String, JsonError> {
    match format {
        GraphOutputFormat::Json => render_graph_export_json(graph),
        GraphOutputFormat::Turtle => Ok(render_graph_export_turtle(graph)),
    }
}

pub fn render_graph_export_json(graph: &GraphExport) -> Result<String, JsonError> {
    serde_json::to_string_pretty(graph)
}

pub fn render_graph_export_turtle(graph: &GraphExport) -> String {
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
            turtle_string_literal(node.id.as_str())
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
                turtle_string_literal(impl_kind.as_str())
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
