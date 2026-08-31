// ARCHITECTURE ENFORCEMENT CRATE
// Every assertion in this file is a merge gate.
// Removing, weakening, or commenting out any assertion requires an explicit
// architecture decision recorded in docs/architecture.md.
// QA MUST FAIL any PR that loosens a boundary assertion without that record.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use cargo_metadata::{DependencyKind, MetadataCommand};
use quote::ToTokens;
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
    ("atm-storage-rusqlite", "atm-core"),
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
    ("atm-core", "atm-template-sc-compose"),
    ("atm-storage", "atm-template-sc-compose"),
    ("atm-storage-rusqlite", "atm-template-sc-compose"),
    ("atm", "atm-template-sc-compose"),
    ("atm-daemon", "atm-template-sc-compose"),
    ("atm-runtime", "atm-template-sc-compose"),
    ("atm-http-runtime", "atm-template-sc-compose"),
    ("atm-core", "atm-herdr"),
    ("atm-storage", "atm-herdr"),
    ("atm-storage-rusqlite", "atm-herdr"),
    ("atm-herdr", "atm-daemon"),
    ("atm-herdr", "atm-daemon-bootstrap"),
    ("atm-herdr", "atm-http-runtime"),
    ("atm-herdr", "atm-storage-rusqlite"),
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

fn contains_adapter_availability_inference(source: &str) -> bool {
    const WRAPPER_TERMS: &[&str] = &["adapter", "wrapper", "transport"];
    const OPTION_BRANCH_TERMS: &[&str] = &["is_some", "is_none", "some", "none", "match"];

    // Scan code tokens rather than comments or one explanatory phrase. This
    // catches the actual forbidden construct: choosing the direct-peer path
    // from an optional TLS/stream wrapper's presence. A comment rewrite cannot
    // evade this guard, and a branch spread across several Rust tokens remains
    // visible inside the short token window.
    let tokens: Vec<_> = source
        .lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .flat_map(|line| {
            line.split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        })
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();

    tokens.iter().enumerate().any(|(index, token)| {
        let is_wrapper = WRAPPER_TERMS.iter().any(|term| token.contains(term));
        let window_start = index.saturating_sub(3);
        let window_end = (index + 5).min(tokens.len());
        is_wrapper
            && tokens[window_start..window_end]
                .iter()
                .any(|nearby| OPTION_BRANCH_TERMS.contains(&nearby.as_str()))
    })
}

#[test]
fn adapter_availability_guard_scans_code_branches_not_comment_wording() {
    assert!(
        !contains_adapter_availability_inference(
            "// adapter availability must never select plaintext\nlet route = direct;"
        ),
        "comments are documentation, not an executable adapter-availability branch"
    );
    for source in [
        "if config.peer_stream_adapter.is_some() { return authenticated(); }",
        "match transport_wrapper { Some(wrapper) => wrapper.connect(), None => direct() }",
        "let direct = tls_adapter.is_none();",
    ] {
        assert!(
            contains_adapter_availability_inference(source),
            "the architecture guard must reject adapter availability selection: {source}"
        );
    }
}

#[test]
fn daemon_must_not_read_caller_workspace_config() {
    let root = workspace_root();
    let composition = read_source(&root.join("crates/atm-daemon-bootstrap/src/lib.rs"));
    assert!(
        composition.contains("assemble_daemon_runtime()?"),
        "replacement daemon composition must select the daemon-only runtime assembly"
    );
    assert!(
        composition.contains("pub fn assemble_daemon_runtime()")
            && composition.contains(".map(RuntimeAssembly::for_daemon)"),
        "daemon-only assembly must discard the workspace-backed configuration view"
    );
    assert!(
        !composition
            .split("pub fn assemble_daemon_runtime()")
            .nth(1)
            .unwrap_or_default()
            .split("/// Starts the replacement Tokio/Axum daemon")
            .next()
            .unwrap_or_default()
            .contains("current_dir"),
        "daemon-only assembly must not resolve the process working directory"
    );
    let runtime_composition = read_source(&root.join("crates/atm-runtime/src/composition.rs"));
    assert!(
        runtime_composition.contains("config_current_dir: None,")
            && runtime_composition
                .contains("workflow_telemetry: Arc::clone(self.workflow_telemetry.diagnostics()),"),
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
/// AO.3 extends this AO.2 baseline guard with the bootstrap-selected
/// `PlaintextTest` arm and its invalid-TLS-state independence.
fn ao2_plaintext_baseline_stays_on_the_existing_direct_peer_pipeline() {
    let root = workspace_root();
    let bootstrap = read_source(&root.join("crates/atm-daemon-bootstrap/src/lib.rs"));
    let runtime = read_source(&root.join("crates/atm-http-runtime/src/lib.rs"));
    let runtime_setup = read_source(&root.join("crates/atm-http-runtime/src/runtime_setup.rs"));
    let runtime_sources = format!("{runtime}\n{runtime_setup}");
    let client = read_source(&root.join("crates/atm-http-runtime/src/client.rs"));
    let policy = read_source(&root.join("crates/atm-core/src/peer_wire.rs"));

    let direct_listener = runtime
        .split("async fn bind_configured_direct_peer_listener")
        .nth(1)
        .and_then(|source| source.split("async fn bind_loopback_listener").next())
        .expect("direct-peer listener implementation");
    let direct_connector = client
        .split("impl DirectPeerTcpConnector")
        .nth(1)
        .and_then(|source| source.split("impl LoopbackTcpConnector").next())
        .expect("direct-peer connector implementation");
    assert!(
        bootstrap.contains("let direct_peer_port = parse_direct_peer_port(std::env::args_os())?;")
            && bootstrap.contains("DirectPeerTcpConfig::configured(direct_peer_port),"),
        "AO2 plaintext characterization must retain the configured direct-peer listener: its default remains the standard protocol port, while an isolated benchmark account may select one explicit non-zero port without changing the pipeline"
    );
    let plaintext_adapter_arm = bootstrap
        .split("fn peer_stream_adapter_for_mode")
        .nth(1)
        .and_then(|source| source.split("fn replacement_runtime_config").next())
        .and_then(|source| source.split("PeerWireSecurity::PlaintextTest =>").nth(1))
        .and_then(|source| source.split("\n    }").next())
        .expect("bootstrap plaintext adapter-selection arm");
    assert!(
        plaintext_adapter_arm.contains("Ok(None),"),
        "AO2 plaintext mode must bypass all TLS configuration and return no stream wrapper"
    );
    for forbidden in [
        "build_mtls_adapter",
        "PeerConfigStore",
        "MtlsPeerStreamAdapter",
        "peer_tls",
        "rustls",
        "certificate",
    ] {
        assert!(
            !plaintext_adapter_arm.contains(forbidden),
            "AO2 plaintext bootstrap arm must not inspect TLS/configuration/adapter state `{forbidden}`"
        );
    }
    assert!(
        client.contains("struct DirectPeerTcpConnector")
            && client.contains("pub fn direct_peer_tcp_client")
            && direct_connector.contains("execute_reqwest_request"),
        "AO2 plaintext characterization must retain the shared direct-peer connector and HTTP exchange"
    );
    assert!(
        direct_listener.contains("TcpListener::bind")
            && runtime_sources.contains("canonical_api_router(")
            && runtime_sources.contains("AuthenticatedConnector::peer_socket()"),
        "AO2 plaintext listener must enter the ordinary canonical router"
    );
    for (scope, source) in [
        ("direct-peer listener", direct_listener),
        ("direct-peer connector", direct_connector),
    ] {
        for forbidden in ["peer_tls", "rustls", "PeerConfigStore", "certificate"] {
            assert!(
                !source.contains(forbidden),
                "AO2 plaintext {scope} must not inspect TLS/configuration/adapter state `{forbidden}`"
            );
        }
        assert!(
            !contains_adapter_availability_inference(source),
            "AO2 plaintext {scope} must not infer its mode from adapter availability or synonyms"
        );
    }
    for forbidden in [
        "std::env",
        "rustls",
        "PeerConfigStore",
        "TlsConnector",
        "TlsAcceptor",
    ] {
        assert!(
            !policy.contains(forbidden),
            "AO2 peer-wire policy must remain transport-neutral and reject `{forbidden}`"
        );
    }
    assert!(
        policy.contains("pub enum PeerWireSecurity")
            && policy.contains("Mtls")
            && policy.contains("PlaintextTest")
            && policy.contains("pub struct PeerWireMode")
            && policy.contains("pub const fn mtls()"),
        "AO2 needs a typed mode vocabulary whose normal policy is mTLS"
    );
}

#[test]
/// AO.3 extends this AO.2 policy guard over the daemon launch seam while
/// preserving the single canonical HTTP application pipeline.
fn ao2_peer_wire_policy_keeps_one_error_registry_and_one_http_pipeline() {
    let root = workspace_root();
    let error_registry = read_source(&root.join("crates/atm-error/src/error_codes.rs"));
    let error_catalog = read_source(&root.join("crates/atm-storage/src/error_catalog.rs"));
    let adr = read_source(&root.join("docs/adr/ADR-047-layered-peer-wire-security.md"));
    let requirements = read_source(&root.join("docs/requirements.md"));
    let daemon_requirements = read_source(&root.join("docs/atm-daemon/requirements.md"));

    for (code, variant) in [
        ("ATM_PEER_WIRE_MODE_INVALID", "PeerWireModeInvalid"),
        (
            "ATM_PEER_WIRE_MODE_SOURCE_FORBIDDEN",
            "PeerWireModeSourceForbidden",
        ),
        (
            "ATM_PEER_WIRE_PLAINTEXT_AUTHENTICATION_REQUIRED",
            "PeerWirePlaintextAuthenticationRequired",
        ),
    ] {
        assert!(
            error_registry.contains(code) && error_catalog.contains(variant),
            "AO2 peer-wire failure `{code}` must use the central registry and catalog recovery"
        );
    }
    for required in [
        "PeerWireMode` launch policy",
        "It does not select an HTTP",
        "never falls back to plaintext",
        "Plaintext-test evidence cannot satisfy",
    ] {
        assert!(
            adr.contains(required),
            "ADR-047 must retain the AO2 policy guarantee `{required}`"
        );
    }
    assert!(
        requirements
            .contains("Mode ownership and the layered-stream constraint are defined by ADR-047.")
            && daemon_requirements.contains("ADR-047 owns the typed launch-mode selection"),
        "both core and daemon requirements must cite the accepted ADR-047 policy"
    );
}

#[test]
fn ao4_benchmark_targets_cannot_introduce_an_alternate_daemon_pipeline() {
    let root = workspace_root();
    let bootstrap = read_source(&root.join("crates/atm-daemon-bootstrap/src/lib.rs"));
    let bootstrap_manifest = read_source(&root.join("crates/atm-daemon-bootstrap/Cargo.toml"));
    let justfile = read_source(&root.join("Justfile"));
    let benchmark_entrypoint =
        root.join("crates/atm-daemon-bootstrap/src/bin/atm-daemon-benchmark.rs");
    let benchmark_source = read_source(&benchmark_entrypoint);
    let benchmark_daemon = bootstrap
        .split("pub async fn run_benchmark_daemon")
        .nth(1)
        .and_then(|source| source.split("fn peer_stream_adapter_for_mode").next())
        .expect("feature-gated benchmark daemon must delegate through bootstrap");

    assert!(
        bootstrap_manifest.contains("benchmark-harness = []")
            && bootstrap_manifest.contains("name = \"atm-daemon-benchmark\"")
            && benchmark_daemon.contains("run_replacement_daemon_with_selector")
            && benchmark_daemon.contains("parse_peer_wire_mode"),
        "the feature-gated benchmark daemon must select the ordinary AO2 peer-wire mode and delegate into the shared bootstrap"
    );
    for forbidden in ["HttpRuntimeBuilder", "StorageAndNudgeRouter", "TcpListener"] {
        assert!(
            !benchmark_source.contains(forbidden),
            "the benchmark entrypoint must not construct a second HTTP daemon pipeline through `{forbidden}`"
        );
    }
    assert!(
        justfile.contains("cargo build --release -p agent-team-mail -p atm-daemon")
            && !justfile.contains("atm-daemon-benchmark")
            && !justfile.contains("benchmark-harness"),
        "the public benchmark recipe must build only the shipped Tokio/Axum daemon"
    );
}

#[test]
fn acknowledgement_cannot_restore_a_second_write_pipeline() {
    let root = workspace_root();
    // The canonical write module is split across its facade and submodules
    // (RULE-003 line cap); the tripwire scans the whole module surface.
    let write = ["mod.rs", "pipeline.rs", "acknowledgement.rs"]
        .iter()
        .map(|file| {
            fs::read_to_string(root.join("crates/atm-core/src/write").join(file))
                .expect("canonical write module must be readable")
        })
        .collect::<String>();
    let send = fs::read_to_string(root.join("crates/atm-core/src/send/mod.rs"))
        .expect("send facade module must be readable");
    let acknowledgement = fs::read_to_string(root.join("crates/atm-core/src/ack/mod.rs"))
        .expect("acknowledgement module must be readable");
    let api = fs::read_to_string(root.join("crates/atm-core/src/api.rs"))
        .expect("transport-neutral API module must be readable");
    let router =
        fs::read_to_string(root.join("crates/atm-http-runtime/src/storage_and_nudge_router.rs"))
            .expect("canonical HTTP write router must be readable");

    assert!(
        write.contains("fn write_mail_with_runtime_impl"),
        "AI.7 requires one canonical write pipeline in `crate::write`"
    );
    assert!(
        !send.contains("fn write_mail_with_runtime_impl")
            && send.contains("pub use crate::write::"),
        "AU.3 forbids a second write pipeline in `crate::send`; send only re-exports `crate::write`"
    );
    assert!(
        write.contains("fn admit_acknowledgement_write")
            && !send.contains("fn admit_acknowledgement_write")
            && !acknowledgement.contains("fn admit_acknowledgement_write"),
        "AI.31 acknowledgement admission must enter the canonical write pipeline through `crate::write` only"
    );
    assert!(
        write.contains("runtime.acknowledge_message_atomically")
            && !acknowledgement.contains("runtime.acknowledge_message_atomically"),
        "AI.31 acknowledgement source resolution and paired commit must stay behind the sealed storage boundary inside `crate::write`"
    );
    assert!(
        !acknowledgement.contains("resolve_acknowledgement_source")
            && !write.contains("resolve_acknowledgement_source"),
        "AI.31 forbids restoring an application-layer acknowledgement source read"
    );
    assert!(
        acknowledgement.contains("crate::write::write_mail_with_runtime("),
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
        !api.contains("MessageRequest")
            && router.contains("impl CanonicalWriteHandler for StorageAndNudgeRouter"),
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
            && api.contains("const MESSAGES_PATH: &str = \"/v1/atm/messages\";"),
        "AI.23 requires send and ACK to select the one POST /v1/atm/messages resource"
    );
    assert!(
        !api.contains("is_ack_path") && !api.contains("/ack\""),
        "AI.23 forbids an acknowledgement-specific HTTP resource"
    );

    let runtime = read_source(&root.join("crates/atm-http-runtime/src/lib.rs"));
    assert!(
        runtime.contains("build_router") || runtime.contains("canonical_router"),
        "AI.23 requires the replacement runtime to own the sole ingress router"
    );
    let handler = read_source(&root.join("crates/atm-http-runtime/src/message_handler.rs"));
    assert!(
        handler.contains("fn decode_framework_request") && handler.contains("http_route_surface()"),
        "AI.23 requires the framework ingress boundary to decode the canonical route surface"
    );
}

#[test]
fn canonical_write_router_has_one_host_routing_decision() {
    let root = workspace_root();
    let router = read_source(&root.join("crates/atm-http-runtime/src/storage_and_nudge_router.rs"));
    assert!(
        router.contains("impl CanonicalWriteHandler for StorageAndNudgeRouter")
            && router.contains("async fn commit_write")
            && router.contains("async fn emit_received_hook"),
        "AM.6 keeps one live typed write router with durable admission and post-durability hook handling"
    );
    assert!(
        !root
            .join("crates/atm-daemon/src/runtime_health.rs")
            .exists()
            && !root
                .join("crates/atm-daemon/src/runtime_health/dispatch.rs")
                .exists()
            && !root
                .join("crates/atm-daemon/src/runtime_health/peer_delivery_router.rs")
                .exists(),
        "AM.6 deletes the unselected daemon dispatcher stack"
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
fn queue_marker_handoff_clear_has_one_core_owner() {
    let root = workspace_root();
    let mut files = Vec::new();
    collect_rust_files(&root.join("crates"), &mut files);

    let mut definitions = Vec::new();
    let mut violations = Vec::new();
    for path in files {
        let source = read_source(&path);
        let syntax = syn::parse_file(&source)
            .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
        let mut visitor = QueueMarkerClearVisitor::default();
        visitor.visit_file(&syntax);
        definitions.extend(
            visitor
                .definitions
                .into_iter()
                .map(|name| format!("{}::{name}", path.display().to_string().replace('\\', "/"))),
        );
        violations.extend(
            visitor
                .violations
                .into_iter()
                .map(|name| format!("{}::{name}", path.display().to_string().replace('\\', "/"))),
        );
    }

    assert_eq!(
        definitions.len(),
        1,
        "clear_queue_marker_after_handoff must have exactly one workspace definition: {definitions:?}"
    );
    assert!(
        definitions[0].contains("crates/atm-core/"),
        "the sole queue-marker clear helper must be owned by atm-core: {definitions:?}"
    );
    assert!(
        violations.is_empty(),
        "direct clear_pending_on_handoff calls are forbidden outside the core helper and store impl/tests: {violations:?}"
    );
}

#[derive(Default)]
struct QueueMarkerClearVisitor {
    definitions: Vec<String>,
    violations: Vec<String>,
    current_function: Option<String>,
    in_test_module: bool,
    in_pending_nudge_store_impl: bool,
}

impl<'ast> Visit<'ast> for QueueMarkerClearVisitor {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let previous = self.current_function.replace(node.sig.ident.to_string());
        if node.sig.ident == "clear_queue_marker_after_handoff" {
            self.definitions.push(node.sig.ident.to_string());
        }
        syn::visit::visit_item_fn(self, node);
        self.current_function = previous;
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let previous = self.in_pending_nudge_store_impl;
        self.in_pending_nudge_store_impl = node.trait_.as_ref().is_some_and(|(_, path, _)| {
            path.segments
                .last()
                .is_some_and(|segment| segment.ident == "PendingNudgeStore")
        });
        syn::visit::visit_item_impl(self, node);
        self.in_pending_nudge_store_impl = previous;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let previous = self.current_function.replace(node.sig.ident.to_string());
        syn::visit::visit_impl_item_fn(self, node);
        self.current_function = previous;
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let previous = self.in_test_module;
        self.in_test_module = previous || node.attrs.iter().any(is_cfg_test_attribute);
        syn::visit::visit_item_mod(self, node);
        self.in_test_module = previous;
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if node.method == "clear_pending_on_handoff"
            && self.current_function.as_deref() != Some("clear_queue_marker_after_handoff")
            && !self.in_pending_nudge_store_impl
            && !self.in_test_module
        {
            self.violations.push(
                self.current_function
                    .clone()
                    .unwrap_or_else(|| "<module-level expression>".to_owned()),
            );
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

#[test]
fn ai23_peer_adapter_never_matches_localhost_or_own_ip() {
    let root = workspace_root();
    let router = read_source(&root.join("crates/atm-http-runtime/src/storage_and_nudge_router.rs"));
    assert!(
        router.contains("dispatch_resolved_peer_write")
            && !router.contains("PeerDelivery")
            && !router.contains("signal_after_persist"),
        "the typed router may deliver one admitted remote write and has no peer worker signal"
    );
    for forbidden in ["is_loopback", "is_loopback()"] {
        assert!(
            !router.contains(forbidden),
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
        "crates/atm-daemon/src/runtime_health.rs",
        "crates/atm-daemon/src/runtime_health/dispatch.rs",
        "crates/atm-daemon/src/runtime_health/peer_delivery_router.rs",
    ] {
        assert!(
            !root.join(deleted_module).exists(),
            "AK.2 must not retain retired peer-worker module `{deleted_module}`"
        );
    }

    let production_sources = [
        "crates/atm-daemon/src/main.rs",
        "crates/atm-http-runtime/src/storage_and_nudge_router.rs",
        "crates/atm-core/src/api.rs",
        "crates/atm-core/src/protocol.rs",
        "crates/atm/src/commands/peer.rs",
        "crates/atm/src/composition.rs",
        "crates/atm-storage/src/contract.rs",
        "crates/atm-storage-rusqlite/src/peer_config_store.rs",
    ];
    for source in production_sources {
        let contents = read_source(&root.join(source));
        let retired = retired_peer_worker_symbols_in_source(&contents);
        assert!(
            retired.is_empty(),
            "AK.2 production source `{source}` must not retain retired symbols {retired:?}"
        );
    }
}

fn retired_peer_worker_symbols_in_source(source: &str) -> BTreeSet<String> {
    let syntax = syn::parse_file(source).expect("production source must parse for AK.2 guard");
    let mut visitor = RetiredPeerWorkerSymbolVisitor::default();
    visitor.visit_file(&syntax);
    visitor.found
}

#[derive(Default)]
struct RetiredPeerWorkerSymbolVisitor {
    found: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for RetiredPeerWorkerSymbolVisitor {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        const RETIRED_TERMINALS: &[&str] = &[
            "PeerDeliveryCoordinator",
            "PeerDrainCoordinator",
            "PeerPostCommitWorkQueue",
            "PeerSyncPolicy",
            "PeerSyncRequest",
            "PeerSyncOutcome",
            "PeerLinkStatus",
            "HttpsTransport",
        ];

        let segments: Vec<_> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        if let Some(terminal) = segments.last()
            && RETIRED_TERMINALS.contains(&terminal.as_str())
        {
            self.found.insert(terminal.clone());
        }
        if segments
            .windows(2)
            .any(|pair| matches!(pair, [first, second] if first == "PostCommitWorkKey" && second == "PeerDelivery"))
        {
            self.found
                .insert("PostCommitWorkKey::PeerDelivery".to_owned());
        }
        syn::visit::visit_path(self, path);
    }
}

#[test]
fn ak2_retirement_guard_matches_symbols_not_substrings() {
    assert!(
        retired_peer_worker_symbols_in_source("pub struct PeerWireSecurity;").is_empty(),
        "AO2's PeerWireSecurity vocabulary must not collide with retired AK.2 identifiers"
    );
    assert_eq!(
        retired_peer_worker_symbols_in_source("fn f(_: PeerSyncPolicy) {}"),
        BTreeSet::from(["PeerSyncPolicy".to_owned()]),
        "the AK.2 guard must still reject an exact retired symbol"
    );
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
    is_post_write_router_helper: bool,
    is_test: bool,
    accesses_host: bool,
    calls_delivery: bool,
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
        vec!["atm-peer-tls-interop".to_string(), "peer-tls".to_string(),],
        "storage TLS helpers must name only crates that consume the TLS API"
    );
}

#[test]
fn tls_identity_scrubs_the_source_pem_buffer_after_parsing() {
    let source = read_source(&workspace_root().join("crates/atm-storage/src/tls.rs"));
    let syntax = syn::parse_file(&source).expect("storage TLS source must parse");
    let mut visitor = PemScrubbingVisitor::default();
    visitor.visit_file(&syntax);
    assert!(
        visitor.wraps_pem_in_zeroizing,
        "TlsIdentity::load must retain its source PEM in Zeroizing so private-key bytes are scrubbed after parsing"
    );
}

#[derive(Default)]
struct PemScrubbingVisitor {
    wraps_pem_in_zeroizing: bool,
}

impl<'ast> Visit<'ast> for PemScrubbingVisitor {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let syn::Pat::Ident(binding) = &local.pat
            && binding.ident == "pem"
            && local.init.as_ref().is_some_and(|init| {
                matches!(
                    init.expr.as_ref(),
                    syn::Expr::Call(call) if is_zeroizing_constructor(call)
                )
            })
        {
            self.wraps_pem_in_zeroizing = true;
        }
        syn::visit::visit_local(self, local);
    }
}

fn is_zeroizing_constructor(call: &syn::ExprCall) -> bool {
    let syn::Expr::Path(path) = call.func.as_ref() else {
        return false;
    };
    path.path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .eq(["Zeroizing".to_owned(), "new".to_owned()])
}

#[test]
/// AO.3 extends this AO.2 ownership guard: bootstrap consumes the opaque mTLS
/// stream seam, while every other production crate stays adapter-neutral.
fn ao2_mtls_stream_adapter_is_the_only_authorized_production_tls_consumer() {
    let root = workspace_root();
    for (source, target) in [
        ("atm-http-runtime", "peer-tls"),
        ("atm", "peer-tls"),
        ("atm-graft", "peer-tls"),
        ("atm-daemon", "peer-tls"),
    ] {
        assert_forbidden_edge_absent(source, target);
    }
    let boundary_path = root.join("boundaries/peer-tls/mtls-peer-stream-adapter.toml");
    let boundary: BoundaryToml = toml::from_str(&read_source(&boundary_path))
        .expect("mTLS stream adapter boundary must be valid TOML");
    assert_eq!(
        boundary.dependencies.allowed_dependents,
        vec!["atm-daemon-bootstrap".to_owned()],
        "only bootstrap may compose the concrete mTLS byte-stream adapter"
    );
    let source = read_source(&root.join("crates/peer-tls/src/lib.rs"));
    let syntax = syn::parse_file(&source).expect("peer-tls source must parse");
    let mut visitor = PeerTlsSurfaceVisitor::default();
    visitor.visit_file(&syntax);
    assert!(
        visitor.forbidden.is_empty(),
        "peer-tls must remain an mTLS byte-stream-only adapter and reject {:?}",
        visitor.forbidden
    );
    assert!(
        visitor.required.contains("tokio_rustls")
            && visitor.required.contains("MtlsPeerStreamAdapter")
            && visitor.required.contains("PeerConfigStore"),
        "AO2 must retain one concrete Rustls/Tokio-Rustls adapter over configuration and byte streams"
    );
}

#[derive(Default)]
struct PeerTlsSurfaceVisitor {
    forbidden: BTreeSet<String>,
    required: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for PeerTlsSurfaceVisitor {
    fn visit_ident(&mut self, ident: &'ast syn::Ident) {
        let name = ident.to_string();
        if matches!(
            name.as_str(),
            "RequestEnvelope" | "canonical_api_router" | "MessageStore" | "PeerWireMode"
        ) {
            self.forbidden.insert(name.clone());
        }
        if matches!(
            name.as_str(),
            "tokio_rustls" | "MtlsPeerStreamAdapter" | "PeerConfigStore"
        ) {
            self.required.insert(name);
        }
        syn::visit::visit_ident(self, ident);
    }
}

#[test]
fn ao2_mtls_surface_guard_matches_ast_identifiers_not_comments_or_strings() {
    let comment_only = syn::parse_file("// PeerWireMode\nconst NOTE: &str = \"MessageStore\";")
        .expect("fixture must parse");
    let mut comment_visitor = PeerTlsSurfaceVisitor::default();
    comment_visitor.visit_file(&comment_only);
    assert!(comment_visitor.forbidden.is_empty());

    let forbidden_use = syn::parse_file("use crate::PeerWireMode;").expect("fixture must parse");
    let mut forbidden_visitor = PeerTlsSurfaceVisitor::default();
    forbidden_visitor.visit_file(&forbidden_use);
    assert_eq!(
        forbidden_visitor.forbidden,
        BTreeSet::from(["PeerWireMode".to_owned()])
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
fn sqlite_writer_batch_window_is_private_to_storage() {
    let root = workspace_root();
    let writer_path = root.join("crates/atm-storage-rusqlite/src/writer/mod.rs");
    let writer = read_source(&writer_path);
    let syntax = syn::parse_file(&writer).expect("sqlite writer source must parse");
    let batch_window = syntax
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Const(item) if item.ident == "BATCH_TIME_BUDGET" => Some(item),
            _ => None,
        })
        .expect("sqlite writer must declare its fixed batch window");
    assert!(
        matches!(batch_window.vis, syn::Visibility::Inherited),
        "the writer batch window must not have any visibility modifier"
    );

    let smoke_sources = writer_batch_window_smoke_sources(&root);
    assert!(
        smoke_sources.contains(&root.join("scripts/smoke/run_admission_capacity.py")),
        "the admission benchmark harness must remain covered by the writer batch-window guard"
    );

    for source in ai11_guarded_workspace_sources(&root)
        .into_iter()
        .chain(smoke_sources)
    {
        if source == writer_path {
            continue;
        }
        assert!(
            !read_source(&source).contains("BATCH_TIME_BUDGET"),
            "the writer batch window must not become {} configuration",
            source.display(),
        );
    }
}

#[test]
fn atm_storage_rusqlite_must_not_depend_on_atm_runtime() {
    assert_forbidden_edge_absent("atm-storage-rusqlite", "atm-runtime");
}

#[test]
fn atm_storage_rusqlite_must_not_depend_on_atm_core() {
    assert_forbidden_edge_absent("atm-storage-rusqlite", "atm-core");
}

#[test]
fn atm_graft_must_not_depend_on_atm_storage_rusqlite() {
    assert_forbidden_edge_absent("atm-graft", "atm-storage-rusqlite");
}

#[test]
fn template_sc_compose_is_bootstrap_owned_and_forbidden_elsewhere() {
    for source in [
        "atm-core",
        "atm-storage",
        "atm-storage-rusqlite",
        "atm",
        "atm-daemon",
        "atm-runtime",
        "atm-http-runtime",
    ] {
        assert_forbidden_edge_absent(source, "atm-template-sc-compose");
    }

    let boundary_path =
        workspace_root().join("boundaries/atm-template-sc-compose/sc-composer.toml");
    let boundary: BoundaryToml = toml::from_str(&read_source(&boundary_path))
        .expect("template sc-compose boundary must be valid TOML");
    assert_eq!(
        boundary.dependencies.allowed_dependents,
        vec!["atm-daemon-bootstrap".to_string()],
        "only the replacement bootstrap may construct the production template adapter"
    );
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
fn aq4_send_to_staging_dir_has_a_single_construction_site_and_atm_temp_is_read_only_via_env_source()
{
    // AQ4 deliverable 7 (lane-C-relevant half, ATM-QA-002): precedent is
    // `al3_received_hook_is_single_receiver_side_path_without_detached_work`'s
    // `emit_received_hook` single-call-site assertion above. Nothing should
    // be able to add a second `send_to_staging_dir()` implementation, or a
    // free-function `env::var("ATM_TEMP")` read, without this test failing.
    let source_root = workspace_root().join("crates");
    let mut files = Vec::new();
    collect_rust_files(&source_root, &mut files);
    // Exclude this meta-test crate itself: its own source (this file)
    // necessarily contains the literal strings this test searches for, as
    // the search patterns rather than real definitions/reads.
    files.retain(|path| {
        !path
            .components()
            .any(|component| component.as_os_str() == "atm-architecture")
    });

    let mut send_to_staging_dir_definitions = 0usize;
    let mut forbidden_atm_temp_env_reads = Vec::new();
    let mut sanctioned_atm_temp_read_files = Vec::new();

    for path in &files {
        let source = read_source(path);
        send_to_staging_dir_definitions += source.matches("fn send_to_staging_dir(").count();

        // Matches both `env::var("ATM_TEMP")` and `std::env::var("ATM_TEMP")`
        // (and the `_os` variant): any module-path prefix before `env::`
        // still leaves this literal substring present.
        for forbidden in ["env::var(\"ATM_TEMP\")", "env::var_os(\"ATM_TEMP\")"] {
            if source.contains(forbidden) {
                forbidden_atm_temp_env_reads.push(format!("{}: {forbidden}", path.display()));
            }
        }
        // The sanctioned form: a **method call** on an `EnvSource` trait
        // object (`env.var("ATM_TEMP")`), never the free-function path
        // (ADR-055's M14 note; `crates/atm-core/src/atm_temp.rs`'s own
        // module doc comment).
        if source.contains("env.var(\"ATM_TEMP\")") {
            sanctioned_atm_temp_read_files.push(path.clone());
        }
    }

    assert_eq!(
        send_to_staging_dir_definitions, 1,
        "send_to_staging_dir() must have exactly one construction site in the workspace"
    );
    assert!(
        forbidden_atm_temp_env_reads.is_empty(),
        "ATM_TEMP must be read only through the EnvSource seam (env.var(...) method call), never a free-function env::var/env::var_os call: {forbidden_atm_temp_env_reads:?}"
    );
    assert_eq!(
        sanctioned_atm_temp_read_files.len(),
        1,
        "ATM_TEMP's real environment read must stay concentrated in exactly one file, found: {sanctioned_atm_temp_read_files:?}"
    );
}

#[test]
fn aq4_resolve_picker_recipient_has_a_single_construction_site() {
    // AQ4 deliverable 7 (dev-owned half, ADR-055 decision (e)): the
    // companion assertion to
    // `aq4_send_to_staging_dir_has_a_single_construction_site_...` above.
    // `resolve_picker_recipient` is the sprint doc's declared "single
    // canonical implementation" for turning a `--from-json` recipient id
    // into a routable `AgentAddress`; nothing should be able to add a
    // second, divergent resolver without this test failing.
    let source_root = workspace_root().join("crates");
    let mut files = Vec::new();
    collect_rust_files(&source_root, &mut files);
    files.retain(|path| {
        !path
            .components()
            .any(|component| component.as_os_str() == "atm-architecture")
    });

    let mut definitions = 0usize;
    for path in &files {
        let source = read_source(path);
        definitions += source.matches("fn resolve_picker_recipient(").count();
    }

    assert_eq!(
        definitions, 1,
        "resolve_picker_recipient() must have exactly one construction site in the workspace"
    );
}

#[test]
fn ai11_deletion_gate_rejects_retired_windows_transport_ast_and_dependencies() {
    let root = workspace_root();
    let daemon_lib = root.join("crates/atm-daemon/src/main.rs");

    let daemon_lib_source = read_source(&daemon_lib).replace("\r\n", "\n");
    assert!(
        !daemon_lib_source.contains("local_ipc_transport")
            && !daemon_lib_source.contains("local_tcp_transport")
            && !daemon_lib_source.contains("local_ipc_connection"),
        "AM.3 must not restore a legacy daemon local listener module declaration"
    );
    let legacy_local_listener_sources = ai11_guarded_workspace_sources(&root)
        .iter()
        .filter(|path| retired_local_listener_source(path).is_some())
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    assert!(
        legacy_local_listener_sources.is_empty(),
        "AM.3 must keep every legacy local listener source absent: {legacy_local_listener_sources:?}"
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

    let router_implementations = ai11_guarded_workspace_sources(&root)
        .iter()
        .filter(|path| !is_test_only_source(path))
        .map(|path| production_api_router_implementation_count(path))
        .sum::<usize>();
    assert_eq!(
        router_implementations, 0,
        "AM.6 deletes the obsolete daemon ApiRouter implementation"
    );
    let typed_router =
        read_source(&root.join("crates/atm-http-runtime/src/storage_and_nudge_router.rs"));
    assert!(
        typed_router.contains("impl CanonicalWriteHandler for StorageAndNudgeRouter"),
        "AM.6 requires the live HTTP runtime to own the canonical write handler"
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
fn ai11_deletion_gate_rejects_orphaned_legacy_local_listener_paths() {
    let fixture =
        workspace_root().join("retired-local-listener-fixture/local_ipc_transport/accept_loop.rs");
    assert_eq!(
        retired_local_listener_source(&fixture),
        Some("legacy local listener source"),
        "the AM.3 deletion gate must catch an accept_loop.rs-style leftover"
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
    assert!(root.join("crates/atm-daemon/src/main.rs").exists());
    assert!(
        !root.join("crates/atm-daemon/src/lib.rs").exists(),
        "the frozen daemon composition library must not reappear beside the shipped binary"
    );
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

fn collect_python_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
    {
        let path = entry
            .unwrap_or_else(|error| panic!("failed to read directory entry: {error}"))
            .path();
        if path.is_dir() {
            collect_python_files(&path, files);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("py") {
            files.push(path);
        }
    }
}

fn writer_batch_window_smoke_sources(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_python_files(&root.join("scripts/smoke"), &mut files);
    files.sort();
    files
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

fn retired_local_listener_source(path: &Path) -> Option<&'static str> {
    let file_name = path.file_name()?.to_str()?;
    if matches!(
        file_name,
        "local_tcp_transport.rs" | "local_ipc_transport.rs" | "local_ipc_connection.rs"
    ) || path
        .components()
        .any(|component| component.as_os_str() == "local_ipc_transport")
    {
        Some("legacy local listener source")
    } else {
        None
    }
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
        &BTreeSet::from(["agent-team-mail-core".to_string(), "atm-herdr".to_string(),]),
        "atm-http-runtime may depend on ATM core contracts and the Herdr process adapter"
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
fn al9_atm_graft_pins_full_dependency_set_including_http_runtime() {
    // RBQA post-merge review (2026-08): atm-graft/Cargo.toml declared
    // atm-http-runtime (AL.9, cc3ae58c4 routes atm-graft write-transport
    // selection through atm_http_runtime::preferred_local_client /
    // selected_write_transport) without the boundary TOML's
    // allowed_dependencies listing it, and without a Rust guard pinning
    // atm-graft's full dependency set the way al1_http_runtime_is_core_
    // contract_only_and_excludes_retired_transport_shapes does for
    // atm-http-runtime. This test closes both gaps.
    let dependencies = direct_normal_workspace_dependencies();
    let actual = dependencies
        .get("atm-graft")
        .expect("atm-graft package must exist");
    let expected = BTreeSet::from([
        "agent-team-mail-core".to_string(),
        "atm-daemon-client".to_string(),
        "atm-http-runtime".to_string(),
    ]);
    assert_eq!(
        actual, &expected,
        "atm-graft's normal workspace dependency set drifted; update this guard and the \
         boundaries/atm-graft/shared-client-consumer.toml allowed_dependencies together"
    );

    let root = workspace_root();
    let boundary_path = root.join("boundaries/atm-graft/shared-client-consumer.toml");
    let boundary: BoundaryToml = toml::from_str(&read_source(&boundary_path))
        .expect("atm-graft shared-client-consumer boundary must parse");
    let allowed_dependencies = boundary
        .dependencies
        .allowed_dependencies
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let package_to_crate_name = BTreeMap::from([
        ("agent-team-mail-core".to_string(), "atm-core".to_string()),
        (
            "atm-daemon-client".to_string(),
            "atm-daemon-client".to_string(),
        ),
        (
            "atm-http-runtime".to_string(),
            "atm-http-runtime".to_string(),
        ),
    ]);
    let missing = actual
        .iter()
        .map(|package| {
            package_to_crate_name.get(package).unwrap_or_else(|| {
                panic!("no crate-name mapping registered for package `{package}`")
            })
        })
        .filter(|crate_name| !allowed_dependencies.contains(crate_name.as_str()))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "boundaries/atm-graft/shared-client-consumer.toml allowed_dependencies is missing crates \
         atm-graft actually depends on: {missing:?}"
    );
}

#[test]
fn al1_compatibility_oracle_retains_negative_inputs_after_ipc_retirement() {
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

    assert_eq!(
        read_source(&root.join("crates/atm-http-runtime/src/client.rs"))
            .matches("DaemonApiClient for HttpRuntimeClient")
            .count(),
        1,
        "after Phase-AM retirement, one shared HttpRuntimeClient implementation must own protocol encoding"
    );
    assert!(
        !root.join("crates/atm-graft/src/transport.rs").exists(),
        "the historical graft IPC adapter must remain deleted after Phase-AM migration"
    );
}

#[test]
fn hermes_atm_runtime_boundary_keeps_generic_graft_host_agnostic() {
    let root = workspace_root();
    let boundary_path = root.join("boundaries/hermes-atm/runtime-composition.toml");
    let boundary: BoundaryToml = toml::from_str(&read_source(&boundary_path))
        .expect("hermes-atm boundary record must parse");
    assert_eq!(boundary.name, "HermesAtmRuntime");
    assert_eq!(boundary.owner_crate_path, "hermes_atm");
    assert_eq!(
        boundary.dependencies.allowed_dependencies,
        vec!["atm-graft"]
    );

    let graft_python = root.join("crates/atm-graft-python/python");
    for source in fs::read_dir(&graft_python)
        .expect("generic Python source directory must be readable")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("py"))
    {
        let contents = read_source(&source);
        assert!(
            !contents.contains("gateway.") && !contents.contains("telegram"),
            "generic atm-graft source must not regain Hermes/Telegram policy: {}",
            source.display()
        );
    }
    let runtime = read_source(&root.join("crates/hermes-atm/src/hermes_atm/runtime.py"));
    assert!(
        runtime.contains("atm_graft.PyGraftSession")
            && runtime.contains("inject_internal_message")
            && !runtime.contains("subprocess")
            && !runtime.contains("socket"),
        "hermes-atm must use the public graft receiver and public host injection seam only"
    );
}

#[test]
fn al4_shared_client_keeps_one_async_client_boundary_without_legacy_framing() {
    let root = workspace_root();
    let client = read_source(&root.join("crates/atm-http-runtime/src/client.rs"));
    let graft = read_source(&root.join("crates/atm-graft/src/lib.rs"));
    let cli = read_source(&root.join("crates/atm/src/composition.rs"));
    let python = read_source(&root.join("crates/atm-graft-python/src/lib.rs"));
    let python_query = read_source(&root.join("crates/atm-graft-python/src/query.rs"));
    let daemon_client = read_source(&root.join("crates/atm-daemon-client/src/lib.rs"));

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
    assert!(
        client.contains("is_safe_to_reconnect") && client.contains("prepare_pre_send_reconnect"),
        "the shared client must keep any pre-send reconnect policy explicit and connector-owned"
    );
    for forbidden in [
        "HttpFrameReader",
        "read_http_response_with_frame_reader",
        "write_http_request_with_headers",
        "read_http_request(",
        "write_http_request(",
        "block_on(",
        "message[]",
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
        python.matches(".block_on(").count() + python_query.matches(".block_on(").count(),
        3,
        "the three Python-exposed graft operations may bridge only at the shared outer PyO3 runtime boundary across the FFI modules"
    );
    assert_eq!(
        daemon_client.matches(".block_on(").count(),
        1,
        "the retained daemon-client compatibility shim may bridge only once"
    );
}

#[test]
fn al9_cli_and_graft_send_use_the_selected_runtime_client() {
    let root = workspace_root();
    let cli = read_source(&root.join("crates/atm/src/composition.rs"));
    let graft = read_source(&root.join("crates/atm-graft/src/lib.rs"));
    let runtime_client = read_source(&root.join("crates/atm-http-runtime/src/client.rs"));

    assert!(
        runtime_client.contains("pub fn selected_write_transport")
            && runtime_client.contains("SAME_HOST_REQUEST_DEADLINE"),
        "AL.9 must keep the local-vs-direct-peer write decision and deadline in atm-http-runtime"
    );

    for (consumer, source) in [("CLI", &cli), ("graft", &graft)] {
        assert!(
            source.contains("async_transport: atm_http_runtime::preferred_local_client("),
            "AL.9 {consumer} composition must select the runtime-owned local client for sends"
        );
        assert!(
            !source.contains("fn selected_write_transport")
                && !source.contains("const SAME_HOST_REQUEST_DEADLINE"),
            "AL.9 {consumer} must delegate write-transport selection and its deadline to atm-http-runtime"
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
            send.contains(
                "atm_http_runtime::selected_write_transport(&request, &self.async_transport)?"
            ) && send.contains(".execute(ApiRequest::new(RequestEnvelope::Write"),
            "AL.9 {consumer} send must await the runtime-selected DaemonApiClient write path"
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
fn phase_am_cli_and_graft_nonwrite_requests_use_the_http_client_boundary() {
    let root = workspace_root();
    let cli = read_source(&root.join("crates/atm/src/composition.rs"));
    let graft = read_source(&root.join("crates/atm-graft/src/lib.rs"));

    for (consumer, source, retired_symbol) in [
        ("CLI", cli.as_str(), "LocalIpcClientTransportAdapter"),
        ("graft", graft.as_str(), "GraftLocalIpcClientTransport"),
    ] {
        assert!(
            !source.contains(retired_symbol),
            "Phase-AM {consumer} must not retain the retired synchronous IPC adapter `{retired_symbol}`"
        );
    }
    assert!(
        !root.join("crates/atm-graft/src/transport.rs").exists(),
        "Phase-AM graft must not retain a synchronous IPC transport module"
    );

    let cli_composition = cli
        .split("pub(crate) struct CliComposition")
        .nth(1)
        .expect("CLI composition source");
    let graft_client = graft
        .split("pub struct GraftClient")
        .nth(1)
        .and_then(|source| source.split("pub struct GraftSession").next())
        .expect("graft client source");

    let consumers: [(&str, &str, &[&str]); 2] = [
        (
            "CLI",
            cli_composition,
            &[
                "async fn execute_request",
                "pub(crate) async fn ack",
                "pub(crate) async fn receive",
                "pub(crate) async fn peek",
                "pub(crate) async fn list",
                "pub(crate) async fn clear",
            ],
        ),
        (
            "graft",
            graft_client,
            &[
                "async fn execute_request",
                "async fn read_message",
                "pub async fn mailbox_work_counts",
            ],
        ),
    ];
    for (consumer, source, required_methods) in consumers {
        assert!(
            !source.contains("legacy_dispatch"),
            "Phase-AM {consumer} must not retain a synchronous compatibility request dispatcher"
        );
        for required in required_methods {
            assert!(
                source.contains(required),
                "Phase-AM {consumer} must retain `{required}` on the shared async HTTP boundary"
            );
        }
    }

    assert!(
        !graft_client.contains("acknowledge_message") && !graft_client.contains("pub async fn ack"),
        "Phase-AM graft must not expose a duplicate acknowledgement write path; `atm ack` remains CLI-owned"
    );
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
fn al8_replacement_runtime_registers_maintenance_for_every_composed_runtime_maintenance_implementer()
 {
    // Phase-end CRITICAL regression: `HerdrQueueWakePump` was composed into
    // `StorageAndNudgeRouter::with_maintenance` (AQ2.7), and
    // `StorageAndNudgeRouter` forwards `RuntimeMaintenance::start` to it, but
    // the production `start_replacement_runtime` entry point never
    // registered the router itself as the `HttpRuntime`'s maintenance
    // object via `HttpRuntimeBuilder::with_maintenance`. `HttpRuntime` never
    // spawned the forwarding call, so the pump silently never ran outside of
    // tests that call `pump.tick_once()` directly. This pins both halves of
    // that wiring so a future composed `RuntimeMaintenance` implementer
    // cannot regress the same way.
    let root = workspace_root();
    let bootstrap = read_source(&root.join("crates/atm-daemon-bootstrap/src/lib.rs"));
    let replacement_handler =
        read_source(&root.join("crates/atm-daemon-bootstrap/src/replacement_handler.rs"));

    assert!(
        replacement_handler.contains("HerdrQueueWakePump::new(")
            && replacement_handler.contains(".with_maintenance(queue_wake_pump)"),
        "AL.8 replacement handler must compose the Herdr queue-wake pump as the router's \
         `RuntimeMaintenance` implementer"
    );

    let start_replacement_runtime = extract_fn_body(&bootstrap, "start_replacement_runtime");
    assert!(
        start_replacement_runtime.contains("HttpRuntimeBuilder::new(")
            && start_replacement_runtime.contains(".with_maintenance("),
        "AL.8 `start_replacement_runtime` builds the replacement runtime's `HttpRuntimeBuilder` \
         but never registers the router as the `HttpRuntime`'s maintenance object; any \
         `RuntimeMaintenance` implementer composed into `StorageAndNudgeRouter` (for example \
         `HerdrQueueWakePump`) would silently never run in production"
    );
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
fn aq25_received_hook_manifest_matches_async_implementers() {
    let root = workspace_root();
    let manifest =
        read_source(&root.join("boundaries/atm-core/message-received-hook-emitter.toml"));
    let selector =
        read_source(&root.join("crates/atm-daemon-bootstrap/src/received_hook_selector.rs"));
    let implementers = [
        "TokioTmuxReceivedHook",
        "HerdrReceivedHook",
        "PublishedGraftReceivedHook",
        "PullPendingReceivedHook",
    ];
    assert_eq!(
        selector
            .matches("impl AsyncMessageReceivedHookEmitter for")
            .count(),
        implementers.len(),
        "the selector's async implementer count must match the AQ2.5 inventory"
    );
    for implementer in implementers {
        assert!(
            selector.contains(&format!(
                "impl AsyncMessageReceivedHookEmitter for {implementer}"
            )),
            "selector is missing async implementer {implementer}"
        );
        assert!(
            manifest.contains(implementer),
            "boundary manifest is missing async implementer {implementer}"
        );
    }
}

#[test]
fn aq25_adr_addendum_contains_normative_trigger_policy() {
    let root = workspace_root();
    let adr = read_source(&root.join("docs/adr/ADR-054-nudge-taxonomy-and-queue-mechanism.md"));
    for required in [
        "AQ2.5 addendum",
        "debounced there",
        "RAM-only daemon-lifetime state",
        "drops the oldest item",
        "at most one oldest queue item",
    ] {
        assert!(
            adr.contains(required),
            "ADR-054 is missing AQ2.5 term {required}"
        );
    }
}

/// AC9 / ATM-QA-003: the ADR-054 addendum must carry a recorded
/// quality-mgr sign-off *section*, not merely policy prose. This checks for
/// the heading and the sign-off table's header row, which persist once
/// quality-mgr fills in the pending row on re-gate -- unlike the pending
/// placeholder text itself, which is expected to change.
#[test]
fn aq25_adr_addendum_records_a_quality_mgr_sign_off_section() {
    let root = workspace_root();
    let adr = read_source(&root.join("docs/adr/ADR-054-nudge-taxonomy-and-queue-mechanism.md"));
    let heading = "### AQ2.5 quality-mgr sign-off";
    let heading_index = adr
        .lines()
        .position(|line| line.trim_end() == heading)
        .expect("ADR-054 must contain an exact AQ2.5 quality-mgr sign-off heading");
    let mut signoff_section = adr
        .lines()
        .skip(heading_index + 1)
        .take_while(|line| !line.starts_with("### "));
    assert!(
        signoff_section
            .any(|line| line.trim() == "| Sprint | Gate | Reviewer | Date | Verdict | Notes |"),
        "ADR-054 AQ2.5 sign-off heading must contain the sign-off table header"
    );
}

#[test]
fn no_merge_conflict_markers_in_tracked_docs() {
    let root = workspace_root();
    let output = Command::new("git")
        .args([
            "-C",
            root.to_str().expect("workspace path is UTF-8"),
            "ls-files",
            "-z",
            "--",
            "docs",
            ".sprints",
            ".triage",
        ])
        .output()
        .expect("git must be available to enumerate tracked documentation");
    assert!(
        output.status.success(),
        "git ls-files failed while enumerating tracked documentation: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut findings = Vec::new();
    for relative in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .filter_map(|path| std::str::from_utf8(path).ok())
    {
        let is_markdown_under_docs = relative.starts_with("docs/") && relative.ends_with(".md");
        let is_sprint_or_triage_artifact =
            relative.starts_with(".sprints/") || relative.starts_with(".triage/");
        if !(is_markdown_under_docs || is_sprint_or_triage_artifact) {
            continue;
        }
        let path = root.join(relative);
        let contents = read_source(&path);
        for (line_number, line) in contents.lines().enumerate() {
            if ["<<<<<<<", "=======", ">>>>>>>"]
                .iter()
                .any(|marker| line.starts_with(marker))
            {
                findings.push(format!("{}:{}: {}", path.display(), line_number + 1, line));
            }
        }
    }
    assert!(
        findings.is_empty(),
        "tracked documentation contains merge-conflict markers:\n{}",
        findings.join("\n")
    );
}

#[test]
fn al3_received_hook_is_single_receiver_side_path_without_detached_work() {
    let root = workspace_root();
    let router = read_source(&root.join("crates/atm-http-runtime/src/storage_and_nudge_router.rs"));
    let send_module = read_source(&root.join("crates/atm-core/src/send/mod.rs"));
    // Scan the full split write module (facade + submodules) so no
    // prohibited construct can hide in a submodule file.
    let write_module = ["mod.rs", "pipeline.rs", "acknowledgement.rs"]
        .iter()
        .map(|file| read_source(&root.join("crates/atm-core/src/write").join(file)))
        .collect::<String>();
    let received_hook_selector =
        read_source(&root.join("crates/atm-daemon-bootstrap/src/received_hook_selector.rs"));
    let strip_comments = |source: &str| {
        source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let send_module_code = strip_comments(&send_module);
    let write_module_code = strip_comments(&write_module);

    let finish = router
        .find("prepared.finish(&self.service_runtime, self.observability.as_ref())")
        .expect("AL.3 must finish the durable write before receiver-hook routing");
    let dispatch = router
        .find(".emit_received_hook(committed.received_hook_dispatches, deadline)")
        .expect("AL.3 must route the received hook through the canonical typed router");
    assert!(
        finish < dispatch,
        "AL.3 must invoke the received hook only after durable write completion"
    );
    assert_eq!(
        router
            .matches(".emit_received_hook(committed.received_hook_dispatches, deadline)")
            .count(),
        1,
        "all UDS, TCP, and peer ingress adapters must converge on one post-persistence hook call site"
    );
    assert!(
        router.contains("let newly_persisted = prepared.is_newly_persisted();")
            && router.contains("if committed.newly_persisted {"),
        "the one hook-routing decision must state the new-versus-idempotent persistence disposition explicitly"
    );
    assert_eq!(
        router.matches("async fn emit_received_hook(").count(),
        1,
        "the router must retain exactly one receiver-hook invocation site"
    );
    assert!(
        router.contains("deadline.expired()"),
        "AL.3 must skip receiver-hook work once the inherited request deadline is exhausted"
    );
    for prohibited in ["thread::spawn", "tokio::spawn", "sync_channel"] {
        assert!(
            !send_module_code.contains(prohibited),
            "the canonical core write planner must not create detached receiver-hook `{prohibited}` work"
        );
        assert!(
            !write_module_code.contains(prohibited),
            "the canonical core write planner must not create detached receiver-hook `{prohibited}` work"
        );
        assert!(
            !received_hook_selector.contains(prohibited),
            "the replacement receiver selector must not create detached receiver-hook `{prohibited}` work"
        );
    }
    assert!(
        write_module_code.contains("pub fn build_received_hook_dispatches")
            && !write_module_code.contains("load_message_record")
            && !send_module_code.contains("load_message_record"),
        "the canonical PreparedWrite seam must retain in-memory received-hook planning without reloading a committed record"
    );
    assert!(
        send_module.contains("pub enum NudgeMode")
            && write_module_code.contains("NudgeMode::Deferred"),
        "the canonical send/write modules must define and apply the deferred queue nudge mode"
    );
    assert!(
        received_hook_selector.contains("impl MessageReceivedHookSelector"),
        "the replacement bootstrap must remain the concrete received-hook selector implementation"
    );
    assert!(
        received_hook_selector.contains("NudgeKind::Queue"),
        "the replacement receiver selector must explicitly handle queue-kind dispatches"
    );

    // `atm-graft/src/runtime/` is deliberately excluded: it is the
    // independently-started receiver implementation, not an outbound client.
    for path in [
        "crates/atm/src",
        "crates/atm-daemon-client/src",
        "crates/atm-graft/src/lib.rs",
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

#[test]
fn av3_handler_region_scanner_isolated_to_named_handlers() {
    let source = r#"
async fn list_messages() { reader.list().await }
async fn receive_mailbox() { reader.read().await }
async fn heartbeat() { ControlPathSyncBridge::run(); }
"#;
    let region = av3_handler_region(source, &["list_messages", "receive_mailbox"]);
    assert!(region.contains("reader . list") && region.contains("reader . read"));
    assert!(
        !region.contains("ControlPathSyncBridge"),
        "handler-region scans must not confuse an allowed residual control-path caller with a read handler"
    );
}

#[test]
fn av3_bridge_run_scanner_reports_the_enclosing_function() {
    let source = r#"
struct Router {
    control_path_sync_bridge: ControlPathSyncBridge,
}
impl Router {
    async fn send(&self) { self.control_path_sync_bridge.run().await; }
    async fn receive_mailbox(&self) { reader.read().await; }
}
"#;
    assert_eq!(
        av3_bridge_run_call_sites_by_enclosing_fn(source),
        BTreeSet::from(["send".to_owned()]),
    );
}

#[test]
fn av3_ast_scanners_ignore_comments_literals_and_test_module_decoys() {
    let source = r##"
#[cfg(test)] mod tests { async fn list_messages() { FreshSemaphoreGate::acquire(); } }
async fn list_messages<T>() { /* } fn fake() */ let note = "{ fn fake() }"; reader.list().await; }
"##;
    let region = av3_handler_region(source, &["list_messages"]);
    assert!(region.contains("reader . list"));
    assert!(!region.contains("FreshSemaphoreGate"));
}

#[test]
fn av3_bridge_scanner_catches_arc_fields_and_inline_construction() {
    let source = r#"
struct Router { bridge: std::sync::Arc<ControlPathSyncBridge> }
impl Router {
  async fn field_path(&self) { self.bridge.run().await; }
  async fn inline_path<T>(&self) { let bridge = ControlPathSyncBridge::new(); bridge.run().await; }
}
"#;
    assert_eq!(
        av3_bridge_run_call_sites_by_enclosing_fn(source),
        BTreeSet::from(["field_path".to_owned(), "inline_path".to_owned()]),
    );
}

#[test]
fn av3_post_cutover_read_handlers_reject_legacy_blocking_dependencies() {
    let router = read_source(
        &workspace_root().join("crates/atm-http-runtime/src/storage_and_nudge_router.rs"),
    );
    // AV.1b is intentionally in flight while AV.3 scaffolding is prepared.
    // Its port is the atomic activation marker for this source guard.
    if !router.contains("AsyncMailboxRuntime") {
        return;
    }
    let handlers = read_handler_names();
    let read_region = av3_handler_region(
        &router,
        &handlers.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    av3_assert_allowlisted_types(
        &read_region,
        &[
            "AsyncMailboxRuntime",
            "DoctorProjection",
            "ApiResponse",
            "ResponseEnvelope",
        ],
        "read-handler",
    );
    for prohibited in [
        "BlockingCoreBridge",
        "ControlPathSyncBridge",
        "spawn_blocking",
        "list_mail_with_runtime",
        "peek_mail_with_runtime",
        "read_mail_with_runtime",
        "run_doctor_with_runtime",
        "MessageStore::list_messages",
        "StorageWriterIngress",
    ] {
        assert!(
            !read_region.contains(prohibited),
            "AV.3 read-handler dependency boundary rejects `{prohibited}`"
        );
    }
}

#[test]
fn av3_allowlist_rejects_a_freshly_named_read_gate() {
    let region = av3_handler_region(
        "async fn list_messages() { FreshSemaphoreGate::acquire().await; }",
        &["list_messages"],
    );
    let result = std::panic::catch_unwind(|| {
        av3_assert_allowlisted_types(&region, &["AsyncMailboxRuntime"], "read-handler")
    });
    assert!(
        result.is_err(),
        "the positive allowlist must reject renamed gates"
    );
}

#[test]
fn av3_async_mailbox_runtime_composition_rejects_read_serialization_primitives() {
    let mailbox_runtime = workspace_root().join("crates/atm-runtime/src/mailbox_runtime.rs");
    if !mailbox_runtime.exists() {
        return;
    }
    let source = read_source(&mailbox_runtime);
    let implementation = av3_async_mailbox_runtime_impl_region(&source);
    assert!(
        source.contains("reader: Arc<dyn AsyncMailboxReader"),
        "the AsyncMailboxRuntime composition must use the reader-lane capability"
    );
    av3_assert_allowlisted_item_types(
        &implementation,
        &[
            "AsyncMailboxRuntime",
            "StorageAsyncMailboxRuntime",
            "AsyncMailboxReader",
            "StateHandoffSupervisor",
            "MailboxSelectionRequest",
            "MailboxSelectionCandidate",
            "MailboxSelectionResult",
            "SelectedMailboxMessage",
            "MailboxScope",
            "Message",
            "MessageKey",
            "MessageQuery",
            "RequestDeadline",
            "ReadDeadline",
            "AtmError",
            "ReadLaneError",
            "Result",
            "Ok",
            "Self",
        ],
        "AsyncMailboxRuntime composition",
    );
    for prohibited in [
        "spawn_blocking",
        "ControlPathSyncBridge",
        "BlockingCoreBridge",
        "StorageWriterIngress",
        "Semaphore",
        "Mutex",
    ] {
        assert!(
            !implementation.contains(prohibited),
            "AV.3 composition boundary rejects read serialization primitive `{prohibited}`"
        );
    }
}

#[test]
fn av3_async_mailbox_runtime_composition_rejects_production_writer_and_gate_fixtures() {
    for prohibited in ["FreshSemaphoreGate", "AsyncMessageStore"] {
        let source = format!(
            "impl AsyncMailboxRuntime for StorageAsyncMailboxRuntime {{ fn compose(&self) {{ {prohibited}::acquire(); }} }}"
        );
        let implementation = av3_async_mailbox_runtime_impl_region(&source);
        let rejection = std::panic::catch_unwind(|| {
            av3_assert_allowlisted_item_types(
                &implementation,
                &["AsyncMailboxRuntime", "StorageAsyncMailboxRuntime", "Self"],
                "synthetic AsyncMailboxRuntime composition",
            )
        });
        assert!(
            rejection.is_err(),
            "the production composition guard must reject `{prohibited}`"
        );
    }
}

#[test]
fn av3_async_mailbox_runtime_composition_excludes_cfg_test_writer_fake() {
    let source = r#"
impl AsyncMailboxRuntime for StorageAsyncMailboxRuntime { fn compose(&self) {} }
#[cfg(test)] mod tests { fn writer_fake() { AsyncMessageStore::acquire(); } }
"#;
    let implementation = av3_async_mailbox_runtime_impl_region(source);
    assert!(
        !implementation.contains("AsyncMessageStore"),
        "the production composition region must exclude cfg(test) writer fakes"
    );
    av3_assert_allowlisted_item_types(
        &implementation,
        &["AsyncMailboxRuntime", "StorageAsyncMailboxRuntime", "Self"],
        "synthetic AsyncMailboxRuntime composition",
    );
}

#[test]
fn av3_control_path_bridge_call_sites_are_the_exact_residual_set_after_rename() {
    let router = read_source(
        &workspace_root().join("crates/atm-http-runtime/src/storage_and_nudge_router.rs"),
    );
    // The D1 rename is intentionally held until AV.1b is merged forward.
    if !router.contains("ControlPathSyncBridge") {
        return;
    }
    let production_router = router
        .split("\n#[cfg(test)]\nmod tests")
        .next()
        .expect("router production/test split");
    assert!(
        !production_router.contains("BlockingCoreBridge"),
        "D1 deletes the legacy BlockingCoreBridge identifier from production source"
    );
    let residual = BTreeSet::from([
        "send".to_owned(),
        "clear_messages".to_owned(),
        "heartbeat".to_owned(),
        "queue_get_next".to_owned(),
        "graft_receiver_register".to_owned(),
        "graft_receiver_refresh".to_owned(),
        "graft_receiver_unregister".to_owned(),
        "graft_receiver_lookup".to_owned(),
    ]);
    assert_eq!(
        av3_bridge_run_call_sites_by_enclosing_fn(production_router),
        residual,
        "AV-FU-1 owns the only permitted residual ControlPathSyncBridge::run call sites"
    );
}

#[test]
fn av3_blocking_core_bridge_is_absent_from_all_production_crates_after_rename() {
    let root = workspace_root();
    let router = read_source(&root.join("crates/atm-http-runtime/src/storage_and_nudge_router.rs"));
    if !router.contains("ControlPathSyncBridge") {
        return;
    }
    let findings = av3_identifier_findings(&root.join("crates"), "BlockingCoreBridge");
    assert!(
        findings.is_empty(),
        "AV.3 D1 forbids BlockingCoreBridge in production crates: {findings:?}"
    );
}

#[test]
fn av3_blocking_core_bridge_crate_scan_rejects_a_stray_reintroduction() {
    assert_eq!(
        av3_identifier_findings_in_source("struct BlockingCoreBridge;", "BlockingCoreBridge"),
        vec!["BlockingCoreBridge"]
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Av3WriterIngressPath {
    None,
    StateHandoffSupervisor,
    DirectWriter,
}

trait Av3InstrumentedWriterIngress {
    fn record(&mut self, handler: &str, path: Av3WriterIngressPath);
}

#[derive(Default)]
struct Av3IngressRecorder(BTreeMap<String, Av3WriterIngressPath>);

impl Av3InstrumentedWriterIngress for Av3IngressRecorder {
    fn record(&mut self, handler: &str, path: Av3WriterIngressPath) {
        self.0.insert(handler.to_owned(), path);
    }
}

fn av3_assert_supervised_read_ingress(handler: &str, path: Av3WriterIngressPath) {
    assert!(
        matches!(
            path,
            Av3WriterIngressPath::None | Av3WriterIngressPath::StateHandoffSupervisor
        ),
        "D2b `{handler}` must not obtain response data through writer ingress"
    );
}

#[test]
fn av3_d2b_scaffold_accepts_only_the_supervised_read_state_handoff() {
    let router = read_source(
        &workspace_root().join("crates/atm-http-runtime/src/storage_and_nudge_router.rs"),
    );
    if !router.contains("AsyncMailboxRuntime") {
        return;
    }
    let mut recorder = Av3IngressRecorder::default();
    for handler in read_handler_names() {
        let body = av3_handler_region(&router, &[&handler]);
        let path = if handler == "receive_messages" && body.contains("StateHandoffSupervisor") {
            Av3WriterIngressPath::StateHandoffSupervisor
        } else if body.contains("StorageWriterIngress") || body.contains("submit_async") {
            Av3WriterIngressPath::DirectWriter
        } else {
            Av3WriterIngressPath::None
        };
        recorder.record(&handler, path);
    }
    for (endpoint, path) in recorder.0 {
        av3_assert_supervised_read_ingress(&endpoint, path);
    }
}

#[test]
fn av3_d2b_scaffold_rejects_a_writer_lane_read_fixture() {
    let source = "async fn list_messages() { StorageWriterIngress::submit_async(); }";
    let body = av3_handler_region(source, &["list_messages"]);
    let path = if body.contains("StorageWriterIngress") {
        Av3WriterIngressPath::DirectWriter
    } else {
        Av3WriterIngressPath::None
    };
    let rejection =
        std::panic::catch_unwind(|| av3_assert_supervised_read_ingress("list_messages", path));
    assert!(
        rejection.is_err(),
        "D2b must reject a read handler routed to the writer lane"
    );
}

#[test]
fn herdr_constructors_have_one_composition_root_call_site() {
    let root = workspace_root();
    let mut rust_files = Vec::new();
    collect_rust_files(&root.join("crates"), &mut rust_files);
    rust_files.retain(|path| !path.starts_with(root.join("crates/atm-architecture/tests")));

    let sources = rust_files
        .iter()
        .map(|path| read_source(path))
        .collect::<Vec<_>>();
    let breaker_calls = sources
        .iter()
        .map(|source| source.matches("HerdrSpawnBreaker::new(").count())
        .sum::<usize>();
    let invoker_calls = sources
        .iter()
        .map(|source| source.matches("HerdrProcessInvoker::new(").count())
        .sum::<usize>();
    assert_eq!(
        breaker_calls, 1,
        "HerdrSpawnBreaker must have one composition call site"
    );
    assert_eq!(
        invoker_calls, 1,
        "HerdrProcessInvoker must have one composition call site"
    );

    let bootstrap =
        read_source(&root.join("crates/atm-daemon-bootstrap/src/replacement_handler.rs"));
    // `build_replacement_handler` is the last item in this module, so its
    // body runs to end-of-file; there is no following `fn ` to bound on.
    let composition = bootstrap
        .split_once("fn build_replacement_handler")
        .map(|(_, tail)| tail)
        .expect("replacement handler composition function must exist");
    assert!(composition.contains("HerdrSpawnBreaker::new("));
    assert!(composition.contains("HerdrProcessInvoker::new("));
}

#[test]
fn herdr_test_utils_are_dev_dependencies_only() {
    let root = workspace_root();
    for crate_name in ["atm-daemon-bootstrap", "atm-http-runtime"] {
        let manifest: toml::Value = toml::from_str(&read_source(
            &root.join(format!("crates/{crate_name}/Cargo.toml")),
        ))
        .expect("consumer manifest must parse");
        let production = manifest
            .get("dependencies")
            .and_then(|table| table.get("atm-herdr"));
        assert!(
            !production.is_some_and(|dependency| {
                dependency
                    .get("features")
                    .and_then(toml::Value::as_array)
                    .is_some_and(|features| {
                        features
                            .iter()
                            .any(|feature| feature.as_str() == Some("test-utils"))
                    })
            }),
            "{crate_name} production atm-herdr dependency must not enable test-utils"
        );
        let dev = manifest
            .get("dev-dependencies")
            .and_then(|table| table.get("atm-herdr"))
            .expect("consumer must declare atm-herdr test dependency");
        assert!(
            dev.get("features")
                .and_then(toml::Value::as_array)
                .is_some_and(|features| features
                    .iter()
                    .any(|feature| feature.as_str() == Some("test-utils"))),
            "{crate_name} must enable atm-herdr test-utils only in dev-dependencies"
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
        root.join("boundaries/atm/local-socket-client-transport.toml"),
        root.join("boundaries/atm-graft/shared-client-consumer.toml"),
        root.join("boundaries/atm-http-runtime/http-runtime.toml"),
        root.join("boundaries/atm-http-runtime/member-state-transition-sink.toml"),
        root.join("boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml"),
        root.join("boundaries/atm-runtime/runtime-composition.toml"),
        root.join("boundaries/atm-storage/tls.toml"),
        root.join("boundaries/atm-template-sc-compose/sc-composer.toml"),
        root.join("boundaries/atm-herdr/herdr-process-adapter.toml"),
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
    let directory = root.join("boundaries/atm-daemon");
    if !directory.exists() {
        return Vec::new();
    }
    let mut files = fs::read_dir(directory)
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

/// AV.3 source-scanner primitives deliberately operate on function bodies,
/// rather than the whole router file. The residual control-path bridge stays
/// in that file after AV.1b, so file-wide token checks would either reject an
/// allowed mutation/control-path caller or permit a read-handler regression.
fn av3_handler_region(source: &str, handlers: &[&str]) -> String {
    let functions = av3_production_functions(source);
    handlers
        .iter()
        .map(|handler| {
            functions.get(*handler).unwrap_or_else(|| {
                panic!("AV.3 handler `{handler}` is missing from production source")
            })
        })
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

fn av3_assert_allowlisted_types(region: &str, allowed: &[&str], scope: &str) {
    let syntax = syn::parse_file(&format!("fn gate() {region}"))
        .unwrap_or_else(|error| panic!("AV.3 {scope} fixture must parse: {error}"));
    av3_assert_allowlisted_syntax_types(&syntax, allowed, scope);
}

fn av3_assert_allowlisted_item_types(region: &str, allowed: &[&str], scope: &str) {
    let syntax = syn::parse_file(region)
        .unwrap_or_else(|error| panic!("AV.3 {scope} fixture must parse: {error}"));
    av3_assert_allowlisted_syntax_types(&syntax, allowed, scope);
}

fn av3_assert_allowlisted_syntax_types(syntax: &syn::File, allowed: &[&str], scope: &str) {
    struct TypeVisitor {
        names: BTreeSet<String>,
    }
    impl<'ast> Visit<'ast> for TypeVisitor {
        fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
            for segment in &path.path.segments {
                let name = segment.ident.to_string();
                if name.chars().next().is_some_and(char::is_uppercase) {
                    self.names.insert(name);
                }
            }
            syn::visit::visit_expr_path(self, path);
        }
        fn visit_path(&mut self, path: &'ast syn::Path) {
            if let Some(segment) = path.segments.last() {
                let name = segment.ident.to_string();
                if name.chars().next().is_some_and(char::is_uppercase) {
                    self.names.insert(name);
                }
            }
            syn::visit::visit_path(self, path);
        }
    }
    let mut visitor = TypeVisitor {
        names: BTreeSet::new(),
    };
    visitor.visit_file(syntax);
    let unexpected = visitor
        .names
        .into_iter()
        .filter(|name| !allowed.contains(&name.as_str()))
        .collect::<Vec<_>>();
    assert!(
        unexpected.is_empty(),
        "AV.3 {scope} allowlist rejects unlisted type(s): {unexpected:?}"
    );
}

fn av3_async_mailbox_runtime_impl_region(source: &str) -> String {
    let syntax =
        syn::parse_file(source).expect("AV.3 AsyncMailboxRuntime source must parse as Rust");
    syntax
        .items
        .into_iter()
        .filter_map(|item| match item {
            syn::Item::Impl(implementation)
                if !implementation
                    .attrs
                    .iter()
                    .any(is_test_configuration_attribute)
                    && implementation.self_ty.to_token_stream().to_string()
                        == "StorageAsyncMailboxRuntime"
                    && implementation.trait_.as_ref().is_some_and(|(_, path, _)| {
                        path.segments
                            .last()
                            .is_some_and(|segment| segment.ident == "AsyncMailboxRuntime")
                    }) =>
            {
                Some(implementation.to_token_stream().to_string())
            }
            _ => None,
        })
        .next()
        .expect("AV.1a composition must implement AsyncMailboxRuntime")
}

fn av3_identifier_findings(root: &Path, identifier: &str) -> Vec<String> {
    let mut files = Vec::new();
    collect_rust_files(root, &mut files);
    files
        .into_iter()
        .filter_map(|path| {
            (!av3_identifier_findings_in_source(&read_source(&path), identifier).is_empty())
                .then(|| path.display().to_string())
        })
        .collect()
}

fn av3_identifier_findings_in_source(source: &str, identifier: &str) -> Vec<String> {
    let syntax = syn::parse_file(source).expect("AV.3 production source must parse");
    struct IdentifierVisitor<'a> {
        identifier: &'a str,
        findings: Vec<String>,
    }
    impl<'ast> Visit<'ast> for IdentifierVisitor<'_> {
        fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
            if item.attrs.iter().any(is_test_configuration_attribute) {
                return;
            }
            syn::visit::visit_item_mod(self, item);
        }
        fn visit_ident(&mut self, ident: &'ast syn::Ident) {
            if ident == self.identifier {
                self.findings.push(ident.to_string());
            }
            syn::visit::visit_ident(self, ident);
        }
    }
    let mut visitor = IdentifierVisitor {
        identifier,
        findings: Vec::new(),
    };
    visitor.visit_file(&syntax);
    visitor.findings
}

fn read_handler_names() -> Vec<String> {
    include_str!("../../../.just/allowlists/read_concurrency_handlers.txt")
        .lines()
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Enumerate enclosing functions of calls routed through the narrowly named
/// AV.3 residual bridge. The field name is discovered from its type so this is
/// a call-site policy instead of a second field-name API.
fn av3_bridge_run_call_sites_by_enclosing_fn(source: &str) -> BTreeSet<String> {
    let syntax = syn::parse_file(source).expect("AV.3 source must parse as Rust");
    let fields = syntax
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Struct(item) => Some(item),
            _ => None,
        })
        .flat_map(|item| item.fields.iter())
        .filter_map(|field| {
            field
                .ty
                .to_token_stream()
                .to_string()
                .contains("ControlPathSyncBridge")
                .then(|| field.ident.as_ref().map(ToString::to_string))
                .flatten()
        })
        .collect::<BTreeSet<_>>();
    av3_production_functions(source)
        .into_iter()
        .filter_map(|(name, body)| {
            let field_call = fields
                .iter()
                .any(|field| body.contains(&format!("self . {field}")));
            ((body.contains("ControlPathSyncBridge") || field_call) && body.contains(". run"))
                .then_some(name)
        })
        .collect()
}

fn av3_production_functions(source: &str) -> BTreeMap<String, String> {
    let syntax = syn::parse_file(source).expect("AV.3 source must parse as Rust");
    let mut functions = BTreeMap::new();
    for item in syntax.items {
        match item {
            syn::Item::Fn(function) => {
                functions.insert(
                    function.sig.ident.to_string(),
                    function.block.to_token_stream().to_string(),
                );
            }
            syn::Item::Impl(implementation) => {
                for member in implementation.items {
                    if let syn::ImplItem::Fn(function) = member {
                        functions.insert(
                            function.sig.ident.to_string(),
                            function.block.to_token_stream().to_string(),
                        );
                    }
                }
            }
            syn::Item::Mod(module) if module.attrs.iter().any(is_test_configuration_attribute) => {}
            _ => {}
        }
    }
    functions
}

/// Returns the brace-delimited body (inclusive of the braces) of the first
/// `fn {fn_name}(` occurrence in `source`, so fitness assertions can pin
/// wiring inside one specific function instead of matching anywhere in the
/// file (for example inside an unrelated test).
fn extract_fn_body<'source>(source: &'source str, fn_name: &str) -> &'source str {
    let marker = format!("fn {fn_name}(");
    let signature_start = source
        .find(&marker)
        .unwrap_or_else(|| panic!("function `{fn_name}` not found in source"));
    let body_start = source[signature_start..]
        .find('{')
        .map(|offset| signature_start + offset)
        .unwrap_or_else(|| panic!("function `{fn_name}` has no body"));
    let mut depth = 0usize;
    for (offset, character) in source[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[body_start..=body_start + offset];
                }
            }
            _ => {}
        }
    }
    panic!("function `{fn_name}` body is not closed")
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
