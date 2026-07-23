// ARCHITECTURE ENFORCEMENT CRATE
// Every assertion in this file is a merge gate.
// Removing, weakening, or commenting out any assertion requires an explicit
// architecture decision recorded in docs/architecture.md.
// QA MUST FAIL any PR that loosens a boundary assertion without that record.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use cargo_metadata::{DependencyKind, MetadataCommand};
use serde::Deserialize;
use syn::visit::Visit;

const EXPECTED_FORBIDDEN_EDGES: &[(&str, &str)] = &[
    ("atm", "atm-storage-rusqlite"),
    ("atm-daemon", "atm-storage-rusqlite"),
    ("atm-runtime", "atm-storage-rusqlite"),
    ("atm-storage-rusqlite", "atm-runtime"),
    ("atm-graft", "atm-daemon"),
    ("atm-graft", "atm-daemon-bootstrap"),
    ("atm-graft", "atm-storage-rusqlite"),
    ("atm-graft", "interprocess"),
    ("atm-runtime", "atm-daemon"),
];

const RETIRED_DAEMON_CONSTRUCT_FRAGMENTS: &[(&str, &str)] = &[
    ("peer_", "transport"),
    ("Peer", "Transport"),
    ("Remote", "Replay"),
    ("replay_", "store"),
    ("remote_retry_", "budget"),
    ("RemoteDelivery", "OutcomeUnknown"),
];

const RETIRED_ERROR_CONTRACT_SYMBOLS: &[&str] = &[
    "AtmErrorKind",
    "ProtocolErrorEnvelope",
    "error_kind_for_code",
];

const AI11_RETIRED_WINDOWS_TRANSPORT_IDENTIFIERS: &[&str] = &[
    "NamedPipe",
    "named_pipe",
    "AF_UNIX",
    "PipeClient",
    "PipeServer",
    "FrameCodec",
    "FrameHeader",
    "read_framed_request",
    "write_framed_response",
];

const AI11_RETIRED_WINDOWS_TRANSPORT_DEPENDENCIES: &[&str] = &[
    "named_pipe",
    "named-pipe",
    "tokio-named-pipes",
    "tokio_named_pipes",
    "windows-named-pipe",
];

#[test]
fn acknowledgement_cannot_restore_a_second_write_pipeline() {
    let root = workspace_root();
    let send = fs::read_to_string(root.join("crates/atm-core/src/send/mod.rs"))
        .expect("canonical write module must be readable");
    let acknowledgement = fs::read_to_string(root.join("crates/atm-core/src/ack/mod.rs"))
        .expect("acknowledgement module must be readable");
    let api = fs::read_to_string(root.join("crates/atm-core/src/api.rs"))
        .expect("transport-neutral API module must be readable");
    let daemon = fs::read_to_string(root.join("crates/atm-daemon/src/runtime_health.rs"))
        .expect("daemon dispatcher module must be readable");

    assert!(
        send.contains("fn write_mail_with_runtime_impl"),
        "AI.7 requires one canonical write pipeline"
    );
    assert!(
        send.contains("resolve_acknowledgement_write"),
        "AI.7 acknowledgement normalization must enter the canonical write pipeline"
    );
    assert!(
        acknowledgement.contains("crate::send::write_mail_with_runtime("),
        "the ack command must invoke the canonical write entry point"
    );
    for retired in [
        "ack_mail_with_runtime_and_post_send_emitter",
        "acknowledge_via_canonical_write",
    ] {
        assert!(
            !acknowledgement.contains(retired),
            "AI.7 forbids the retired separate acknowledgement write path `{retired}`"
        );
    }
    assert!(
        !api.contains("MessageRequest") && !daemon.contains("ApiRequest::Message("),
        "AI.7 forbids a second acknowledgement API/daemon-dispatch variant"
    );
}

#[test]
fn canonical_write_router_has_one_host_routing_decision() {
    let root = workspace_root();
    let mut visitor = HostRoutingVisitor::default();
    for path in canonical_write_modules(&root) {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} must be readable: {error}", path.display()));
        let file = syn::parse_file(&source)
            .unwrap_or_else(|error| panic!("{} must remain valid Rust: {error}", path.display()));
        visitor.source_path = Some(path);
        visitor.visit_file(&file);
    }

    assert_eq!(
        visitor.post_router_host_accesses(),
        1,
        "AI.12 requires PostWriteRouter::dispatch to make the sole host decision"
    );
    assert_eq!(
        visitor.peer_delivery_calls(),
        1,
        "AI.12 requires exactly one peer delivery call from PostWriteRouter::dispatch"
    );
    assert_eq!(
        visitor.message_writer_implementations, 1,
        "AI.12 requires exactly one production MessageWriter implementation"
    );
    assert_eq!(
        visitor.post_write_router_implementations, 1,
        "AI.12 requires exactly one production PostWriteRouter implementation"
    );
    assert!(
        visitor.violations().is_empty(),
        "AI.12 permits host routing, local nudge emission, and peer transport only from PostWriteRouter::dispatch: {:?}",
        visitor.violations()
    );
    let daemon = fs::read_to_string(root.join("crates/atm-daemon/src/runtime_health.rs"))
        .expect("daemon request dispatcher source must be readable");
    assert!(
        !daemon.contains("dispatch_remote_write"),
        "AI.12 forbids the pre-persistence remote write branch"
    );
    let send = fs::read_to_string(root.join("crates/atm-core/src/send/mod.rs"))
        .expect("canonical writer source must be readable");
    for retired in [
        "write_mail_with_runtime_and_post_send_emitter",
        "send_mail_with_runtime_and_post_send_emitter",
        "write_mail_persisted_with_runtime",
    ] {
        assert!(
            !send.contains(retired),
            "AI.12 forbids the pre-router local-nudge helper `{retired}`"
        );
    }
}

#[test]
fn canonical_write_router_rejects_all_mandated_negative_fixtures() {
    for (name, source) in [
        (
            "second writer",
            "impl MessageWriter for First { fn write(&self) { } } impl MessageWriter for Second { fn write(&self) { } }",
        ),
        (
            "second post-write router",
            "impl PostWriteRouter for First { fn dispatch(&self) { } } impl PostWriteRouter for Second { fn dispatch(&self) { } }",
        ),
        (
            "pre-write nudge",
            "fn write() { self.emit_local_post_write(); }",
        ),
        (
            "pre-write peer send",
            "fn write() { transport.deliver_to_peer(); }",
        ),
        (
            "host-routing delivery outside router",
            "impl Pick { fn pick_transport(&self) { let transport = self.https_transport; request.host; transport.deliver(); } }",
        ),
        (
            "aliased local nudge",
            "use nudge::emit_local_post_write as emit; fn write() { emit(); }",
        ),
    ] {
        let violations = routing_violations_in_fixture(source);
        assert!(
            !violations.is_empty(),
            "AI.12 guard must reject the mandated {name} fixture"
        );
    }

    let permitted_admission_check = routing_violations_in_fixture(
        "impl PostWriteRouter for Only { fn dispatch(&self) { } } fn resolve_write_recipient_snapshot() { request.host; snapshot(); }",
    );
    assert!(
        permitted_admission_check.is_empty(),
        "AI.12 permits a host-only persistence admission check: {permitted_admission_check:?}"
    );
}

fn canonical_write_modules(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files(&root.join("crates"), &mut files);
    files
}

fn routing_violations_in_fixture(source: &str) -> Vec<String> {
    let file = syn::parse_file(source).expect("negative fixture must parse");
    let mut visitor = HostRoutingVisitor::default();
    visitor.visit_file(&file);
    visitor.violations()
}

#[derive(Default)]
struct HostRoutingVisitor {
    functions: Vec<HostRoutingFunction>,
    message_writer_implementations: usize,
    post_write_router_implementations: usize,
    delivery_function_aliases: BTreeSet<String>,
    source_path: Option<PathBuf>,
    current_function: Option<usize>,
    in_post_write_router: bool,
}

#[derive(Default)]
struct HostRoutingFunction {
    name: String,
    is_post_write_dispatch: bool,
    is_test: bool,
    accesses_host: bool,
    calls_delivery: bool,
    peer_delivery_calls: usize,
    https_transport_bindings: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for HostRoutingVisitor {
    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let is_message_writer = node.trait_.as_ref().is_some_and(|(_, path, _)| {
            path.segments
                .last()
                .is_some_and(|segment| segment.ident == "MessageWriter")
        });
        let is_post_write_router = node.trait_.as_ref().is_some_and(|(_, path, _)| {
            path.segments
                .last()
                .is_some_and(|segment| segment.ident == "PostWriteRouter")
        });
        if self.is_production_source() {
            self.message_writer_implementations += usize::from(is_message_writer);
            self.post_write_router_implementations += usize::from(is_post_write_router);
        }
        let previous = self.in_post_write_router;
        self.in_post_write_router = is_post_write_router;
        syn::visit::visit_item_impl(self, node);
        self.in_post_write_router = previous;
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        collect_delivery_function_aliases(&node.tree, &mut self.delivery_function_aliases);
        syn::visit::visit_item_use(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let previous = self.begin_function(node.sig.ident.to_string(), &node.attrs);
        syn::visit::visit_impl_item_fn(self, node);
        self.current_function = previous;
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let previous = self.begin_function(node.sig.ident.to_string(), &node.attrs);
        syn::visit::visit_item_fn(self, node);
        self.current_function = previous;
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        if node
            .init
            .as_ref()
            .is_some_and(|init| contains_https_transport_field(&init.expr))
            && let syn::Pat::Ident(binding) = &node.pat
            && let Some(function) = self.current_function_mut()
        {
            function
                .https_transport_bindings
                .insert(binding.ident.to_string());
        }
        syn::visit::visit_local(self, node);
    }

    fn visit_expr_field(&mut self, node: &'ast syn::ExprField) {
        if matches!(&node.member, syn::Member::Named(name) if name == "host")
            && let Some(function) = self.current_function_mut()
        {
            function.accesses_host = true;
        }
        syn::visit::visit_expr_field(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let method = node.method.to_string();
        let peer_delivery = method == "deliver_to_peer"
            || (method == "deliver" && self.is_https_transport_receiver(&node.receiver));
        if (peer_delivery || method == "emit_local_post_write")
            && let Some(function) = self.current_function_mut()
            && !function.is_test
        {
            function.calls_delivery = true;
            if peer_delivery {
                function.peer_delivery_calls += 1;
            }
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if self.is_delivery_function_call(&node.func)
            && let Some(function) = self.current_function_mut()
            && !function.is_test
        {
            function.calls_delivery = true;
        }
        syn::visit::visit_expr_call(self, node);
    }
}

impl HostRoutingVisitor {
    fn begin_function(&mut self, name: String, attrs: &[syn::Attribute]) -> Option<usize> {
        let index = self.functions.len();
        self.functions.push(HostRoutingFunction {
            is_post_write_dispatch: self.in_post_write_router && name == "dispatch",
            is_test: attrs.iter().any(|attr| attr.path().is_ident("test")),
            name,
            ..HostRoutingFunction::default()
        });
        self.current_function.replace(index)
    }

    fn current_function_mut(&mut self) -> Option<&mut HostRoutingFunction> {
        self.current_function
            .and_then(|index| self.functions.get_mut(index))
    }

    fn is_https_transport_receiver(&self, receiver: &syn::Expr) -> bool {
        let syn::Expr::Path(path) = receiver else {
            return false;
        };
        let Some(segment) = path.path.segments.last() else {
            return false;
        };
        self.current_function
            .and_then(|index| self.functions.get(index))
            .is_some_and(|function| {
                function
                    .https_transport_bindings
                    .contains(&segment.ident.to_string())
            })
    }

    fn is_delivery_function_call(&self, function: &syn::Expr) -> bool {
        let syn::Expr::Path(path) = function else {
            return false;
        };
        let Some(segment) = path.path.segments.last() else {
            return false;
        };
        matches!(
            segment.ident.to_string().as_str(),
            "emit_local_post_write" | "deliver_to_peer"
        ) || self
            .delivery_function_aliases
            .contains(&segment.ident.to_string())
    }

    fn post_router_host_accesses(&self) -> usize {
        self.functions
            .iter()
            .filter(|function| function.is_post_write_dispatch && function.accesses_host)
            .count()
    }

    fn peer_delivery_calls(&self) -> usize {
        self.functions
            .iter()
            .filter(|function| function.is_post_write_dispatch)
            .map(|function| function.peer_delivery_calls)
            .sum()
    }

    fn violations(&self) -> Vec<String> {
        let mut violations = self
            .functions
            .iter()
            .filter(|function| function.calls_delivery && !function.is_post_write_dispatch)
            .map(|function| {
                format!(
                    "delivery call outside PostWriteRouter::dispatch in {}",
                    function.name
                )
            })
            .collect::<Vec<_>>();
        if self.message_writer_implementations > 1 {
            violations.push("second MessageWriter implementation".to_string());
        }
        if self.post_write_router_implementations > 1 {
            violations.push("second PostWriteRouter implementation".to_string());
        }
        violations
    }

    fn is_production_source(&self) -> bool {
        self.source_path
            .as_ref()
            .is_none_or(|path| !is_test_only_source(path))
    }
}

fn collect_delivery_function_aliases(tree: &syn::UseTree, aliases: &mut BTreeSet<String>) {
    match tree {
        syn::UseTree::Rename(rename)
            if matches!(
                rename.ident.to_string().as_str(),
                "emit_local_post_write" | "deliver_to_peer"
            ) =>
        {
            aliases.insert(rename.rename.to_string());
        }
        syn::UseTree::Name(name)
            if matches!(
                name.ident.to_string().as_str(),
                "emit_local_post_write" | "deliver_to_peer"
            ) =>
        {
            aliases.insert(name.ident.to_string());
        }
        syn::UseTree::Path(path) => collect_delivery_function_aliases(&path.tree, aliases),
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_delivery_function_aliases(item, aliases);
            }
        }
        syn::UseTree::Glob(_) | syn::UseTree::Rename(_) | syn::UseTree::Name(_) => {}
    }
}

fn contains_https_transport_field(expression: &syn::Expr) -> bool {
    struct HttpsTransportFieldVisitor {
        found: bool,
    }

    impl<'ast> Visit<'ast> for HttpsTransportFieldVisitor {
        fn visit_expr_field(&mut self, node: &'ast syn::ExprField) {
            if matches!(&node.member, syn::Member::Named(name) if name == "https_transport") {
                self.found = true;
            }
            syn::visit::visit_expr_field(self, node);
        }
    }

    let mut visitor = HttpsTransportFieldVisitor { found: false };
    visitor.visit_expr(expression);
    visitor.found
}

#[test]
fn production_code_cannot_restore_retired_error_contract_symbols() {
    let root = workspace_root().join("crates");
    let mut files = Vec::new();
    collect_rust_files(&root, &mut files);
    let violations = files
        .into_iter()
        .filter(|path| !is_error_contract_guard_fixture(path))
        .filter_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            retired_error_contract_symbol(&contents)
                .map(|symbol| format!("{} contains retired `{symbol}`", path.display()))
        })
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "AI.3's two-field error contract forbids retired error shapes: {violations:?}"
    );
}

fn retired_error_contract_symbol(source: &str) -> Option<&'static str> {
    RETIRED_ERROR_CONTRACT_SYMBOLS
        .iter()
        .copied()
        .find(|symbol| source.contains(symbol))
}

fn is_error_contract_guard_fixture(path: &Path) -> bool {
    path == workspace_root().join("crates/atm-architecture/tests/boundary_enforcement.rs")
}

#[test]
fn retired_error_contract_detector_rejects_duplicate_code_mapping_fixture() {
    assert_eq!(
        retired_error_contract_symbol("fn error_kind_for_code() {}"),
        Some("error_kind_for_code")
    );
}

#[test]
fn retired_error_contract_guard_exempts_only_its_own_fixture() {
    let root = workspace_root();
    assert!(is_error_contract_guard_fixture(
        &root.join("crates/atm-architecture/tests/boundary_enforcement.rs")
    ));
    assert!(!is_error_contract_guard_fixture(
        &root.join("crates/atm-core/tests/error_contract_regression.rs")
    ));
}

#[test]
fn only_the_error_contract_module_may_define_an_atm_error_literal() {
    let root = workspace_root();
    let contract_module = root.join("crates/atm-storage/src/error.rs");
    let source_root = root.join("crates");
    let mut files = Vec::new();
    collect_rust_files(&source_root, &mut files);
    let violations = files
        .into_iter()
        .filter(|path| path != &contract_module)
        .filter_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            direct_atm_error_literal(&contents).then(|| path.display().to_string())
        })
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "AtmError literals bypass the canonical error catalog: {violations:?}"
    );
}

fn direct_atm_error_literal(source: &str) -> bool {
    source.split("AtmError").skip(1).any(|suffix| {
        let Some(body) = suffix.trim_start().strip_prefix('{') else {
            return false;
        };
        matches!(body.trim_start(), body if body.starts_with("code:") || body.starts_with("message:"))
    })
}

#[test]
fn direct_atm_error_literal_detector_rejects_constructor_fixture() {
    let fixture = concat!("Atm", "Error", " {", " code: error_code, message: detail }");
    assert!(direct_atm_error_literal(fixture));
    assert!(!direct_atm_error_literal(
        "fn error() -> AtmError { panic!() }"
    ));
}

#[test]
fn atm_daemon_must_not_depend_on_atm_storage_rusqlite() {
    assert_forbidden_edge_absent("atm-daemon", "atm-storage-rusqlite");
}

#[test]
fn atm_must_not_depend_on_atm_storage_rusqlite() {
    assert_forbidden_edge_absent("atm", "atm-storage-rusqlite");
}

#[test]
fn atm_runtime_must_not_depend_on_atm_daemon() {
    assert_forbidden_edge_absent("atm-runtime", "atm-daemon");
}

#[test]
fn atm_runtime_must_not_depend_on_atm_storage_rusqlite() {
    assert_forbidden_edge_absent("atm-runtime", "atm-storage-rusqlite");
}

#[test]
fn atm_storage_rusqlite_must_not_depend_on_atm_runtime() {
    assert_forbidden_edge_absent("atm-storage-rusqlite", "atm-runtime");
}

#[test]
fn atm_graft_must_not_depend_on_atm_storage_rusqlite() {
    assert_forbidden_edge_absent("atm-graft", "atm-storage-rusqlite");
}

#[test]
fn boundary_toml_forbidden_edges_match_rust_guard_catalog() {
    let expected = expected_edge_set();
    let documented = documented_forbidden_edges();
    let missing = missing_forbidden_edges(&expected, &documented);
    let unexpected = missing_forbidden_edges(&documented, &expected);
    assert!(
        missing.is_empty() && unexpected.is_empty(),
        "boundary TOML forbidden_edges drifted from the Rust architecture guard; missing: {missing:?}; unexpected: {unexpected:?}"
    );
}

#[test]
fn daemon_boundary_tomls_must_not_allow_atm_storage_rusqlite() {
    let violations = daemon_boundary_files()
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let boundary: BoundaryToml = toml::from_str(&contents)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
            boundary
                .dependencies
                .allowed_dependencies
                .contains(&"atm-storage-rusqlite".to_string())
                .then(|| path.display().to_string())
        })
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "daemon boundary TOMLs must not allow atm-storage-rusqlite directly; violating files: {violations:?}"
    );
}

#[test]
fn synthetic_daemon_boundary_relaxation_fixture_is_detected() {
    let fixture = BoundaryToml {
        dependencies: BoundaryDependencies {
            allowed_dependencies: vec![
                "atm-storage".to_string(),
                "atm-storage-rusqlite".to_string(),
            ],
            ..BoundaryDependencies::default()
        },
        ..BoundaryToml::default()
    };

    assert!(
        fixture
            .dependencies
            .allowed_dependencies
            .contains(&"atm-storage-rusqlite".to_string()),
        "synthetic fixture must demonstrate the daemon TOML relock would fail closed if atm-storage-rusqlite were re-added"
    );
}

#[test]
fn synthetic_boundary_relaxation_fixture_reports_removed_forbidden_edge() {
    // Synthetic fixture proving the guard will fail closed if a forbidden edge
    // is relaxed out of the Rust catalog or the boundary TOMLs.
    let mut relaxed = expected_edge_set();
    relaxed.remove(&("atm-daemon".to_string(), "atm-storage-rusqlite".to_string()));

    let missing = missing_forbidden_edges(&expected_edge_set(), &relaxed);
    assert_eq!(
        missing,
        vec!["atm-daemon -> atm-storage-rusqlite".to_string()],
        "synthetic relaxation fixture must demonstrate the removed daemon/sqlite edge is detected"
    );
}

#[test]
fn workspace_source_must_not_reintroduce_retired_peer_delivery_constructs() {
    let source_root = workspace_root().join("crates");
    let mut findings = Vec::new();
    let mut files = Vec::new();
    collect_rust_files(&source_root, &mut files);
    for path in files {
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for (prefix, suffix) in RETIRED_DAEMON_CONSTRUCT_FRAGMENTS {
            let construct = format!("{prefix}{suffix}");
            if contents.contains(&construct) {
                findings.push(format!("{}: {construct}", path.display()));
            }
        }
    }
    assert!(
        findings.is_empty(),
        "retired peer-delivery constructs must not re-enter workspace Rust source: {findings:?}"
    );
}

#[test]
fn ai11_deletion_gate_rejects_retired_windows_transport_ast_and_dependencies() {
    let root = workspace_root();
    let daemon_lib = root.join("crates/atm-daemon/src/lib.rs");
    let local_tcp = root.join("crates/atm-daemon/src/local_tcp_transport.rs");
    let local_ipc = root.join("crates/atm-daemon/src/local_ipc_transport.rs");
    let peer_https = root.join("crates/atm-daemon/src/https_transport.rs");

    let daemon_lib_source = read_source(&daemon_lib);
    assert!(
        daemon_lib_source.contains("#[cfg(not(windows))]")
            && daemon_lib_source.contains("mod local_ipc_transport;"),
        "AI.11 permits UDS only behind the Unix-only local_ipc_transport module gate"
    );
    assert!(
        daemon_lib_source.contains("#[cfg(windows)]")
            && daemon_lib_source
                .contains("pub(crate) use local_tcp_transport::LocalIpcServerTransportAdapter;"),
        "Windows must select the loopback-TCP local transport adapter"
    );

    let retired = ai11_guarded_workspace_sources(&root)
        .iter()
        .flat_map(|path| retired_windows_transport_ast_findings(path))
        .collect::<Vec<_>>();
    assert!(
        retired.is_empty(),
        "AI.11 must not restore Windows pipe/AF_UNIX or generic frame-codec transport: {retired:?}"
    );

    let metadata = MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .expect("cargo metadata must succeed for the workspace");
    let forbidden_dependencies = metadata
        .packages
        .into_iter()
        .filter(|package| {
            matches!(
                package.name.as_str(),
                "atm" | "atm-core" | "atm-daemon" | "atm-daemon-client"
            )
        })
        .flat_map(|package| {
            package
                .dependencies
                .into_iter()
                .filter_map(move |dependency| {
                    AI11_RETIRED_WINDOWS_TRANSPORT_DEPENDENCIES
                        .contains(&dependency.name.as_str())
                        .then(|| {
                            format!("{} directly depends on {}", package.name, dependency.name)
                        })
                })
        })
        .collect::<Vec<_>>();
    assert!(
        forbidden_dependencies.is_empty(),
        "AI.11 must not restore retired Windows transport dependencies: {forbidden_dependencies:?}"
    );

    let local_tcp_source = read_source(&local_tcp);
    let non_loopback_binds = local_tcp_source
        .lines()
        .filter(|line| line.contains("TcpListener::bind"))
        .filter(|line| !line.contains("Ipv4Addr::LOCALHOST"))
        .collect::<Vec<_>>();
    assert!(
        non_loopback_binds.is_empty(),
        "AI.11 local TCP listeners must bind only IPv4 loopback: {non_loopback_binds:?}"
    );
    let adapter_sources = [
        ("local TCP", read_source(&local_tcp)),
        ("local IPC", read_source(&local_ipc)),
        ("peer HTTPS", read_source(&peer_https)),
    ];
    for forbidden in [
        "LocalServiceRuntime",
        "persist_message",
        "emit_post_send_effects",
        "write_mail_with_runtime",
    ] {
        for (adapter, source) in &adapter_sources {
            assert!(
                !source.contains(forbidden),
                "{adapter} adapter must not call storage/write/nudge code directly: `{forbidden}`"
            );
        }
    }

    let router_implementations = ai11_guarded_workspace_sources(&root)
        .iter()
        .filter(|path| !is_test_only_source(path))
        .map(|path| production_api_router_implementation_count(path))
        .sum::<usize>();
    assert_eq!(
        router_implementations, 1,
        "AI.11 requires exactly one production ApiRouter implementation"
    );
}

#[test]
fn ai11_deletion_gate_detector_rejects_retired_windows_transport_ast_fixtures() {
    let fixture = syn::parse_file(
        r#"
        use windows::named_pipe::NamedPipe;
        const ENDPOINT: &str = r"\\.\pipe\atm";
        const DOMAIN: i32 = AF_UNIX;
        "#,
    )
    .expect("fixture must parse");
    let mut detector = RetiredWindowsTransportDetector::default();
    detector.visit_file(&fixture);
    assert_eq!(
        detector.findings,
        BTreeSet::from([
            "identifier `AF_UNIX`".to_string(),
            "identifier `NamedPipe`".to_string(),
            "identifier `named_pipe`".to_string(),
            "named-pipe endpoint literal".to_string(),
        ])
    );
}

#[test]
fn ai11_deletion_gate_detector_rejects_retired_envelope_wire_codec_ast_fixtures() {
    let fixture = syn::parse_file(
        r#"
        struct FrameHeader { length: u32 }
        struct FrameCodec;
        fn read_framed_request() {}
        fn write_framed_response() {}
        "#,
    )
    .expect("fixture must parse");
    let mut detector = RetiredWindowsTransportDetector::default();
    detector.visit_file(&fixture);
    assert_eq!(
        detector.findings,
        BTreeSet::from([
            "identifier `FrameCodec`".to_string(),
            "identifier `FrameHeader`".to_string(),
            "identifier `read_framed_request`".to_string(),
            "identifier `write_framed_response`".to_string(),
        ])
    );
}

#[test]
fn deleted_daemon_boundary_modules_must_be_retired() {
    let root = workspace_root();
    let stale = daemon_boundary_files()
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let boundary: BoundaryToml = toml::from_str(&contents)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
            let sources = daemon_boundary_module_sources(&root, &boundary.implementation.module)?;
            let source_exists = sources.iter().any(|source| source.exists());
            module_is_stale_if_missing(source_exists, &boundary.status.state).then(|| {
                format!(
                    "{} declares missing module {} with status {}",
                    path.display(),
                    boundary.implementation.module,
                    boundary.status.state
                )
            })
        })
        .collect::<Vec<_>>();

    assert!(
        stale.is_empty(),
        "daemon boundary TOMLs for deleted modules must be retired: {stale:?}"
    );
}

#[test]
fn deleted_core_boundary_modules_must_be_retired() {
    let root = workspace_root();
    let stale = core_boundary_files()
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let boundary: BoundaryToml = toml::from_str(&contents)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
            let sources = boundary_module_sources(&root, &boundary)?;
            let source_exists = sources.iter().any(|source| source.exists());
            module_is_stale_if_missing(source_exists, &boundary.status.state).then(|| {
                format!(
                    "{} declares missing module {} with status {}",
                    path.display(),
                    boundary.implementation.module,
                    boundary.status.state
                )
            })
        })
        .collect::<Vec<_>>();

    assert!(
        stale.is_empty(),
        "core boundary TOMLs for deleted modules must be retired: {stale:?}"
    );
}

#[test]
fn retired_core_boundary_records_must_not_name_live_dependents_and_must_be_historical_docs() {
    let docs = fs::read_to_string(workspace_root().join("docs/atm-core/boundaries.md"))
        .expect("docs/atm-core/boundaries.md must be readable");
    let stale = core_boundary_files()
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let boundary: BoundaryToml = toml::from_str(&contents)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
            if boundary.status.state != "retired" {
                return None;
            }

            let mut failures = Vec::new();
            if !boundary.dependencies.allowed_dependents.is_empty() {
                failures.push(format!(
                    "lists live dependents {:?}",
                    boundary.dependencies.allowed_dependents
                ));
            }
            let section = documented_boundary_section(&docs, &boundary.name);
            if section.is_none_or(|section| {
                !(section.contains("Historical status:")
                    || section.contains("Historical boundary record"))
            }) {
                failures.push("is not documented as historical".to_string());
            }

            (!failures.is_empty()).then(|| format!("{}: {}", path.display(), failures.join("; ")))
        })
        .collect::<Vec<_>>();

    assert!(
        stale.is_empty(),
        "retired atm-core boundary records must not name live callers and must be documented as historical: {stale:?}"
    );
}

#[test]
fn missing_daemon_module_requires_retired_boundary_state() {
    assert!(module_is_stale_if_missing(false, "active"));
    assert!(!module_is_stale_if_missing(false, "retired"));
    assert!(!module_is_stale_if_missing(false, "planned"));
}

#[test]
fn missing_core_module_requires_retired_boundary_state() {
    assert!(module_is_stale_if_missing(false, "active"));
    assert!(!module_is_stale_if_missing(false, "retired"));
    assert!(!module_is_stale_if_missing(false, "planned"));
}

#[test]
fn bare_daemon_boundary_module_resolves_crate_entry_points() {
    let root = workspace_root();
    let sources = daemon_boundary_module_sources(&root, "atm_daemon")
        .expect("atm_daemon must resolve to daemon crate entry points");

    assert_eq!(
        sources,
        vec![
            root.join("crates/atm-daemon/src/lib.rs"),
            root.join("crates/atm-daemon/src/main.rs"),
        ]
    );
    assert!(sources.iter().any(|source| source.exists()));
}

#[test]
fn retired_bare_daemon_boundary_records_are_checked_against_entry_points() {
    let root = workspace_root();
    for file_name in [
        "daemon-reconcile-coordinator.toml",
        "file-watch-event-source.toml",
    ] {
        let path = root.join("boundaries/atm-daemon").join(file_name);
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let boundary: BoundaryToml = toml::from_str(&contents)
            .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
        let sources = daemon_boundary_module_sources(&root, &boundary.implementation.module)
            .unwrap_or_else(|| panic!("{} must resolve a daemon module", path.display()));

        assert_eq!(boundary.implementation.module, "atm_daemon");
        assert!(sources.iter().any(|source| source.exists()));
        assert!(!module_is_stale_if_missing(
            sources.iter().any(|source| source.exists()),
            &boundary.status.state
        ));
    }
}

fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
    {
        let path = entry
            .unwrap_or_else(|error| panic!("failed to read directory entry: {error}"))
            .path();
        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

fn ai11_guarded_workspace_sources(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files(&root.join("crates"), &mut files);
    files.retain(|path| path != &ai11_deletion_gate_fixture_path(root));
    files.sort();
    files
}

fn ai11_deletion_gate_fixture_path(root: &Path) -> PathBuf {
    root.join("crates/atm-architecture/tests/boundary_enforcement.rs")
}

fn is_test_only_source(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "tests")
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("test_"))
}

fn production_api_router_implementation_count(path: &Path) -> usize {
    let source = read_source(path);
    let syntax = syn::parse_file(&source)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
    let mut detector = ProductionApiRouterImplementationDetector::default();
    detector.visit_file(&syntax);
    detector.count
}

#[derive(Default)]
struct ProductionApiRouterImplementationDetector {
    count: usize,
}

impl<'ast> Visit<'ast> for ProductionApiRouterImplementationDetector {
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if item.trait_.as_ref().is_some_and(|(_, path, _)| {
            path.segments
                .last()
                .is_some_and(|segment| segment.ident == "ApiRouter")
        }) {
            self.count += 1;
        }
        syn::visit::visit_item_impl(self, item);
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if item.attrs.iter().any(is_test_configuration_attribute) {
            return;
        }
        syn::visit::visit_item_mod(self, item);
    }
}

fn is_test_configuration_attribute(attribute: &syn::Attribute) -> bool {
    attribute.path().is_ident("cfg")
        && attribute
            .meta
            .require_list()
            .is_ok_and(|list| list.tokens.to_string().contains("test"))
}

fn daemon_boundary_module_sources(root: &Path, module: &str) -> Option<Vec<PathBuf>> {
    let relative_module = module.strip_prefix("atm_daemon")?;
    let source_root = root.join("crates/atm-daemon/src");
    if relative_module.is_empty() {
        return Some(vec![
            source_root.join("lib.rs"),
            source_root.join("main.rs"),
        ]);
    }

    let relative_module = relative_module.strip_prefix("::")?;
    let module_path = source_root.join(relative_module.replace("::", "/"));
    Some(vec![
        module_path.with_extension("rs"),
        module_path.join("mod.rs"),
    ])
}

fn boundary_module_sources(root: &Path, boundary: &BoundaryToml) -> Option<Vec<PathBuf>> {
    let module = boundary.implementation.module.trim();
    if module.is_empty() {
        if boundary.implementation.visibility == "trait_only" {
            return Some(core_contract_trait_sources(
                root,
                &boundary.public.trait_name,
            ));
        }
        return Some(Vec::new());
    }

    let crate_path = boundary.owner_crate_path.trim();
    if crate_path.is_empty() {
        return Some(Vec::new());
    }
    let crate_source_root = root
        .join("crates")
        .join(crate_path.replace('_', "-"))
        .join("src");
    let relative_module = module
        .strip_prefix(crate_path)
        .and_then(|value| value.strip_prefix("::"))
        .unwrap_or(module);
    if relative_module == module && module != crate_path {
        return Some(Vec::new());
    }
    if relative_module.is_empty() {
        return Some(vec![
            crate_source_root.join("lib.rs"),
            crate_source_root.join("main.rs"),
        ]);
    }

    let module_path = crate_source_root.join(relative_module.replace("::", "/"));
    Some(vec![
        module_path.with_extension("rs"),
        module_path.join("mod.rs"),
    ])
}

fn core_contract_trait_sources(root: &Path, trait_name: &str) -> Vec<PathBuf> {
    if trait_name.trim().is_empty() {
        return Vec::new();
    }
    let needle = format!("pub trait {}", trait_name.trim());
    let mut files = Vec::new();
    collect_rust_files(&root.join("crates/atm-core/src/boundary"), &mut files);
    files
        .into_iter()
        .filter(|path| {
            fs::read_to_string(path)
                .map(|contents| contents.contains(&needle))
                .unwrap_or(false)
        })
        .collect()
}

fn module_is_stale_if_missing(source_exists: bool, state: &str) -> bool {
    !source_exists && !matches!(state, "retired" | "planned")
}

fn assert_forbidden_edge_absent(source: &str, forbidden: &str) {
    let dependencies = direct_normal_workspace_dependencies();
    let actual = dependencies.get(source).cloned().unwrap_or_default();
    assert!(
        !actual.contains(forbidden),
        "{source} must not have a normal workspace dependency on {forbidden}; actual workspace deps: {actual:?}"
    );
}
fn direct_normal_workspace_dependencies() -> BTreeMap<String, BTreeSet<String>> {
    let metadata = MetadataCommand::new()
        .manifest_path(workspace_root().join("Cargo.toml"))
        .no_deps()
        .exec()
        .expect("cargo metadata must succeed for the workspace");

    let workspace_package_names = metadata
        .packages
        .iter()
        .map(|package| package.name.to_string())
        .collect::<BTreeSet<_>>();

    metadata
        .packages
        .into_iter()
        .map(|package| {
            let deps = package
                .dependencies
                .into_iter()
                .filter(|dependency| dependency.kind == DependencyKind::Normal)
                .filter(|dependency| {
                    workspace_package_names.contains::<str>(dependency.name.as_ref())
                })
                .map(|dependency| dependency.name.to_string())
                .collect::<BTreeSet<_>>();
            (package.name.to_string(), deps)
        })
        .collect()
}

fn documented_forbidden_edges() -> BTreeSet<(String, String)> {
    guarded_boundary_files()
        .into_iter()
        .flat_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let boundary: BoundaryToml = toml::from_str(&contents)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
            boundary
                .dependencies
                .forbidden_edges
                .into_iter()
                .map(|edge| {
                    let (source, target) = edge.split_once(" -> ").unwrap_or_else(|| {
                        panic!("invalid forbidden edge `{edge}` in {}", path.display())
                    });
                    (source.to_string(), target.to_string())
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn expected_edge_set() -> BTreeSet<(String, String)> {
    EXPECTED_FORBIDDEN_EDGES
        .iter()
        .map(|(source, target)| (source.to_string(), target.to_string()))
        .collect()
}

fn missing_forbidden_edges(
    expected: &BTreeSet<(String, String)>,
    actual: &BTreeSet<(String, String)>,
) -> Vec<String> {
    expected
        .difference(actual)
        .map(|(source, target)| format!("{source} -> {target}"))
        .collect()
}

fn guarded_boundary_files() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files = vec![
        root.join("boundaries/atm-graft/shared-client-consumer.toml"),
        root.join("boundaries/atm-runtime/runtime-composition.toml"),
    ];
    let mut sqlite_files = fs::read_dir(root.join("boundaries/atm-storage-rusqlite"))
        .expect("boundaries/atm-storage-rusqlite directory must be readable")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    sqlite_files.sort();
    files.extend(sqlite_files);
    files
}

fn daemon_boundary_files() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files = fs::read_dir(root.join("boundaries/atm-daemon"))
        .expect("boundaries/atm-daemon directory must be readable")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn core_boundary_files() -> Vec<PathBuf> {
    boundary_files_in("atm-core")
}

fn boundary_files_in(owner: &str) -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files = fs::read_dir(root.join("boundaries").join(owner))
        .unwrap_or_else(|error| panic!("boundaries/{owner} directory must be readable: {error}"))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn documented_boundary_section<'a>(docs: &'a str, name: &str) -> Option<&'a str> {
    let start_marker = format!("## {}", name);
    let start = docs.find(&start_marker)?;
    let rest = &docs[start..];
    let next = rest
        .match_indices("\n## ")
        .next()
        .map(|(index, _)| index)
        .unwrap_or(rest.len());
    Some(&rest[..next])
}

fn read_source(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

fn retired_windows_transport_ast_findings(path: &Path) -> Vec<String> {
    let source = read_source(path);
    let syntax = syn::parse_file(&source)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
    let mut detector = RetiredWindowsTransportDetector::default();
    detector.visit_file(&syntax);
    detector
        .findings
        .into_iter()
        .map(|finding| format!("{}: {finding}", path.display()))
        .collect()
}

#[derive(Default)]
struct RetiredWindowsTransportDetector {
    findings: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for RetiredWindowsTransportDetector {
    fn visit_ident(&mut self, ident: &'ast syn::Ident) {
        let value = ident.to_string();
        if AI11_RETIRED_WINDOWS_TRANSPORT_IDENTIFIERS.contains(&value.as_str()) {
            self.findings.insert(format!("identifier `{value}`"));
        }
        syn::visit::visit_ident(self, ident);
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        if literal.value().contains(r"\\.\pipe\") {
            self.findings
                .insert("named-pipe endpoint literal".to_string());
        }
        syn::visit::visit_lit_str(self, literal);
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("atm-architecture must live under crates/atm-architecture")
        .to_path_buf()
}

#[derive(Default, Deserialize)]
struct BoundaryToml {
    #[serde(default)]
    name: String,
    #[serde(default)]
    owner_crate_path: String,
    #[serde(default)]
    public: BoundaryPublic,
    #[serde(default)]
    implementation: BoundaryImplementation,
    #[serde(default)]
    status: BoundaryStatus,
    #[serde(default)]
    dependencies: BoundaryDependencies,
}

#[derive(Default, Deserialize)]
struct BoundaryPublic {
    #[serde(default, rename = "trait")]
    trait_name: String,
}

#[derive(Default, Deserialize)]
struct BoundaryImplementation {
    #[serde(default)]
    visibility: String,
    #[serde(default)]
    module: String,
}

#[derive(Default, Deserialize)]
struct BoundaryStatus {
    #[serde(default)]
    state: String,
}

#[derive(Default, Deserialize)]
struct BoundaryDependencies {
    #[serde(default)]
    allowed_dependents: Vec<String>,
    #[serde(default)]
    allowed_dependencies: Vec<String>,
    #[serde(default)]
    forbidden_edges: Vec<String>,
}
