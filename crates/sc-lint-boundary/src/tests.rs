#![cfg(test)]

use super::*;
use std::fs;
use std::path::Path;

use tempfile::TempDir;

#[test]
fn findings_report_text_is_stable() {
    let report = super::FindingsReport {
        tool: "sc-lint-boundary",
        version: "0.1.0",
        schema_version: "0.1.0",
        status: ReportStatus::Pass,
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
#[should_panic(expected = "NodeId::new requires a non-empty identifier")]
fn node_id_rejects_empty_identifier() {
    let _ = NodeId::new("");
}

#[test]
#[should_panic(expected = "OwnerId::new requires a non-empty identifier")]
fn owner_id_rejects_empty_identifier() {
    let _ = OwnerId::new("");
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
        .find(|node| node.id == "crate::example::example::module::crate::inline_mod::InlineType")
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
            node.id
                == "crate::example::example::module::crate::inline_mod::InlineType::impl::inherent::helper"
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
                == "crate::example::example::module::crate::inline_mod::InlineType::impl::inherent::helper"
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == "references"
            && edge.from
                == "crate::example::example::module::crate::inline_mod::InlineType::impl::inherent::helper"
            && edge.to == "crate::example::example::module::crate::inline_mod::InlineType"
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == "references_expr"
            && edge.from
                == "crate::example::example::module::crate::inline_mod::InlineType::impl::inherent::helper"
            && edge.to == "crate::example::example::module::crate::inline_mod::InlineType"
    }));
    assert!(!graph.edges.iter().any(|edge| {
        edge.from
                == "crate::example::example::module::crate::inline_mod::InlineType::impl::inherent::helper"
            && edge.to == "crate::example::example::module::crate::inline_mod::self"
    }));
}

#[test]
fn keeps_same_named_inherent_and_trait_methods_distinct() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
                pub struct Loop;

                pub trait Retained {
                    fn load(&self) -> usize;
                }

                impl Loop {
                    pub fn load(&self) -> usize {
                        1
                    }
                }

                impl Retained for Loop {
                    fn load(&self) -> usize {
                        Self::load(self)
                    }
                }
            "#,
    );

    let graph = export_workspace_graph(&ExportGraphOptions {
        root: fixture.root().to_path_buf(),
    })
    .unwrap();
    let load_methods: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| node.kind == "method" && node.label == "load")
        .collect();
    assert_eq!(load_methods.len(), 2);

    let inherent = load_methods
        .iter()
        .find(|node| node.impl_kind == Some(ImplKind::Inherent))
        .unwrap();
    let trait_method = load_methods
        .iter()
        .find(|node| node.impl_kind == Some(ImplKind::Trait))
        .unwrap();
    assert_ne!(inherent.id, trait_method.id);
    assert!(inherent.id.ends_with("::impl::inherent::load"));
    assert!(trait_method.id.contains("::impl::"));
    assert!(trait_method.id.ends_with("::load"));
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == "contains" && edge.from.ends_with("::impl::inherent") && edge.to == inherent.id
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == "contains"
            && edge.from.contains("::impl::")
            && edge.from != "crate::example::example::module::crate::Loop::impl::inherent"
            && edge.to == trait_method.id
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == "references_expr"
            && edge.from == trait_method.id
            && edge.to == "crate::example::example::module::crate::Loop"
    }));

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();
    assert!(!report.findings.iter().any(|finding| {
        finding.rule_id == RuleId::ScbCycle002
            && finding
                .node_ids
                .iter()
                .any(|node_id| node_id == &trait_method.id)
    }));
    let trait_finding = report
        .findings
        .iter()
        .find(|finding| finding.rule_id == RuleId::ScbCycle003)
        .unwrap();
    assert_eq!(
        trait_finding.owner_ids,
        vec!["crate::example::example::module::crate::Loop".to_string()]
    );
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
            && edge.to == "crate::example::example::module::crate::Choice::variant::Pair::field::0"
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
    let turtle = render_graph_export_turtle(&graph);

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

    assert!(
        graph.nodes.iter().any(|node| {
            node.id == "crate::example::example::module::crate::outer::child::Nested"
        })
    );
}

#[test]
fn resolves_path_attribute_module_layout() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_source(
        "example",
        "lib.rs",
        "#[path = \"support/aliased.rs\"] mod custom;",
    );
    fixture.write_source("example", "support/aliased.rs", "pub struct FromPathAttr;");

    let graph = export_workspace_graph(&ExportGraphOptions {
        root: fixture.root().to_path_buf(),
    })
    .unwrap();

    assert!(
        graph.nodes.iter().any(|node| {
            node.id == "crate::example::example::module::crate::custom::FromPathAttr"
        })
    );
}

#[test]
fn resolves_nested_submodule_inside_path_attribute_module() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_source(
        "example",
        "lib.rs",
        "#[path = \"support/aliased.rs\"] mod custom;",
    );
    fixture.write_source("example", "support/aliased.rs", "mod nested;");
    fixture.write_source("example", "support/nested.rs", "pub struct NestedFromPath;");

    let graph = export_workspace_graph(&ExportGraphOptions {
        root: fixture.root().to_path_buf(),
    })
    .unwrap();

    assert!(graph.nodes.iter().any(|node| {
        node.id == "crate::example::example::module::crate::custom::nested::NestedFromPath"
    }));
}

#[test]
fn fails_when_path_attribute_file_is_missing() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_source(
        "example",
        "lib.rs",
        "#[path = \"support/missing.rs\"] mod custom;",
    );

    let error = export_workspace_graph(&ExportGraphOptions {
        root: fixture.root().to_path_buf(),
    })
    .unwrap_err();

    let message = format!("{error:#}");
    assert!(message.contains("while resolving module `crate::custom`"));
    assert!(message.contains("path attribute resolved to missing file"));
    assert!(message.contains("support/missing.rs"));
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
    assert_eq!(report.status, ReportStatus::Pass);
    assert!(report.findings.is_empty());
}

#[test]
fn rejects_unknown_rule_filter() {
    let error = RuleFilter::try_from("unknown").unwrap_err();
    assert_eq!(
        error.to_string(),
        "unsupported rule filter `unknown`; supported: cycles, boundaries, internal_only, forbid_external_impls, portability"
    );
}

#[test]
fn flags_hardcoded_tmp_path_in_test_scope() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_lint_config(
        r#"
        [portability]
        config_home_env = "ATM_CONFIG_HOME"
        "#,
    );
    let fixture_source = format!(
        r#"
            #[cfg(test)]
            mod tests {{
                use std::path::PathBuf;

                #[test]
                fn writes_artifact() {{
                    let _ = PathBuf::from("{tmp_path}");
                }}
            }}
        "#,
        tmp_path = ["/", "tmp", "/test-artifact"].concat(),
    );
    fixture.write_source("example", "lib.rs", &fixture_source);

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: Some(RuleFilter::Portability),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Fail);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule_id, RuleId::Port001);
}

#[test]
fn passes_temp_dir_usage_in_test_scope() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_lint_config(
        r#"
        [portability]
        config_home_env = "ATM_CONFIG_HOME"
        "#,
    );
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
            #[cfg(test)]
            mod tests {
                #[test]
                fn writes_artifact() {
                    let _ = std::env::temp_dir().join("test-artifact");
                }
            }
        "#,
    );

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: Some(RuleFilter::Portability),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
    assert!(report.findings.is_empty());
}

#[test]
fn does_not_flag_unix_only_production_code() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_lint_config(
        r#"
        [portability]
        config_home_env = "ATM_CONFIG_HOME"
        "#,
    );
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
            #[cfg(unix)]
            pub fn socket_path() -> std::path::PathBuf {
                std::env::temp_dir().join("runtime-socket")
            }
        "#,
    );

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: Some(RuleFilter::Portability),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
    assert!(report.findings.is_empty());
}

#[test]
fn flags_dirs_home_dir_without_override_check() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_lint_config(
        r#"
        [portability]
        config_home_env = "ATM_CONFIG_HOME"
        "#,
    );
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
            #[cfg(test)]
            mod tests {
                fn config_root() -> std::path::PathBuf {
                    dirs::home_dir().expect("home")
                }
            }
        "#,
    );

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: Some(RuleFilter::Portability),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Fail);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule_id, RuleId::Port002);
    assert!(report.findings[0].message.contains("ATM_CONFIG_HOME"));
}

#[test]
fn passes_dirs_home_dir_after_override_check() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_lint_config(
        r#"
        [portability]
        config_home_env = "ATM_CONFIG_HOME"
        "#,
    );
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
            #[cfg(test)]
            mod tests {
                fn get_os_home_dir() -> std::path::PathBuf {
                    if let Ok(home) = std::env::var("ATM_CONFIG_HOME") {
                        return std::path::PathBuf::from(home);
                    }
                    dirs::home_dir().expect("home")
                }
            }
        "#,
    );

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: Some(RuleFilter::Portability),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
    assert!(report.findings.is_empty());
}

#[test]
fn flags_set_var_in_test_scope() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_lint_config(
        r#"
        [portability]
        config_home_env = "ATM_CONFIG_HOME"
        "#,
    );
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
            #[cfg(test)]
            mod tests {
                #[test]
                fn mutates_env() {
                    unsafe { std::env::set_var("HOME", "test-home") };
                }
            }
        "#,
    );

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: Some(RuleFilter::Portability),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Fail);
    assert_eq!(report.findings.len(), 1);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.rule_id == RuleId::Port003)
    );
}

#[test]
fn flags_ungated_std_os_unix_import_in_production_code() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_lint_config(
        r#"
        [portability]
        config_home_env = "ATM_CONFIG_HOME"
        "#,
    );
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
            pub fn os_string(bytes: Vec<u8>) -> std::ffi::OsString {
                use std::os::unix::ffi::OsStringExt;
                std::ffi::OsString::from_vec(bytes)
            }
        "#,
    );

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: Some(RuleFilter::Portability),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Fail);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.rule_id == RuleId::Port004)
    );
}

#[test]
fn passes_cfg_unix_gated_std_os_unix_import_in_production_code() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_lint_config(
        r#"
        [portability]
        config_home_env = "ATM_CONFIG_HOME"
        "#,
    );
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
            #[cfg(unix)]
            pub fn os_string(bytes: Vec<u8>) -> std::ffi::OsString {
                use std::os::unix::ffi::OsStringExt;
                std::ffi::OsString::from_vec(bytes)
            }
        "#,
    );

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: Some(RuleFilter::Portability),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
    assert!(report.findings.is_empty());
}

#[test]
fn flags_cfg_attr_not_unix_allow_dead_code_in_production_code() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_lint_config(
        r#"
        [portability]
        config_home_env = "ATM_CONFIG_HOME"
        "#,
    );
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
            #[cfg_attr(not(unix), allow(dead_code))]
            pub fn unix_socket_path() -> std::path::PathBuf {
                std::env::temp_dir().join("runtime.sock")
            }
        "#,
    );

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: Some(RuleFilter::Portability),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Fail);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.rule_id == RuleId::Port005)
    );
}

#[test]
fn flags_bare_wait_in_production_code() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
            use std::sync::{Condvar, Mutex};

            pub fn block_until_ready(condvar: &Condvar, state: &Mutex<bool>) {
                let state = state.lock().expect("lock");
                let _guard = condvar.wait(state).expect("wait");
            }
        "#,
    );

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: None,
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Fail);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.rule_id == RuleId::ScbRuntime001)
    );
}

#[test]
fn flags_discarded_wait_timeout_result_in_production_code() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
            use std::sync::{Condvar, Mutex};
            use std::time::Duration;

            pub fn block_until_ready(condvar: &Condvar, state: &Mutex<bool>) {
                let state = state.lock().expect("lock");
                condvar.wait_timeout(state, Duration::from_secs(1)).expect("wait");
            }
        "#,
    );

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: None,
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Fail);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.rule_id == RuleId::ScbRuntime002)
    );
}

#[test]
fn passes_inspected_wait_timeout_result_in_production_code() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
            use std::sync::{Condvar, Mutex};
            use std::time::Duration;

            pub fn block_until_ready(condvar: &Condvar, state: &Mutex<bool>) {
                let state = state.lock().expect("lock");
                let (_guard, wait) = condvar
                    .wait_timeout(state, Duration::from_secs(1))
                    .expect("wait");
                if wait.timed_out() {
                    return;
                }
            }
        "#,
    );

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: None,
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
    assert!(!report.findings.iter().any(|finding| matches!(
        finding.rule_id,
        RuleId::ScbRuntime001 | RuleId::ScbRuntime002
    )));
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule_id, RuleId::ScbCycle002);
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule_id, RuleId::ScbCycle002);
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
    assert_eq!(report.findings.len(), 2);
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule_id == RuleId::ScbCycle002)
    );
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule_id == RuleId::ScbCycle003)
    );
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule_id, RuleId::ScbCycle003);
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
    assert!(report.findings.is_empty());
}

#[test]
fn does_not_flag_comparison_like_trait_impl_self_loop() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
                #[derive(Debug, Clone)]
                pub struct Loop(String);

                impl core::cmp::PartialEq for Loop {
                    fn eq(&self, other: &Self) -> bool {
                        self.0 == other.0
                    }
                }

                impl core::cmp::Eq for Loop {}
            "#,
    );

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
    assert!(report.findings.is_empty());
}

#[test]
fn normalizes_trait_paths_for_exact_policy_matching() {
    let path: syn::Path = syn::parse_quote!(serde::Deserialize<'de>);
    assert_eq!(graph::trait_path_key(&path), "serde::Deserialize");

    let path: syn::Path = syn::parse_quote!(core::cmp::PartialEq);
    assert_eq!(graph::trait_path_key(&path), "core::cmp::PartialEq");
}

#[test]
fn default_rule_config_ignores_common_non_architectural_trait_paths() {
    assert!(analysis::is_non_architectural_trait_impl_self_loop(
        "core::cmp::PartialEq"
    ));
    assert!(analysis::is_non_architectural_trait_impl_self_loop(
        "serde::Deserialize"
    ));
    assert!(analysis::is_non_architectural_trait_impl_self_loop(
        "FromStr"
    ));
    assert!(analysis::is_non_architectural_trait_impl_self_loop("Parse"));
    assert!(!analysis::is_non_architectural_trait_impl_self_loop(
        "one::Display"
    ));
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Fail);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule_id, RuleId::ScbCycle001);
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
fn suppresses_recursive_value_container_cycle_when_all_owners_allow_it() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
                #[sc_lint(boundary.allow("cycle.recursive_value_container"))]
                pub enum Value {
                    Object(Map),
                }

                #[sc_lint(boundary.allow("cycle.recursive_value_container"))]
                pub struct Map {
                    entries: Vec<Value>,
                }
            "#,
    );

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
    assert!(report.findings.is_empty());
}

#[test]
fn keeps_recursive_value_container_cycle_when_only_one_owner_allows_it() {
    let fixture = WorkspaceFixture::new();
    fixture.write_workspace_root();
    fixture.write_package_manifest("example");
    fixture.write_source(
        "example",
        "lib.rs",
        r#"
                #[sc_lint(boundary.allow("cycle.recursive_value_container"))]
                pub enum Value {
                    Object(Map),
                }

                pub struct Map {
                    entries: Vec<Value>,
                }
            "#,
    );

    let report = analyze_workspace(&AnalyzeOptions {
        root: fixture.root().to_path_buf(),
        format: OutputFormat::Json,
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Fail);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule_id, RuleId::ScbCycle001);
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Fail);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule_id, RuleId::ScbCycle001);
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
        rule: Some(RuleFilter::InternalOnly),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Fail);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule_id, RuleId::ScbBoundary001);
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
        rule: Some(RuleFilter::InternalOnly),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Fail);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule_id, RuleId::ScbBoundary002);
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
        rule: Some(RuleFilter::InternalOnly),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
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
        rule: Some(RuleFilter::Boundaries),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Fail);
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule_id == RuleId::ScbBoundary003)
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
        rule: Some(RuleFilter::Boundaries),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.status, ReportStatus::Pass);
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
        rule: Some(RuleFilter::Cycles),
    })
    .unwrap();

    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule_id, RuleId::ScbCycle003);
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

    let message = format!("{error:#}");
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

    let message = format!("{error:#}");
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

    let message = format!("{error:#}");
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

    assert!(format!("{error:#}").contains("boundary.allow rule ids must not be empty"));
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

    assert!(format!("{error:#}").contains("boundary.allow requires at least one rule id string"));
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

    assert!(format!("{error:#}").contains("unsupported boundary directive"));
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

    assert!(format!("{error:#}").contains("unsupported sc_lint scope `other`"));
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

    assert!(format!("{error:#}").contains("unsupported boundary directive"));
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

    fn write_lint_config(&self, contents: &str) {
        self.write(".just/lint-config.toml", contents);
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
