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
    ("atm", "atm-daemon"),
    ("atm", "atm-storage-rusqlite"),
    ("atm-daemon", "atm-runtime"),
    ("atm-daemon", "atm-storage-rusqlite"),
    ("atm-runtime", "atm-storage-rusqlite"),
    ("atm-storage", "atm-core"),
    ("atm-storage", "atm-storage-rusqlite"),
    ("atm-storage-rusqlite", "atm-runtime"),
    ("atm-graft", "atm-daemon"),
    ("atm-graft", "atm-daemon-bootstrap"),
    ("atm-graft", "atm-storage-rusqlite"),
    ("atm-graft", "interprocess"),
    ("atm-daemon-bootstrap", "atm-graft"),
    ("atm-http-runtime", "atm"),
    ("atm-http-runtime", "atm-daemon-bootstrap"),
    ("atm-http-runtime", "atm-graft"),
    ("atm-http-runtime", "atm-storage-rusqlite"),
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
fn daemon_must_not_read_caller_workspace_config() {
    let root = workspace_root();
    let composition = read_source(&root.join("crates/atm-daemon/src/composition.rs"));
    assert!(
        composition.contains("runtime_assembly.for_daemon()"),
        "daemon composition must select the runtime view that disables caller workspace config"
    );
    let runtime_composition = read_source(&root.join("crates/atm-runtime/src/composition.rs"));
    assert!(
        runtime_composition.contains(
            "self.doctor_ports = runtime_doctor_ports(Arc::new(RuntimeConfigDoctor::default()));"
        ),
        "the daemon runtime view must replace the caller-workspace config doctor"
    );

    let mut files = Vec::new();
    collect_rust_files(&root.join("crates/atm-daemon/src"), &mut files);
    let findings: Vec<_> = files
        .into_iter()
        .filter_map(|path| {
            let source = read_source(&path);
            (source.contains("config::load_config")
                || source.contains("ConfigIngress")
                || source.contains("load_workspace_config"))
            .then(|| path.display().to_string())
        })
        .collect();
    assert!(
        findings.is_empty(),
        "daemon source must not restore caller workspace config access: {findings:?}"
    );
}

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
        send.contains("crate::ack::admit_acknowledgement_write"),
        "AI.31 acknowledgement admission must enter the canonical write pipeline"
    );
    assert!(
        acknowledgement.contains("runtime.acknowledge_message_atomically"),
        "AI.31 acknowledgement source resolution and paired commit must stay behind the sealed storage boundary"
    );
    assert!(
        !acknowledgement.contains("resolve_acknowledgement_source"),
        "AI.31 forbids restoring an application-layer acknowledgement source read"
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
fn ai23_write_ingress_has_one_http_resource_and_no_adapter_side_effects() {
    let root = workspace_root();
    let api = read_source(&root.join("crates/atm-core/src/api.rs"));
    assert!(
        api.contains("HttpRouteKind::Write")
            && api.contains("path_template: MESSAGES_PATH")
            && api.contains("const MESSAGES_PATH: &str = \"/v1/atm/messages\";")
            && api.contains("fn route_kind_for_http"),
        "AI.23 requires send and ACK to select the one POST /v1/atm/messages resource"
    );
    assert!(
        !api.contains("is_ack_path") && !api.contains("/ack\""),
        "AI.23 forbids an acknowledgement-specific HTTP resource"
    );

    for (adapter, path) in [
        (
            "local IPC",
            "crates/atm-daemon/src/local_ipc_transport/request_worker.rs",
        ),
        ("local TCP", "crates/atm-daemon/src/local_tcp_transport.rs"),
    ] {
        let source = read_source(&root.join(path));
        assert!(
            source.contains(".route("),
            "AI.23 {adapter} adapter must enter the shared ApiRouter"
        );
        for forbidden in [
            "PostWriteRouter",
            "MessageWriter",
            "persist_",
            "prepare_write",
            "emit_local_post_write",
            "DaemonPostSend",
        ] {
            assert!(
                !source.contains(forbidden),
                "AI.23 {adapter} adapter must not own write persistence or post-write side effects: `{forbidden}`"
            );
        }
    }
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
        visitor.collect_delivery_function_aliases(&file);
        visitor.visit_file(&file);
    }

    assert_eq!(
        visitor.post_router_host_accesses(),
        1,
        "AI.12 requires PostWriteRouter::dispatch to make the sole host decision"
    );
    assert_eq!(
        visitor.peer_delivery_calls(),
        0,
        "AI.31 forbids peer delivery from PostWriteRouter::dispatch"
    );
    assert_eq!(
        visitor.message_writer_implementations, 1,
        "AI.12 requires exactly one production MessageWriter implementation"
    );
    assert_eq!(
        visitor.post_write_router_implementations, 1,
        "AI.12 requires exactly one production PostWriteRouter implementation"
    );
    assert_eq!(
        visitor.reconciliation_delivery_calls(),
        0,
        "AK.2 deletes the reconciliation delivery callsite"
    );
    assert!(
        visitor.violations().is_empty(),
        "AI.31 permits host routing and work signalling but forbids foreground peer transport: {:?}",
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
fn ai23_peer_adapter_never_matches_localhost_or_own_ip() {
    let root = workspace_root();
    let router_path = root.join("crates/atm-daemon/src/runtime_health/peer_delivery_router.rs");
    let source = read_source(&router_path);
    let file = syn::parse_file(&source).unwrap_or_else(|error| {
        panic!("{} must remain valid Rust: {error}", router_path.display())
    });
    let mut visitor = HostRoutingVisitor::default();
    visitor.visit_file(&file);
    assert!(
        visitor
            .functions
            .iter()
            .any(|function| function.is_post_write_dispatch),
        "AI.23 requires the production PostWriteRouter::dispatch function"
    );

    let dispatch_start = source
        .find("fn dispatch(")
        .expect("the production PostWriteRouter::dispatch function must remain explicit");
    let dispatch = &source[dispatch_start..];
    let peer_receipt_guard = dispatch
        .find("message.prepared.is_peer_receipt()")
        .expect("the generic local/peer routing guard must handle peer receipts");
    let host_guard = dispatch
        .find(".and_then(|address| address.host())")
        .expect("the generic local/peer routing guard must inspect an optional host");
    let peer_branch = dispatch
        .find("Host-qualified origin writes are durable immutable records only")
        .expect("AK.2 must explicitly return after host-qualified persistence");
    assert!(
        peer_receipt_guard < peer_branch && host_guard < peer_branch,
        "peer receipts and host-qualified origin writes must share the one generic input router"
    );
    assert!(
        !dispatch.contains("PeerDelivery") && !dispatch.contains("signal_after_persist"),
        "AK.2 forbids a peer worker signal after local admission"
    );
    for forbidden in ["localhost", "127.0.0.1", "is_loopback", "is_loopback()"] {
        assert!(
            !source.contains(forbidden),
            "AI.23 forbids a dedicated loopback/own-IP production branch: `{forbidden}`"
        );
    }

    let negative_source = "fn dispatch() { if host.is_loopback() { local(); } else { resolve_peer_authority(host); } }";
    syn::parse_file(negative_source).expect("negative loopback fixture must parse");
    assert!(
        negative_source.contains("is_loopback"),
        "the structural test must be able to identify a forbidden loopback branch"
    );
}

#[test]
fn ak2_peer_worker_symbols_are_absent_from_production() {
    let root = workspace_root();
    for deleted_module in [
        "crates/atm-daemon/src/peer_drain_coordinator.rs",
        "crates/atm-daemon/src/peer_delivery_observability.rs",
        "crates/atm-daemon/src/https_transport.rs",
    ] {
        assert!(
            !root.join(deleted_module).exists(),
            "AK.2 must not retain retired peer-worker module `{deleted_module}`"
        );
    }

    let production_sources = [
        "crates/atm-daemon/src/lib.rs",
        "crates/atm-daemon/src/composition.rs",
        "crates/atm-daemon/src/runtime_health.rs",
        "crates/atm-daemon/src/runtime_health/post_commit_work.rs",
        "crates/atm-daemon/src/runtime_health/peer_delivery_router.rs",
        "crates/atm-core/src/api.rs",
        "crates/atm-core/src/protocol.rs",
        "crates/atm/src/commands/peer.rs",
        "crates/atm/src/composition.rs",
        "crates/atm-storage/src/contract.rs",
        "crates/atm-storage-rusqlite/src/peer_config_store.rs",
    ];
    let retired_symbols = [
        "PeerDeliveryCoordinator",
        "PeerDrainCoordinator",
        "PeerPostCommitWorkQueue",
        "PostCommitWorkKey::PeerDelivery",
        "PeerSyncPolicy",
        "PeerSyncRequest",
        "PeerSyncOutcome",
        "PeerLinkStatus",
        "PeerWireSecurity",
        "HttpsTransport",
    ];
    for source in production_sources {
        let contents = read_source(&root.join(source));
        for symbol in retired_symbols {
            assert!(
                !contents.contains(symbol),
                "AK.2 production source `{source}` must not retain `{symbol}`"
            );
        }
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
        (
            "local delivery function binding",
            "fn write() { let emit = emit_local_post_write; emit(); }",
        ),
        (
            "transitive local delivery function binding",
            "fn write() { let first = emit_local_post_write; let second = first; second(); }",
        ),
        (
            "boxed delivery function binding",
            "fn write() { let emit = Box::new(emit_local_post_write); emit(); }",
        ),
        (
            "parenthesized delivery function binding",
            "fn write() { let emit = (emit_local_post_write); emit(); }",
        ),
        (
            "block-wrapped delivery function binding",
            "fn write() { let emit = { emit_local_post_write }; emit(); }",
        ),
        (
            "method-wrapped delivery function binding",
            "fn write() { let emit = emit_local_post_write.clone(); emit(); }",
        ),
        (
            "tuple-field delivery function binding",
            "fn write() { let pair = (emit_local_post_write,); let emit = pair.0; emit(); }",
        ),
        (
            "closure-call delivery function binding",
            "fn write() { let emit = (|| emit_local_post_write)(); emit(); }",
        ),
        (
            "borrowed delivery function binding",
            "fn write() { let emit = &emit_local_post_write; emit(); }",
        ),
        (
            "cast delivery function binding",
            "fn write() { let emit = emit_local_post_write as fn(); emit(); }",
        ),
        (
            "transitive delivery function alias",
            "use nudge::emit_local_post_write as step_one; use step_one as step_two; fn write() { step_two(); }",
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

#[test]
fn ai23_ingress_adapters_cannot_own_write_side_effects() {
    let root = workspace_root();
    for relative in [
        "crates/atm-daemon/src/local_tcp_transport.rs",
        "crates/atm-daemon/src/local_ipc_transport/request_worker.rs",
    ] {
        let path = root.join(relative);
        let source = read_source(&path);
        let file = syn::parse_file(&source)
            .unwrap_or_else(|error| panic!("{} must remain valid Rust: {error}", path.display()));
        let mut visitor = IngressWriteSideEffectVisitor::default();
        visitor.visit_file(&file);
        assert!(
            visitor.findings.is_empty(),
            "AI.23 ingress adapter {relative} may authenticate/decode then call ApiRouter only; it must not own write side effects: {:?}",
            visitor.findings
        );
    }

    let fixture = syn::parse_file(
        "impl MessageWriter for Bad { fn write(&self) {} } fn ingress() { persist_message(); emit_local_post_write(); route_write(); }",
    )
    .expect("negative fixture must parse");
    let mut visitor = IngressWriteSideEffectVisitor::default();
    visitor.visit_file(&fixture);
    assert_eq!(
        visitor.findings,
        BTreeSet::from([
            "MessageWriter implementation".to_string(),
            "direct `emit_local_post_write` call".to_string(),
            "direct `persist_message` call".to_string(),
            "direct `route_write` call".to_string(),
        ]),
        "negative fixture proves the gate is AST-based and fails closed"
    );
}

#[derive(Default)]
struct IngressWriteSideEffectVisitor {
    findings: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for IngressWriteSideEffectVisitor {
    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if node.trait_.as_ref().is_some_and(|(_, path, _)| {
            path.segments.last().is_some_and(|segment| {
                matches!(
                    segment.ident.to_string().as_str(),
                    "MessageWriter" | "PostWriteRouter"
                )
            })
        }) {
            let trait_name = node
                .trait_
                .as_ref()
                .and_then(|(_, path, _)| path.segments.last())
                .expect("trait segment checked above")
                .ident
                .to_string();
            self.findings.insert(format!("{trait_name} implementation"));
        }
        syn::visit::visit_item_impl(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref()
            && let Some(segment) = path.path.segments.last()
        {
            let name = segment.ident.to_string();
            if matches!(name.as_str(), "persist_message" | "route_write")
                || name.starts_with("emit_local_post_write")
            {
                self.findings.insert(format!("direct `{name}` call"));
            }
        }
        syn::visit::visit_expr_call(self, node);
    }
}

fn canonical_write_modules(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files(&root.join("crates"), &mut files);
    files
}

fn routing_violations_in_fixture(source: &str) -> Vec<String> {
    let file = syn::parse_file(source).expect("negative fixture must parse");
    let mut visitor = HostRoutingVisitor::default();
    visitor.collect_delivery_function_aliases(&file);
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
    in_test_module: bool,
}

#[derive(Default)]
struct HostRoutingFunction {
    name: String,
    is_post_write_dispatch: bool,
    is_post_write_router_helper: bool,
    is_test: bool,
    accesses_host: bool,
    calls_delivery: bool,
    peer_delivery_calls: usize,
    reconciliation_delivery_calls: usize,
    https_transport_bindings: BTreeSet<String>,
    function_bindings: BTreeMap<String, FunctionBinding>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FunctionBinding {
    Delivery,
    Safe,
    Unresolved,
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
        if self.is_production_source() && !self.in_test_module {
            self.message_writer_implementations += usize::from(is_message_writer);
            self.post_write_router_implementations += usize::from(is_post_write_router);
        }
        let previous = self.in_post_write_router;
        self.in_post_write_router = is_post_write_router;
        syn::visit::visit_item_impl(self, node);
        self.in_post_write_router = previous;
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
        if let syn::Pat::Ident(binding) = &node.pat
            && let Some(init) = node.init.as_ref()
            && self.is_function_binding_candidate(&init.expr)
            && let provenance = self.function_binding_provenance(&init.expr)
            && let Some(function) = self.current_function_mut()
        {
            function
                .function_bindings
                .insert(binding.ident.to_string(), provenance);
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
        if method == "host"
            && let Some(function) = self.current_function_mut()
        {
            function.accesses_host = true;
        }
        let local_nudge = method.starts_with("emit_local_post_write");
        let reconciliation_delivery = (method == "deliver"
            && self.is_runtime_dispatcher_source()
            && self.current_function.is_some_and(|index| {
                self.functions
                    .get(index)
                    .is_some_and(|function| function.name == "reconcile_peer")
            }))
            || (method == "deliver"
                && self.is_peer_drain_coordinator_source()
                && self.current_function.is_some_and(|index| {
                    self.functions
                        .get(index)
                        .is_some_and(|function| function.name == "deliver_one")
                }));
        let peer_delivery = reconciliation_delivery
            || method == "deliver_to_peer"
            || (method == "deliver" && self.is_https_transport_receiver(&node.receiver));
        if (peer_delivery || local_nudge)
            && let Some(function) = self.current_function_mut()
            && !function.is_test
        {
            function.calls_delivery = true;
            if peer_delivery {
                function.peer_delivery_calls += 1;
            }
            if reconciliation_delivery {
                function.reconciliation_delivery_calls += 1;
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

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let previous = self.in_test_module;
        self.in_test_module = previous || node.attrs.iter().any(is_cfg_test_attribute);
        syn::visit::visit_item_mod(self, node);
        self.in_test_module = previous;
    }
}

impl HostRoutingVisitor {
    fn begin_function(&mut self, name: String, attrs: &[syn::Attribute]) -> Option<usize> {
        let index = self.functions.len();
        self.functions.push(HostRoutingFunction {
            is_post_write_dispatch: self.in_post_write_router && name == "dispatch",
            // AI.27 extracts the router's two cohesive actions to keep the
            // dispatcher below the production file/function limits. These
            // helpers remain private methods in the router-only module; no
            // other module may gain this authority.
            is_post_write_router_helper: self.is_post_write_router_helper(&name),
            is_test: self.in_test_module
                || attrs
                    .iter()
                    .any(|attr| attr.path().is_ident("test") || is_cfg_test_attribute(attr)),
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
        let name = segment.ident.to_string();
        if is_delivery_function_name(&name) || self.delivery_function_aliases.contains(&name) {
            return true;
        }
        self.current_function
            .and_then(|index| self.functions.get(index))
            .and_then(|current| current.function_bindings.get(&name))
            .is_some_and(|binding| *binding != FunctionBinding::Safe)
    }

    fn is_function_binding_candidate(&self, expression: &syn::Expr) -> bool {
        match expression {
            syn::Expr::Path(path) => path
                .path
                .segments
                .last()
                .is_some_and(|segment| {
                    let name = segment.ident.to_string();
                    is_delivery_function_name(&name)
                        || self.delivery_function_aliases.contains(&name)
                        || self
                            .current_function
                            .and_then(|index| self.functions.get(index))
                            .is_some_and(|function| function.function_bindings.contains_key(&name))
                }),
            syn::Expr::Cast(cast) => self.is_function_binding_candidate(&cast.expr),
            syn::Expr::Closure(closure) => self.is_function_binding_candidate(&closure.body),
            syn::Expr::Paren(paren) => self.is_function_binding_candidate(&paren.expr),
            syn::Expr::Group(group) => self.is_function_binding_candidate(&group.expr),
            syn::Expr::Reference(reference) => self.is_function_binding_candidate(&reference.expr),
            syn::Expr::Block(block) => block.block.stmts.last().is_some_and(|statement| {
                matches!(statement, syn::Stmt::Expr(expression, _) if self.is_function_binding_candidate(expression))
            }),
            syn::Expr::Call(call) => {
                self.is_function_binding_candidate(&call.func)
                    || call.args.iter().any(|argument| self.is_function_binding_candidate(argument))
            }
            syn::Expr::MethodCall(call) => {
                self.is_function_binding_candidate(&call.receiver)
                    || call.args.iter().any(|argument| self.is_function_binding_candidate(argument))
            }
            syn::Expr::Field(field) => self.is_function_binding_candidate(&field.base),
            syn::Expr::Tuple(tuple) => tuple
                .elems
                .iter()
                .any(|element| self.is_function_binding_candidate(element)),
            _ => false,
        }
    }

    fn function_binding_provenance(&self, expression: &syn::Expr) -> FunctionBinding {
        match expression {
            syn::Expr::Path(path) => {
                let Some(segment) = path.path.segments.last() else {
                    return FunctionBinding::Unresolved;
                };
                let name = segment.ident.to_string();
                if is_delivery_function_name(&name)
                    || self.delivery_function_aliases.contains(&name)
                {
                    FunctionBinding::Delivery
                } else {
                    self.current_function
                        .and_then(|index| self.functions.get(index))
                        .and_then(|function| function.function_bindings.get(&name))
                        .copied()
                        .unwrap_or(FunctionBinding::Unresolved)
                }
            }
            syn::Expr::Paren(paren) => self.function_binding_provenance(&paren.expr),
            syn::Expr::Group(group) => self.function_binding_provenance(&group.expr),
            syn::Expr::Reference(reference) => self.function_binding_provenance(&reference.expr),
            syn::Expr::Block(block) => block
                .block
                .stmts
                .last()
                .and_then(|statement| match statement {
                    syn::Stmt::Expr(expression, _) => Some(expression),
                    _ => None,
                })
                .map_or(FunctionBinding::Safe, |expression| {
                    self.function_binding_provenance(expression)
                }),
            syn::Expr::Call(call) => {
                if self.function_binding_provenance(&call.func) == FunctionBinding::Delivery
                    || call.args.iter().any(|argument| {
                        self.function_binding_provenance(argument) == FunctionBinding::Delivery
                    })
                {
                    FunctionBinding::Delivery
                } else {
                    FunctionBinding::Unresolved
                }
            }
            syn::Expr::MethodCall(call) => {
                let receiver = self.function_binding_provenance(&call.receiver);
                if receiver == FunctionBinding::Delivery
                    || call.args.iter().any(|argument| {
                        self.function_binding_provenance(argument) == FunctionBinding::Delivery
                    })
                {
                    FunctionBinding::Delivery
                } else {
                    FunctionBinding::Unresolved
                }
            }
            syn::Expr::Field(field) => self.function_binding_provenance(&field.base),
            syn::Expr::Tuple(tuple) => tuple
                .elems
                .iter()
                .map(|element| self.function_binding_provenance(element))
                .find(|provenance| *provenance == FunctionBinding::Delivery)
                .unwrap_or(FunctionBinding::Unresolved),
            syn::Expr::Closure(closure) => self.function_binding_provenance(&closure.body),
            syn::Expr::Lit(_) => FunctionBinding::Safe,
            _ => FunctionBinding::Unresolved,
        }
    }

    fn collect_delivery_function_aliases(&mut self, file: &syn::File) {
        let mut aliases = Vec::new();
        collect_use_aliases(file, &mut aliases);
        let mut changed = true;
        while changed {
            changed = false;
            for (source, alias) in &aliases {
                if (is_delivery_function_name(source)
                    || self.delivery_function_aliases.contains(source))
                    && self.delivery_function_aliases.insert(alias.clone())
                {
                    changed = true;
                }
            }
        }
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

    fn reconciliation_delivery_calls(&self) -> usize {
        self.functions
            .iter()
            .map(|function| function.reconciliation_delivery_calls)
            .sum()
    }

    fn violations(&self) -> Vec<String> {
        let mut violations = self
            .functions
            .iter()
            .filter(|function| {
                function.calls_delivery
                    && !function.is_post_write_router_helper
                    && function.reconciliation_delivery_calls == 0
            })
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

    fn is_runtime_dispatcher_source(&self) -> bool {
        self.source_path.as_ref().is_some_and(|path| {
            is_path_suffix(
                path,
                &[
                    "crates/atm-daemon/src/runtime_health.rs",
                    "crates/atm-daemon/src/runtime_health/peer_sync.rs",
                ],
            )
        })
    }

    fn is_peer_drain_coordinator_source(&self) -> bool {
        self.source_path.as_ref().is_some_and(|path| {
            path.ends_with(Path::new("crates/atm-daemon/src/peer_drain_coordinator.rs"))
        })
    }

    fn is_post_write_router_helper(&self, name: &str) -> bool {
        self.source_path.as_ref().is_some_and(|path| {
            is_path_suffix(
                path,
                &["crates/atm-daemon/src/runtime_health/peer_delivery_router.rs"],
            ) && is_delivery_function_name(name)
        })
    }
}

fn is_delivery_function_name(name: &str) -> bool {
    name.starts_with("emit_local_post_write") || name == "deliver_to_peer"
}

fn is_path_suffix(path: &Path, suffixes: &[&str]) -> bool {
    suffixes
        .iter()
        .any(|suffix| path.ends_with(Path::new(suffix)))
}

fn collect_use_aliases(file: &syn::File, aliases: &mut Vec<(String, String)>) {
    struct UseAliasCollector<'a> {
        aliases: &'a mut Vec<(String, String)>,
    }

    impl<'ast> Visit<'ast> for UseAliasCollector<'_> {
        fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
            collect_use_aliases_from_tree(&node.tree, self.aliases);
        }
    }

    UseAliasCollector { aliases }.visit_file(file);
}

fn collect_use_aliases_from_tree(tree: &syn::UseTree, aliases: &mut Vec<(String, String)>) {
    match tree {
        syn::UseTree::Rename(rename) => {
            aliases.push((rename.ident.to_string(), rename.rename.to_string()))
        }
        syn::UseTree::Name(name) => aliases.push((name.ident.to_string(), name.ident.to_string())),
        syn::UseTree::Path(path) => collect_use_aliases_from_tree(&path.tree, aliases),
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_use_aliases_from_tree(item, aliases);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

fn is_cfg_test_attribute(attribute: &syn::Attribute) -> bool {
    attribute.path().is_ident("cfg")
        && attribute
            .parse_args::<syn::Ident>()
            .is_ok_and(|ident| ident == "test")
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
fn atm_error_code_registry_is_shared_below_storage_and_core() {
    let root = workspace_root();
    let registry = read_source(&root.join("crates/atm-error/src/error_codes.rs"));
    let storage_facade = read_source(&root.join("crates/atm-storage/src/error_codes.rs"));
    let core_facade = read_source(&root.join("crates/atm-core/src/error_codes.rs"));

    assert!(
        registry.contains("pub enum AtmErrorCode"),
        "the neutral atm-error crate must own the concrete registry"
    );
    assert!(
        storage_facade.contains("pub use atm_error"),
        "atm-storage must consume the shared registry instead of defining a copy"
    );
    assert!(
        core_facade.contains("pub use atm_error"),
        "atm-core must consume the same shared registry"
    );
    assert!(
        !storage_facade.contains("pub enum AtmErrorCode"),
        "atm-storage must not reintroduce an owned AtmErrorCode definition"
    );
    assert!(
        !core_facade.contains("pub enum AtmErrorCode"),
        "atm-core's facade must not duplicate the registry either"
    );
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
fn atm_daemon_must_not_depend_on_atm_peer_tls_interop() {
    assert_forbidden_edge_absent("atm-daemon", "atm-peer-tls-interop");
    let boundary_path = workspace_root().join("boundaries/atm-peer-tls-interop/tls-interop.toml");
    let boundary: BoundaryToml = toml::from_str(&read_source(&boundary_path))
        .expect("TLS interop boundary must be valid TOML");
    assert!(
        boundary
            .dependencies
            .forbidden_edges
            .iter()
            .any(|edge| edge == "atm-daemon -> atm-peer-tls-interop"),
        "TLS interop boundary must mechanically retain the daemon forbidden edge"
    );
}

#[test]
fn storage_tls_boundary_lists_only_current_tls_consumers() {
    let boundary_path = workspace_root().join("boundaries/atm-storage/tls.toml");
    let boundary: BoundaryToml = toml::from_str(&read_source(&boundary_path))
        .expect("storage TLS boundary must be valid TOML");
    assert_eq!(
        boundary.dependencies.allowed_dependents,
        vec!["atm-peer-tls-interop".to_string()],
        "storage TLS helpers must name only crates that consume the TLS API"
    );
}

#[test]
fn atm_must_not_depend_on_atm_storage_rusqlite() {
    assert_forbidden_edge_absent("atm", "atm-storage-rusqlite");
}

#[test]
fn guarded_runtime_boundaries_forbid_their_declared_crate_edges() {
    for (source, target) in [
        ("atm", "atm-daemon"),
        ("atm-daemon", "atm-runtime"),
        ("atm-http-runtime", "atm"),
        ("atm-http-runtime", "atm-daemon-bootstrap"),
        ("atm-http-runtime", "atm-graft"),
        ("atm-http-runtime", "atm-storage-rusqlite"),
        ("atm-daemon-bootstrap", "atm-graft"),
    ] {
        assert_forbidden_edge_absent(source, target);
    }
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
        "reference-only daemon boundaries must not select atm-storage-rusqlite; violating files: {violations:?}"
    );

    let root = workspace_root();
    let replacement_bootstrap =
        root.join("boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml");
    let contents = fs::read_to_string(&replacement_bootstrap)
        .expect("replacement bootstrap boundary must be readable");
    let boundary: BoundaryToml =
        toml::from_str(&contents).expect("replacement bootstrap boundary must remain valid TOML");
    assert!(
        boundary
            .dependencies
            .allowed_dependencies
            .contains(&"atm-storage-rusqlite".to_string()),
        "AL.8 must document the one approved concrete storage selection point"
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
    let local_ipc_worker = root.join("crates/atm-daemon/src/local_ipc_transport/request_worker.rs");

    let daemon_lib_source = read_source(&daemon_lib).replace("\r\n", "\n");
    assert!(
        daemon_lib_source.contains("mod local_tcp_transport;")
            && daemon_lib_source.contains("#[cfg(not(windows))]\nmod local_ipc_transport;")
            && daemon_lib_source.contains("#[cfg(windows)]\npub(crate) use local_tcp_transport::LocalIpcServerTransportAdapter;"),
        "Unix keeps its UDS HTTP ingress while Windows selects the TCP HTTP adapter"
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
    let local_ipc_source = read_source(&local_ipc_worker);
    assert!(
        local_tcp_source.contains("pub(crate) struct LocalIpcServerTransportAdapter"),
        "the Windows local HTTP adapter must remain implemented by loopback TCP"
    );
    assert!(
        local_tcp_source.contains("HttpFrameReader")
            && local_ipc_source.contains("HttpFrameReader"),
        "Unix UDS and loopback TCP must use the shared HTTP frame reader"
    );
    let non_loopback_binds = local_tcp_source
        .lines()
        .filter(|line| line.contains("TcpListener::bind"))
        .filter(|line| !line.contains("Ipv4Addr::LOCALHOST"))
        .collect::<Vec<_>>();
    assert!(
        non_loopback_binds.is_empty(),
        "AI.11 local TCP listeners must bind only IPv4 loopback: {non_loopback_binds:?}"
    );
    let adapter_sources = [("local TCP", read_source(&local_tcp))];
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
            .is_some_and(|name| name.starts_with("test_") || name.ends_with("_tests.rs"))
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

fn production_runtime_identifier_findings(path: &Path, prohibited: &[&str]) -> Vec<String> {
    let source = read_source(path);
    let syntax = syn::parse_file(&source)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
    let mut detector = ProductionRuntimeIdentifierDetector {
        prohibited: prohibited.iter().copied().collect(),
        findings: BTreeSet::new(),
    };
    detector.visit_file(&syntax);
    detector.findings.into_iter().collect()
}

struct ProductionRuntimeIdentifierDetector<'a> {
    prohibited: BTreeSet<&'a str>,
    findings: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for ProductionRuntimeIdentifierDetector<'_> {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if item.attrs.iter().any(is_test_configuration_attribute) {
            return;
        }
        syn::visit::visit_item_mod(self, item);
    }

    fn visit_ident(&mut self, ident: &'ast syn::Ident) {
        let value = ident.to_string();
        if self.prohibited.contains(value.as_str()) {
            self.findings.insert(value);
        }
        syn::visit::visit_ident(self, ident);
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

#[test]
fn al1_http_runtime_is_core_contract_only_and_excludes_retired_transport_shapes() {
    let dependencies = direct_normal_workspace_dependencies();
    let actual = dependencies
        .get("atm-http-runtime")
        .expect("AL.1 HTTP runtime package must exist");
    assert_eq!(
        actual,
        &BTreeSet::from(["agent-team-mail-core".to_string()]),
        "atm-http-runtime may depend on ATM core contracts only"
    );

    let root = workspace_root();
    let runtime_root = root.join("crates/atm-http-runtime/src");
    let source = read_source(&runtime_root.join("lib.rs"));
    assert!(
        source.contains("../../../docs/plans/phase-al-am-runtime-boundary-checklist.md")
            && root
                .join("docs/plans/phase-al-am-runtime-boundary-checklist.md")
                .is_file(),
        "the public runtime crate documentation must link the shared AL/AM boundary checklist"
    );
    let prohibited = [
        "rusqlite",
        "tmux",
        "atm_graft",
        "PeerMessageArray",
        "PeerResendScheduler",
        "PeerDrainCoordinator",
        "HttpFrameReader",
    ];
    let mut sources = Vec::new();
    collect_rust_files(&runtime_root, &mut sources);
    for source_path in sources {
        let findings = production_runtime_identifier_findings(&source_path, &prohibited);
        assert!(
            findings.is_empty(),
            "AL.1 HTTP runtime {} must not contain prohibited production identifiers: {findings:?}",
            source_path.display()
        );
    }
}

#[test]
fn al1_compatibility_oracle_freezes_negative_inputs_and_client_allowlist() {
    let root = workspace_root();
    let oracle = read_source(&root.join("docs/plans/phase-al/AL1-runtime-compatibility-oracle.md"));

    for fixture in [
        "fixtures/malformed-json.http",
        "fixtures/oversized-body.http",
        "fixtures/invalid-peer-source-host.http",
    ] {
        assert!(
            oracle.contains(fixture),
            "AL.1 compatibility oracle must retain the `{fixture}` negative-input fixture"
        );
        assert!(
            root.join("docs/plans/phase-al").join(fixture).is_file(),
            "AL.1 compatibility fixture `{fixture}` must exist"
        );
    }
    for implementation in [
        "LocalIpcClientTransportAdapter",
        "GraftLocalIpcClientTransport",
        "FakeClientTransport",
        "LoopbackClientTransport",
    ] {
        assert!(
            oracle.contains(implementation),
            "AL.1 must inventory the existing DaemonApiClient implementation `{implementation}` before AL.4"
        );
    }

    let implementation_count = [
        root.join("crates/atm/src/composition.rs"),
        root.join("crates/atm-graft/src/transport.rs"),
        root.join("crates/atm-core/src/transport/testing.rs"),
    ]
    .into_iter()
    .map(|path| {
        read_source(&path)
            .matches("impl DaemonApiClient for")
            .count()
    })
    .sum::<usize>();
    assert_eq!(
        implementation_count, 4,
        "AL.1's four pre-AL.4 DaemonApiClient implementations must remain identifiable for AL.4's coordinated migration"
    );

    for path in [
        root.join("crates/atm/src/composition.rs"),
        root.join("crates/atm-graft/src/transport.rs"),
        root.join("crates/atm-core/src/transport/testing.rs"),
    ] {
        let source = read_source(&path);
        assert!(
            source.contains("#[async_trait]") && source.contains("async fn execute"),
            "AL.4 must migrate every retained DaemonApiClient implementation in {}",
            path.display()
        );
    }
}

#[test]
fn al4_shared_client_keeps_one_async_client_boundary_without_legacy_framing() {
    let root = workspace_root();
    let client = read_source(&root.join("crates/atm-http-runtime/src/client.rs"));
    let graft = read_source(&root.join("crates/atm-graft/src/lib.rs"));
    let cli = read_source(&root.join("crates/atm/src/composition.rs"));
    let python = read_source(&root.join("crates/atm-graft-python/src/lib.rs"));

    assert_eq!(
        client
            .matches("DaemonApiClient for HttpRuntimeClient")
            .count(),
        1,
        "AL.4 permits exactly one framework runtime client implementation"
    );
    assert!(
        client.contains("encode_http_request") && client.contains("decode_http_response"),
        "all future physical connectors must share core request encoding and response decoding"
    );
    assert!(
        client.contains("tokio::time::timeout") && client.contains("RequestDeadline"),
        "the shared client must enforce one absolute Tokio deadline"
    );
    for forbidden in [
        "HttpFrameReader",
        "read_http_response_with_frame_reader",
        "write_http_request_with_headers",
        "read_http_request(",
        "write_http_request(",
        "block_on(",
        "message[]",
        "retry",
        "replay",
    ] {
        assert!(
            !client.contains(forbidden),
            "AL.4's shared client must not introduce `{forbidden}`"
        );
    }
    assert!(
        graft.contains("async fn send_message") && cli.contains("async fn send("),
        "graft and CLI writes must await the existing DaemonApiClient boundary"
    );
    assert!(
        !graft.contains("block_on(") && !cli.contains("block_on("),
        "library and CLI layers must not bridge async work synchronously"
    );
    assert_eq!(
        python.matches(".block_on(").count(),
        1,
        "the Python extension may bridge only once at its outer PyO3 FFI boundary"
    );
}

#[test]
fn al9_cli_and_graft_send_use_the_selected_runtime_client() {
    let root = workspace_root();
    let cli = read_source(&root.join("crates/atm/src/composition.rs"));
    let graft = read_source(&root.join("crates/atm-graft/src/lib.rs"));

    for (consumer, source) in [("CLI", &cli), ("graft", &graft)] {
        assert!(
            source.contains("async_transport: atm_http_runtime::preferred_local_client("),
            "AL.9 {consumer} composition must select the runtime-owned local client for sends"
        );
        assert!(
            source.contains("direct_peer_tcp_client(") && source.contains("direct_peer_port()"),
            "AL.9 {consumer} host-qualified writes must choose the shared direct peer client before encoding"
        );
    }

    let cli_send = cli
        .split("pub(crate) async fn send(")
        .nth(1)
        .and_then(|source| source.split("pub(crate) fn ack(").next())
        .expect("CLI send implementation");
    let graft_send = graft
        .split("async fn send_message(")
        .nth(1)
        .and_then(|source| source.split("fn read_message(").next())
        .expect("graft send implementation");
    for (consumer, send) in [("CLI", cli_send), ("graft", graft_send)] {
        assert!(
            send.contains(".selected_write_transport(&request)?")
                && send.contains(".execute(ApiRequest::new(RequestEnvelope::Write"),
            "AL.9 {consumer} send must await the selected DaemonApiClient write path"
        );
        assert!(
            !send.contains("legacy_dispatch")
                && !send.contains("daemon_exchange_request")
                && !send.contains("daemon_try_connect"),
            "AL.9 {consumer} send must not regress to the retained synchronous compatibility path"
        );
    }
}

#[cfg(unix)]
#[test]
fn al5_uds_is_a_framework_adapter_over_the_one_client_and_router() {
    let root = workspace_root();
    let runtime = read_source(&root.join("crates/atm-http-runtime/src/lib.rs"));
    let http1_server = read_source(&root.join("crates/atm-http-runtime/src/http1_server.rs"));
    let staging = read_source(&root.join("crates/atm-http-runtime/src/private_staging.rs"));
    let unix_socket = read_source(&root.join("crates/atm-http-runtime/src/unix_socket.rs"));
    let client = read_source(&root.join("crates/atm-http-runtime/src/client.rs"));
    let combined = format!("{runtime}\n{unix_socket}\n{http1_server}\n{client}");

    assert!(
        combined.contains("UnixListener")
            && combined.contains("serve_unix_http1(")
            && combined.contains("UnixSocketPathGuard")
            && combined.contains("spawn_blocking(move || bind_unix_listener(&socket))")
            && combined.contains("drain_server_group(")
            && combined.contains("publish_prepared_unix_socket")
            && combined.contains("std::fs::rename(&staged_path, &socket.path)"),
        "AL.5 must own UDS lifecycle through Tokio, Axum/Hyper, blocking-pool setup, sibling drain, and inode-safe endpoint cleanup"
    );
    assert!(
        http1_server.contains("http1::Builder")
            && http1_server.contains("TokioTimer::new()")
            && http1_server.contains("header_read_timeout")
            && http1_server.contains("keep_alive(true)"),
        "the framework HTTP/1 adapter must enforce a Tokio timer-backed header deadline while allowing bounded HTTP/1 request batches"
    );
    assert!(
        staging.contains("pub(crate) fn allocate")
            && combined.contains("private_staging::allocate(parent, \"uds\"")
            && !combined.contains("UDS_STAGING_DIRECTORY_COUNTER"),
        "runtime-owned UDS staging allocation must use the one shared private-staging owner"
    );
    assert!(
        client.contains("reqwest::Client::builder()")
            && client.contains(".unix_socket(socket_path)")
            && client.contains("HttpRuntimeClient::new"),
        "AL.5 must use Reqwest only as the physical UDS connector under the AL.4 shared client"
    );
    for prohibited in [
        "HttpFrameReader",
        "read_http_request(",
        "write_http_request(",
        "write_http_request_with_headers",
        "std::os::unix::net::UnixStream",
        "std::thread::spawn",
        "thread::sleep",
        "message[]",
        "replay",
        "tokio::try_join!",
        "UnixListener::bind(&socket.path)",
    ] {
        assert!(
            !combined.contains(prohibited),
            "AL.5 UDS adapter must not introduce `{prohibited}`"
        );
    }
}

#[test]
fn al6_loopback_tcp_is_capability_authentication_over_the_one_client_and_router() {
    let root = workspace_root();
    let runtime = read_source(&root.join("crates/atm-http-runtime/src/lib.rs"));
    let http1_server = read_source(&root.join("crates/atm-http-runtime/src/http1_server.rs"));
    let staging = read_source(&root.join("crates/atm-http-runtime/src/private_staging.rs"));
    let adapter = read_source(&root.join("crates/atm-http-runtime/src/loopback_tcp.rs"));
    let client = read_source(&root.join("crates/atm-http-runtime/src/client.rs"));
    let combined = format!("{runtime}\n{http1_server}\n{adapter}\n{client}");

    assert!(
        runtime.contains("canonical_api_router(")
            && runtime
                .contains("authenticated_loopback_router(canonical_router.clone(), capability)")
            && http1_server.contains("into_make_service_with_connect_info::<SocketAddr>()")
            && http1_server.contains("Semaphore::new(max_connections)")
            && http1_server.contains("acquire_owned()"),
        "AL.6 loopback TCP must add authentication and bounded connection admission only before the canonical Axum route"
    );
    assert!(
        staging.contains("pub(crate) fn allocate")
            && adapter.contains("private_staging::allocate(parent, \"loopback\"")
            && !adapter.contains("LOOPBACK_RECORD_COUNTER"),
        "AL.6 endpoint publication must use the one shared private-staging owner"
    );
    assert!(
        adapter.contains("ConnectInfo(peer)")
            && adapter.contains("LOCAL_CAPABILITY_HEADER")
            && adapter.contains("LocalHttpEndpointRecord::active")
            && adapter.contains("SetFileSecurityW")
            && adapter.contains("cleanup_loopback_endpoint_record")
            && !adapter.contains("impl Drop for LoopbackEndpointRecordGuard"),
        "AL.6 loopback adapter must authenticate the loopback peer/capability and retain its platform-owned record ACL"
    );
    assert!(
        client.contains("struct LoopbackTcpConnector")
            && client.contains("load_active_loopback_endpoint")
            && client.contains("execute_reqwest_request"),
        "AL.6 must use the AL.4 shared Reqwest request encoder/decoder after endpoint-record validation"
    );
    for prohibited in [
        "HttpFrameReader",
        "read_http_request(",
        "write_http_request(",
        "write_http_request_with_headers",
        "std::net::TcpStream",
        "std::thread::spawn",
        "thread::sleep",
        "PeerMessageArray",
        "PeerResendScheduler",
        "PeerDrainCoordinator",
        "message[]",
        "replay",
    ] {
        assert!(
            !combined.contains(prohibited),
            "AL.6 loopback adapter must not introduce `{prohibited}`"
        );
    }
}

#[test]
fn al8_active_daemon_root_cannot_reach_frozen_server_composition() {
    let root = workspace_root();
    let manifest = read_source(&root.join("crates/atm-daemon/Cargo.toml"));
    let entrypoint = read_source(&root.join("crates/atm-daemon/src/main.rs"));
    let bootstrap = read_source(&root.join("crates/atm-daemon-bootstrap/src/lib.rs"));
    let owner_gate = read_source(&root.join("crates/atm-daemon-bootstrap/src/owner_gate.rs"));
    let active_root = format!("{entrypoint}\n{bootstrap}\n{owner_gate}");

    assert!(
        manifest.contains("autolib = false")
            && entrypoint.contains(
                "atm_daemon_bootstrap::run_replacement_daemon_with_observability(observability).await",
            )
            && !entrypoint.contains("atm_daemon::"),
        "AL.8 must compile atm-daemon as the replacement binary only; its frozen library cannot remain an active server fallback"
    );
    assert!(
        bootstrap.contains("HttpRuntimeBuilder::new(config, handler)")
            && bootstrap.contains(".start()")
            && bootstrap.contains("active_received_hook_selector")
            && bootstrap.contains("DaemonOwnerGuard::acquire_at")
            && bootstrap.contains("REPLACEMENT_DRAIN_DEADLINE"),
        "AL.8 must acquire the owner gate, inject the received-hook selector, start the Tokio runtime, and retain the one five-second drain bound"
    );
    for forbidden in [
        "run_daemon_with_observability",
        "RuntimeComposition",
        "LocalIpcServerTransportAdapter",
        "DaemonRequestDispatcher",
        "DispatchWorkerPool",
        "HttpFrameReader",
        "PeerResendScheduler",
        "peer_delivery",
    ] {
        assert!(
            !active_root.contains(forbidden),
            "AL.8 active daemon root must not reach frozen legacy construct `{forbidden}`"
        );
    }
    assert_forbidden_edge_absent("atm-daemon", "atm-storage-rusqlite");
    assert_forbidden_edge_absent("atm-daemon", "atm-peer-tls-interop");

    let dependencies = direct_normal_workspace_dependencies();
    assert_eq!(
        dependencies
            .get("atm-daemon")
            .expect("active daemon package must exist"),
        &BTreeSet::from([
            "agent-team-mail-core".to_string(),
            "atm-daemon-bootstrap".to_string(),
        ]),
        "the active daemon executable may reach ATM code only through core contracts and the replacement bootstrap"
    );

    let selector =
        read_source(&root.join("crates/atm-daemon-bootstrap/src/received_hook_selector.rs"));
    let daemon_manifest = read_source(&root.join("crates/atm-daemon/Cargo.toml"));
    let active_sources = format!("{active_root}\n{selector}");
    for forbidden in [
        "Runtime::Builder",
        "Handle::block_on",
        "std::thread::spawn",
        "std::thread::sleep",
        "HttpFrameReader",
        "read_http_request(",
        "write_http_request(",
        "PeerResendScheduler",
        "PeerDrainCoordinator",
    ] {
        assert!(
            !active_sources.contains(forbidden),
            "AL.8 active composition must not restore `{forbidden}`"
        );
    }
    assert!(
        !entrypoint.contains("BenchmarkHookMode")
            && !entrypoint.contains("ATM_HTTP_RECEIVED_HOOK_MODE")
            && !entrypoint.contains("ATM_HTTP_BENCHMARK_MODE")
            && !daemon_manifest.contains("benchmark-harness"),
        "the shipped daemon must not expose a benchmark hook-disable selection surface"
    );
}

#[test]
fn al8_marks_the_replacement_bootstrap_as_the_only_active_daemon_boundary() {
    let root = workspace_root();
    let active = daemon_boundary_files()
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let boundary: BoundaryToml = toml::from_str(&contents)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
            (boundary.status.state == "active").then_some(path)
        })
        .collect::<Vec<_>>();
    assert!(
        active.is_empty(),
        "AL.8 keeps every legacy daemon boundary reference-only until Phase AM deletion: {active:?}"
    );
    let replacement_bootstrap =
        root.join("boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml");
    let contents = fs::read_to_string(&replacement_bootstrap)
        .expect("replacement bootstrap boundary must be readable");
    let boundary: BoundaryToml =
        toml::from_str(&contents).expect("replacement bootstrap boundary must remain valid TOML");
    assert_eq!(boundary.status.state, "active");
}

#[test]
fn al9_received_hook_selector_exposes_only_its_factory_boundary() {
    let root = workspace_root();
    let selector =
        read_source(&root.join("crates/atm-daemon-bootstrap/src/received_hook_selector.rs"));
    let bootstrap = read_source(&root.join("crates/atm-daemon-bootstrap/src/lib.rs"));

    assert!(
        selector.contains("struct ReplacementReceivedHookSelector")
            && !selector.contains("pub struct ReplacementReceivedHookSelector")
            && selector.contains("fn new(service_runtime: LocalServiceRuntime) -> Self")
            && !selector.contains("pub fn new(service_runtime: LocalServiceRuntime) -> Self"),
        "AL.9 keeps the concrete received-hook selector internal to daemon bootstrap"
    );
    assert!(
        bootstrap.contains("pub use received_hook_selector::active_received_hook_selector;")
            && !bootstrap.contains("ReplacementReceivedHookSelector"),
        "AL.9 exposes only the received-hook selector factory across the bootstrap boundary"
    );
}

#[test]
fn al1_receiver_hook_boundary_replaces_retired_release_gate_artifacts() {
    let root = workspace_root();
    let release_gate = read_source(&root.join("scripts/validate_release.py"));
    let graft_boundary_inventory = read_source(&root.join("docs/atm-graft/boundaries.md"));
    assert!(
        release_gate.contains("message-received-hook-emitter.toml")
            && release_gate.contains("message-received-hook.toml"),
        "release validation must guard the active receiver-hook manifests"
    );
    assert!(
        graft_boundary_inventory.contains("## Message Received Hook"),
        "the Graft receiver implementation must have a current boundary-inventory entry"
    );
    assert!(
        !root
            .join("boundaries/atm-core/post-send-hook-emitter.toml")
            .exists()
            && !root
                .join("boundaries/atm-core/graft-post-send-port.toml")
                .exists(),
        "retired sender-oriented hook manifests must not remain live compatibility artifacts"
    );
}

#[test]
fn al3_received_hook_is_single_receiver_side_path_without_detached_work() {
    let root = workspace_root();
    let dispatcher = read_source(&root.join("crates/atm-daemon/src/runtime_health/dispatch.rs"));
    let router =
        read_source(&root.join("crates/atm-daemon/src/runtime_health/peer_delivery_router.rs"));
    let post_commit =
        read_source(&root.join("crates/atm-daemon/src/runtime_health/post_commit_work.rs"));
    let post_write = read_source(&root.join("crates/atm-core/src/send/post_write.rs"));
    let message_received_emitter =
        read_source(&root.join("crates/atm-daemon/src/message_received_emitter.rs"));
    let post_commit_code = post_commit
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let post_write_code = post_write
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let message_received_emitter_code = message_received_emitter
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    let finish = dispatcher
        .find(".finish(&self.service_runtime, self.observability.as_ref())")
        .expect("AL.3 must finish the durable write before receiver-hook routing");
    let dispatch = dispatcher
        .find("PostWriteRouter::dispatch(self, &mut message, deadline)")
        .expect("AL.3 must route the received hook through the canonical dispatcher");
    assert!(
        finish < dispatch,
        "AL.3 must invoke the received hook only after durable write completion"
    );
    assert_eq!(
        dispatcher
            .matches("PostWriteRouter::dispatch(self, &mut message, deadline)")
            .count(),
        1,
        "all UDS, TCP, and peer ingress adapters must converge on one post-persistence hook call site"
    );
    assert!(
        dispatcher.contains("let newly_persisted = message.prepared.is_newly_persisted();")
            && dispatcher.contains("if newly_persisted {"),
        "the one hook-routing decision must state the new-versus-idempotent persistence disposition explicitly"
    );
    assert_eq!(
        router
            .matches("atm_core::send::emit_persisted_local_post_write(")
            .count(),
        1,
        "the router must retain exactly one receiver-hook invocation site"
    );
    assert!(
        router.contains("deadline.remaining().is_none()"),
        "AL.3 must skip receiver-hook work once the inherited request deadline is exhausted"
    );

    for (adapter, path) in [
        (
            "local UDS",
            "crates/atm-daemon/src/local_ipc_transport/request_worker.rs",
        ),
        ("local TCP", "crates/atm-daemon/src/local_tcp_transport.rs"),
        // AK.2 removed the legacy daemon HTTPS peer adapter. Peer ingress now
        // reaches the same replacement router through the current runtime
        // boundary, so there is no transport-specific source file to inspect.
    ] {
        let source = read_source(&root.join(path));
        assert!(source.contains(".route("), "{adapter} must use ApiRouter");
        assert!(
            !source.contains("MessageReceivedHookEmitter")
                && !source.contains("emit_persisted_local_post_write"),
            "{adapter} must not create a transport-specific received-hook path"
        );
    }

    for prohibited in ["LocalNudge", "MessageReceivedHookEmitter"] {
        assert!(
            !post_commit_code.contains(prohibited),
            "the post-commit peer adapter must not restore receiver-hook `{prohibited}` work"
        );
    }
    for prohibited in ["thread::spawn", "tokio::spawn", "sync_channel"] {
        assert!(
            !post_commit_code.contains(prohibited),
            "the post-commit peer adapter must not restore receiver-hook `{prohibited}` work"
        );
        assert!(
            !post_write_code.contains(prohibited),
            "the core post-write adapter must not create detached receiver-hook `{prohibited}` work"
        );
        assert!(
            !message_received_emitter_code.contains(prohibited),
            "the daemon receiver emitter must not create detached receiver-hook `{prohibited}` work"
        );
    }
    assert!(
        post_write_code.contains("MessageReceivedHookEmitter")
            && post_write_code.contains("emit_post_send_effects"),
        "the core post-write adapter must retain the injected receiver-hook boundary"
    );
    assert!(
        message_received_emitter_code.contains("impl MessageReceivedHookEmitter"),
        "the daemon receiver emitter must remain the concrete injected hook implementation"
    );

    // `atm-graft/src/runtime.rs` is deliberately excluded: it is the
    // independently-started receiver implementation, not an outbound client.
    for path in [
        "crates/atm/src",
        "crates/atm-daemon-client/src",
        "crates/atm-graft/src/transport.rs",
    ] {
        let path = root.join(path);
        let sources = if path.is_dir() {
            let mut sources = Vec::new();
            collect_rust_files(&path, &mut sources);
            sources
        } else {
            vec![path]
        };
        for source_path in sources {
            let source = read_source(&source_path);
            assert!(
                !source.contains("MessageReceivedHookEmitter")
                    && !source.contains(".emit_post_send("),
                "outbound client {} must not call a receiver notification hook",
                source_path.display()
            );
        }
    }
}

#[test]
fn al3_replacement_runtime_cannot_restore_legacy_or_blocking_runtime_constructs() {
    let runtime_root = workspace_root().join("crates/atm-http-runtime/src");
    let mut sources = Vec::new();
    collect_rust_files(&runtime_root, &mut sources);
    for source_path in sources {
        let source = read_source(&source_path);
        for prohibited in [
            "atm_daemon",
            "Runtime::Builder",
            "Handle::block_on",
            "std::sync::mpsc",
            "std::thread::sleep",
            "thread::sleep",
            "peer_delivery_router",
            "local_ipc_transport",
            "local_tcp_transport",
            "https_transport",
        ] {
            assert!(
                !source.contains(prohibited),
                "replacement runtime {} must not restore `{prohibited}`",
                source_path.display()
            );
        }
    }
    let storage_router = read_source(&runtime_root.join("storage_and_nudge_router.rs"));
    assert!(
        storage_router.contains("async fn commit_write")
            && storage_router.contains("prepare_write_with_async_runtime("),
        "the replacement write path must await the core async storage admission boundary"
    );
    assert!(
        !storage_router.contains("StorageWriterIngress")
            && !storage_router.contains("MAX_CONCURRENT_WRITER_SUBMISSIONS"),
        "the replacement write path must not restore the redundant spawn_blocking writer ingress"
    );
    for prohibited in ["rusqlite::", ".with_transaction(", "Connection::open"] {
        assert!(
            !storage_router.contains(prohibited),
            "the replacement runtime must not open a direct SQLite transaction through `{prohibited}`"
        );
    }
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
        root.join("boundaries/atm-daemon/socket-server-transport.toml"),
        root.join("boundaries/atm-graft/shared-client-consumer.toml"),
        root.join("boundaries/atm-http-runtime/http-runtime.toml"),
        root.join("boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml"),
        root.join("boundaries/atm-runtime/runtime-composition.toml"),
        root.join("boundaries/atm-storage/tls.toml"),
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
