#![allow(
    deprecated,
    reason = "daemon bootstrap still forwards the legacy atm-core roster boundary while retained callers migrate to canonical storage seams"
)]

use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
#[cfg(unix)]
use std::num::NonZeroU32;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Once};
use std::time::Duration;

use atm_core::LocalFileNonClaudeOutbound;
use atm_core::boundary::{NonClaudeOutbound, RosterStore, TemplateComposer};
use atm_core::error::AtmError;
#[cfg(unix)]
use atm_core::home::HOST_RUNTIME_SOCKET_FILE;
use atm_core::home::current_host_runtime_scope;
use atm_core::local_http::LOCAL_HTTP_RECORD_FILENAME;
use atm_core::observability::{
    CommandEvent, NullObservability, ObservabilityPort, action_name, outcome_label,
};
use atm_core::peer_wire::{PeerWireMode, PeerWireSecurity};
use atm_core::send::input::DEFAULT_MESSAGE_MAX_BYTES;
use atm_core::types::HostName;
use atm_core::types::{AgentName, TeamName};
use atm_http_runtime::{
    AcceptedPeerStream, DirectPeerTcpConfig, EstablishedPeerStream, HttpRuntimeBuilder,
    HttpRuntimeConfig, LoopbackTcpConfig, NonZeroDuration, PeerStreamAdapter, PeerStreamFuture,
    RuntimeHealth, RuntimeLimits, RuntimeTimeouts, StorageAndNudgeRouter,
};
use atm_runtime::{RuntimeAssembly, RuntimeAssemblyInputs, assemble_runtime};
use atm_storage_rusqlite::SqliteStorageFactory;
use peer_tls::MtlsPeerStreamAdapter;
use tokio::net::TcpStream;

mod owner_gate;
mod received_hook_selector;

pub use owner_gate::DaemonOwnerGuard;
pub use received_hook_selector::active_received_hook_selector;
#[cfg(feature = "benchmark-harness")]
pub use received_hook_selector::{BenchmarkHookMode, benchmark_received_hook_selector};

static INSTALL_RETAINED_RUNTIME_FACTORY: Once = Once::new();
/// Architecture §21.6.4's single replacement-daemon drain deadline.
pub const REPLACEMENT_DRAIN_DEADLINE: Duration = Duration::from_secs(5);
/// Space reserved above one valid message body for the canonical HTTP JSON
/// envelope (routing, identity, acknowledgement, and protocol metadata).
///
/// It keeps the 1 MiB message contract independent from HTTP framing while
/// remaining a bounded admission limit. The wire request's `max_message_bytes`
/// can only lower the body policy; it cannot raise this server ceiling.
const CANONICAL_WRITE_ENVELOPE_OVERHEAD_BYTES: usize = 64 * 1024;

/// Identity values captured once at the daemon bootstrap boundary.
///
/// The daemon library intentionally does not read `ATM_TEAM` or
/// `ATM_IDENTITY` itself.  Capturing these values here keeps the environment
/// read at process startup and lets the runtime-health reporter receive typed
/// values rather than consulting a mutable process environment on requests.
#[derive(Debug, Clone, Default)]
pub struct DaemonLaunchIdentity {
    pub team: Option<TeamName>,
    pub identity: Option<AgentName>,
}

/// Resolve the daemon's launch identity before the runtime starts serving.
pub fn resolve_daemon_launch_identity() -> DaemonLaunchIdentity {
    DaemonLaunchIdentity {
        team: atm_core::caller_context::read_cli_team_from_env_or_warn(
            "atm_daemon_bootstrap::resolve_daemon_launch_identity",
        ),
        identity: atm_core::caller_context::read_cli_identity_from_env_or_warn(
            "atm_daemon_bootstrap::resolve_daemon_launch_identity",
        ),
    }
}

/// Parse the sole non-durable peer-wire launch policy before composition.
///
/// The process mode deliberately has no environment, database, or adapter
/// availability source: those inputs can cause a startup error but cannot
/// select plaintext.
pub fn parse_peer_wire_mode(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<PeerWireMode, AtmError> {
    if std::env::var_os("ATM_PEER_WIRE_SECURITY").is_some() {
        return Err(AtmError::peer_wire_mode_source_forbidden(
            "ATM_PEER_WIRE_SECURITY is forbidden; use --peer-wire-security at daemon launch",
        ));
    }
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let mut mode = None;
    while let Some(argument) = arguments.next() {
        let argument = argument.into_string().map_err(|_| {
            AtmError::peer_wire_mode_invalid("daemon launch arguments must be valid UTF-8")
        })?;
        let value = if argument == "--peer-wire-security" {
            Some(arguments.next().ok_or_else(|| {
                AtmError::peer_wire_mode_invalid(
                    "--peer-wire-security requires `mutual-tls` or `plaintext-test`",
                )
            })?)
        } else {
            argument
                .strip_prefix("--peer-wire-security=")
                .map(std::ffi::OsString::from)
        };
        let Some(value) = value else {
            continue;
        };
        let value = value.into_string().map_err(|_| {
            AtmError::peer_wire_mode_invalid("peer-wire launch mode must be valid UTF-8")
        })?;
        let parsed = match value.as_str() {
            "mutual-tls" => PeerWireMode::mtls(),
            "plaintext-test" => PeerWireMode::plaintext_test(),
            _ => {
                return Err(AtmError::peer_wire_mode_invalid(
                    "--peer-wire-security accepts only `mutual-tls` or `plaintext-test`",
                ));
            }
        };
        if mode.replace(parsed).is_some() {
            return Err(AtmError::peer_wire_mode_invalid(
                "--peer-wire-security may be supplied only once",
            ));
        }
    }
    Ok(mode.unwrap_or_default())
}

struct BootstrapMtlsStreamAdapter {
    adapter: Arc<MtlsPeerStreamAdapter>,
}

impl PeerStreamAdapter for BootstrapMtlsStreamAdapter {
    fn connect<'a>(
        &'a self,
        stream: TcpStream,
        peer: &'a HostName,
    ) -> PeerStreamFuture<'a, EstablishedPeerStream> {
        Box::pin(async move {
            let stream: EstablishedPeerStream = Box::new(self.adapter.connect(stream, peer).await?);
            Ok(stream)
        })
    }

    fn accept<'a>(&'a self, stream: TcpStream) -> PeerStreamFuture<'a, AcceptedPeerStream> {
        Box::pin(async move {
            let (stream, source_host) = self.adapter.accept_with_peer(stream).await?;
            Ok(AcceptedPeerStream {
                source_host,
                stream: Box::new(stream),
            })
        })
    }
}

pub fn install_sqlite_retained_runtime_factory() {
    INSTALL_RETAINED_RUNTIME_FACTORY.call_once(|| {
        atm_core::runtime_install_hooks::install_retained_runtime_factory_for_daemon_bootstrap(
            default_local_runtime,
        );
    });
}

/// Assemble the host-scoped runtime through the one approved concrete backend
/// selection point. Callers receive only backend-neutral runtime handles.
pub fn assemble_host_runtime(
    config_current_dir: PathBuf,
    non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
) -> Result<RuntimeAssembly, AtmError> {
    assemble_host_runtime_with_template_composer(
        config_current_dir,
        non_claude_outbound,
        Some(template_composer()),
    )
}

/// Construct the approved renderer adapter for callers that only need local
/// composition (for example `atm compose`) and must not touch mailbox state.
pub fn template_composer() -> Arc<dyn TemplateComposer> {
    Arc::new(atm_template_sc_compose::ScComposeTemplateComposer::new())
}

/// Assemble the host-scoped runtime with the sole bootstrap-owned template
/// port. The concrete adapter remains invisible to `atm-runtime` and every
/// caller downstream of this composition root.
pub fn assemble_host_runtime_with_template_composer(
    config_current_dir: PathBuf,
    non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
    template_composer: Option<Arc<dyn TemplateComposer>>,
) -> Result<RuntimeAssembly, AtmError> {
    assemble_runtime(RuntimeAssemblyInputs {
        host_runtime_scope: current_host_runtime_scope()?,
        storage_factory: Arc::new(SqliteStorageFactory::host_scoped()),
        config_current_dir,
        non_claude_outbound,
        template_composer,
        workflow_telemetry: None,
    })
}

/// Assemble the default local runtime for retained bootstrap consumers.
pub fn assemble_default_runtime() -> Result<RuntimeAssembly, AtmError> {
    let config_current_dir = std::env::current_dir().map_err(|_source| {
        AtmError::config("failed to resolve current directory for runtime assembly")
    })?;
    assemble_host_runtime(
        config_current_dir,
        Arc::new(LocalFileNonClaudeOutbound::new()),
    )
}

/// Assemble the system-daemon runtime without inspecting a caller workspace.
///
/// A LaunchAgent may start with a working directory whose `getcwd(2)` blocks
/// (for example while the workspace volume is being reconciled).  The daemon
/// must not depend on that directory: [`RuntimeAssembly::for_daemon`] removes
/// the workspace-backed config doctor before requests can be served.
pub fn assemble_daemon_runtime() -> Result<RuntimeAssembly, AtmError> {
    assemble_host_runtime(PathBuf::new(), Arc::new(LocalFileNonClaudeOutbound::new()))
        .map(RuntimeAssembly::for_daemon)
}

/// Starts the replacement Tokio/Axum daemon as the only active serving path.
///
/// The singleton guard is acquired before validation or binding. The runtime
/// owns its listener lifecycle; this bootstrap owns only concrete backend and
/// harness selection. No legacy daemon server, dispatcher, worker, or framing
/// module is referenced from this path.
pub async fn run_replacement_daemon() -> Result<(), AtmError> {
    run_replacement_daemon_with_observability(Arc::new(NullObservability)).await
}

/// Starts the shipped replacement daemon with the process-owned observability
/// adapter supplied by its binary entrypoint.
pub async fn run_replacement_daemon_with_observability(
    observability: Arc<dyn ObservabilityPort + Send + Sync>,
) -> Result<(), AtmError> {
    let peer_wire_mode = parse_peer_wire_mode(std::env::args_os())?;
    run_replacement_daemon_with_selector(
        observability,
        active_received_hook_selector,
        resolve_daemon_launch_identity(),
        peer_wire_mode,
    )
    .await
}

/// Starts the separately compiled benchmark daemon with an explicit hook mode.
///
/// This symbol is unavailable to the shipped `atm-daemon` binary because it
/// exists only behind the `benchmark-harness` feature.
#[cfg(feature = "benchmark-harness")]
pub async fn run_benchmark_daemon(hook_mode: BenchmarkHookMode) -> Result<(), AtmError> {
    let peer_wire_mode = parse_peer_wire_mode(std::env::args_os())?;
    run_replacement_daemon_with_selector(
        Arc::new(NullObservability),
        move |service_runtime| benchmark_received_hook_selector(service_runtime, hook_mode),
        resolve_daemon_launch_identity(),
        peer_wire_mode,
    )
    .await
}

fn build_replacement_handler(
    assembly: RuntimeAssembly,
    observability: Arc<dyn ObservabilityPort + Send + Sync>,
    selector_factory: impl FnOnce(
        atm_core::LocalServiceRuntime,
    ) -> Arc<dyn atm_core::boundary::MessageReceivedHookSelector>,
    daemon_launch_identity: &DaemonLaunchIdentity,
    peer_wire_mode: PeerWireMode,
    peer_stream_adapter: Option<Arc<dyn PeerStreamAdapter>>,
    runtime_health: RuntimeHealth,
) -> Result<Arc<StorageAndNudgeRouter>, AtmError> {
    let selector = selector_factory(assembly.service_runtime.clone());
    let handler = StorageAndNudgeRouter::new(
        assembly.service_runtime,
        observability,
        selector,
        atm_core::home::atm_home()?,
    )
    .with_runtime_health(runtime_health, assembly.doctor_ports)
    .with_daemon_context(atm_core::doctor::DoctorExecutionContext {
        team: daemon_launch_identity.team.clone(),
        identity: daemon_launch_identity.identity.clone(),
        version: Some(atm_core::protocol::ReleaseVersion::current()),
        cli_schema_version: Some(atm_core::protocol::CLI_SCHEMA_VERSION),
        http_api_version: Some(atm_core::protocol::HttpApiVersion::current()),
        peer_wire_security: Some(peer_wire_mode.security().as_launch_value().to_owned()),
    });
    let handler = match peer_stream_adapter {
        Some(adapter) => handler.with_peer_stream_adapter(adapter),
        None => handler,
    };
    Ok(Arc::new(handler))
}

/// Selects the optional mTLS stream adapter from the immutable daemon launch
/// mode.  Plaintext-test mode must not inspect, validate, or depend on the
/// TLS control-plane state: it keeps the existing direct-peer HTTP pipeline
/// intact without a stream wrapper.
fn peer_stream_adapter_for_mode(
    peer_wire_mode: PeerWireMode,
    build_mtls_adapter: impl FnOnce() -> Result<Arc<dyn PeerStreamAdapter>, AtmError>,
) -> Result<Option<Arc<dyn PeerStreamAdapter>>, AtmError> {
    match peer_wire_mode.security() {
        PeerWireSecurity::Mtls => build_mtls_adapter().map(Some),
        PeerWireSecurity::PlaintextTest => Ok(None),
    }
}

async fn run_replacement_daemon_with_selector(
    observability: Arc<dyn ObservabilityPort + Send + Sync>,
    selector_factory: impl FnOnce(
        atm_core::LocalServiceRuntime,
    ) -> Arc<dyn atm_core::boundary::MessageReceivedHookSelector>,
    daemon_launch_identity: DaemonLaunchIdentity,
    peer_wire_mode: PeerWireMode,
) -> Result<(), AtmError> {
    install_sqlite_retained_runtime_factory();
    let scope = current_host_runtime_scope()?;
    let _owner = DaemonOwnerGuard::acquire_at(scope.owner_lock.clone())?;
    let runtime_health = RuntimeHealth::with_owner(std::process::id());
    let assembly = assemble_daemon_runtime()?;
    let workflow_telemetry = assembly.workflow_telemetry.clone();
    let peer_stream_adapter = peer_stream_adapter_for_mode(peer_wire_mode, || {
        Ok(Arc::new(BootstrapMtlsStreamAdapter {
            adapter: Arc::new(MtlsPeerStreamAdapter::from_peer_config(
                assembly.peer_config_store.as_ref(),
            )?),
        }) as Arc<dyn PeerStreamAdapter>)
    })?;
    // The shipped daemon always keeps the injected receiver hook active.
    // Benchmark-only selection is available only from the separate binary.
    let handler = build_replacement_handler(
        assembly,
        Arc::clone(&observability),
        selector_factory,
        &daemon_launch_identity,
        peer_wire_mode,
        peer_stream_adapter.clone(),
        runtime_health.clone(),
    )?;
    let loopback = LoopbackTcpConfig::new(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        scope.runtime_root.as_ref().join(LOCAL_HTTP_RECORD_FILENAME),
        _owner.instance_id(),
    );
    let config = HttpRuntimeConfig::new(
        loopback,
        unix_socket_config(&scope)?,
        RuntimeLimits::new(
            NonZeroUsize::new(DEFAULT_MESSAGE_MAX_BYTES + CANONICAL_WRITE_ENVELOPE_OVERHEAD_BYTES)
                .expect("non-zero body limit"),
            NonZeroUsize::new(128).expect("non-zero connection limit"),
        ),
        RuntimeTimeouts::new(
            NonZeroDuration::new(Duration::from_secs(3)).expect("non-zero request timeout"),
            NonZeroDuration::new(REPLACEMENT_DRAIN_DEADLINE).expect("non-zero shutdown timeout"),
        ),
    );
    let config = config.with_direct_peer_tcp(DirectPeerTcpConfig::standard());
    let config = match peer_stream_adapter.as_ref() {
        Some(adapter) => config.with_peer_stream_adapter(Arc::clone(adapter)),
        None => config,
    };
    tracing::info!(
        peer_wire_security = peer_wire_mode.security().as_launch_value(),
        mtls_ready = peer_stream_adapter.is_some(),
        "replacement daemon selected peer-wire mode"
    );
    // A retained startup record carries only the selected public mode. The
    // concrete adapter remains opaque; never emit certificates, pins, keys,
    // or trust records from this composition boundary.
    if let (Some(team), Some(identity)) = (
        daemon_launch_identity.team.clone(),
        daemon_launch_identity.identity.clone(),
    ) && let Err(error) = observability.emit(CommandEvent {
        command: "atm-daemon",
        action: action_name("peer_wire_mode_selected"),
        outcome: outcome_label(peer_wire_mode.security().as_launch_value()),
        team,
        agent: identity.clone(),
        sender: identity,
        message_id: None,
        requires_ack: false,
        dry_run: false,
        task_id: None,
        error_code: None,
        error_message: None,
    }) {
        tracing::warn!(%error, "failed to retain peer-wire mode startup observability event");
    }
    let mut running = HttpRuntimeBuilder::new(config, handler)
        .with_runtime_health(runtime_health)
        .build()?
        .start()
        .await?;
    if let Err(error) = emit_ready_signal_if_requested() {
        // The process has not advertised readiness, so it must not retain an
        // otherwise-live listener when its supervisor handshake fails.
        let _ = running.begin_shutdown().finish().await;
        workflow_telemetry.shutdown().await;
        return Err(error);
    }
    tokio::select! {
        signal = wait_for_shutdown_signal() => {
            let signal = signal?;
            eprintln!("replacement ATM daemon received {}; starting graceful shutdown", signal.as_str());
        }
        _ = running.wait_for_server_stop() => {
            eprintln!("replacement ATM HTTP runtime server stopped unexpectedly; beginning cleanup");
            let result = match running.begin_shutdown().finish().await {
                Ok(_) => Err(AtmError::daemon_unavailable(
                    "replacement HTTP runtime server stopped unexpectedly",
                )),
                Err(error) => Err(error),
            };
            workflow_telemetry.shutdown().await;
            return result;
        }
    }
    let stopped = running.begin_shutdown().finish().await;
    workflow_telemetry.shutdown().await;
    let _stopped = stopped?;
    Ok(())
}

/// Emit the benchmark/supervisor marker only after every enabled replacement
/// listener has been bound and the runtime has marked itself ready.
fn emit_ready_signal_if_requested() -> Result<(), AtmError> {
    let requested = std::env::var_os("ATM_DAEMON_READY_STDOUT").is_some();
    let mut stdout = std::io::stdout().lock();
    write_ready_signal_if_requested(&mut stdout, requested)
}

fn write_ready_signal_if_requested(
    output: &mut impl Write,
    requested: bool,
) -> Result<(), AtmError> {
    if !requested {
        return Ok(());
    }
    writeln!(output, "ATM_DAEMON_READY").map_err(|source| {
        AtmError::daemon_unavailable_with_cause("failed to emit daemon ready signal", source)
    })?;
    output.flush().map_err(|source| {
        AtmError::daemon_unavailable_with_cause("failed to flush daemon ready signal", source)
    })
}

#[cfg(unix)]
fn unix_socket_config(
    scope: &atm_core::home::HostRuntimeScope,
) -> Result<Option<atm_http_runtime::UnixSocketConfig>, AtmError> {
    use std::os::unix::fs::MetadataExt;

    let uid = std::fs::metadata(scope.runtime_root.as_ref())
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to inspect daemon runtime directory ownership")
                .with_cause(source)
        })?
        .uid();
    Ok(unix_socket_config_for_uid(scope.runtime_root.as_ref(), uid))
}

#[cfg(unix)]
fn unix_socket_config_for_uid(
    runtime_root: &std::path::Path,
    uid: u32,
) -> Option<atm_http_runtime::UnixSocketConfig> {
    // `UnixSocketOwnerUid` deliberately excludes uid 0 so a socket cannot be
    // accidentally configured for a synthetic/unowned principal. A daemon
    // running as root still has a safe MVP listener: authenticated loopback.
    // Do not make that optional local adapter prevent process startup.
    let uid = NonZeroU32::new(uid)?;
    let mode = NonZeroU32::new(0o600).expect("owner-only socket mode is non-zero");
    Some(atm_http_runtime::UnixSocketConfig::new(
        runtime_root.join(HOST_RUNTIME_SOCKET_FILE),
        atm_http_runtime::UnixSocketOwnerUid::new(uid),
        atm_http_runtime::UnixSocketMode::new(mode),
    ))
}

#[cfg(not(unix))]
fn unix_socket_config(
    _scope: &atm_core::home::HostRuntimeScope,
) -> Result<Option<atm_http_runtime::UnixSocketConfig>, AtmError> {
    Ok(None)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShutdownSignal {
    Interrupt,
    #[cfg(unix)]
    Terminate,
}

impl ShutdownSignal {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Interrupt => "SIGINT",
            #[cfg(unix)]
            Self::Terminate => "SIGTERM",
        }
    }
}

async fn wait_for_shutdown_signal() -> Result<ShutdownSignal, AtmError> {
    // AL.8 uses Tokio's process signal facility so the replacement process has
    // no dedicated signal thread or blocking shutdown worker.
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = signal(SignalKind::terminate()).map_err(|source| {
            AtmError::daemon_unavailable("failed to install replacement daemon SIGTERM handler")
                .with_cause(source)
        })?;
        Ok(tokio::select! {
            _ = tokio::signal::ctrl_c() => ShutdownSignal::Interrupt,
            _ = terminate.recv() => ShutdownSignal::Terminate,
        })
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        Ok(ShutdownSignal::Interrupt)
    }
}

fn default_local_runtime() -> Result<atm_core::LocalServiceRuntime, AtmError> {
    assemble_default_runtime().map(|assembly| assembly.service_runtime)
}

/// Open the default SQLite boundary and expose only the approved roster seam.
///
/// # Errors
///
/// Returns [`AtmError`] when the default SQLite-backed retained runtime cannot
/// assemble its canonical boundary state.
pub fn with_default_roster_store<T>(
    f: impl FnOnce(&(dyn RosterStore + Send + Sync)) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    atm_runtime::with_installed_roster_store(f)
}

/// Open the default SQLite boundary and expose only the approved built-in
/// nudge-template override lookup seam.
///
/// # Errors
///
/// Returns [`AtmError`] when the default SQLite-backed retained runtime cannot
/// assemble its canonical override-store boundary state.
pub fn with_default_nudge_template_override_store<T>(
    f: impl FnOnce(
        &(dyn atm_core::boundary::NudgeTemplateOverrideStore + Send + Sync),
    ) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    let assembly = assemble_default_runtime()?;
    f(assembly.nudge_template_override_store.as_ref())
}

/// Open the default durable cross-host configuration boundary.
///
/// The returned trait owns only listener, certificate-reference, and exact
/// trusted-peer configuration; it exposes no transport or mailbox state.
pub fn with_default_peer_config_store<T>(
    f: impl FnOnce(&(dyn atm_storage::PeerConfigStore + Send + Sync)) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    let assembly = assemble_default_runtime()?;
    f(assembly.peer_config_store().as_ref())
}

/// Open the canonical roster and durable peer-authority seams from one
/// runtime assembly for CLI-only peer address normalization.
///
/// Keeping the two reads in the same assembly avoids accidentally composing
/// a roster from one runtime snapshot with peer trust from another. The
/// closure receives storage contracts only; no transport or daemon surface is
/// exposed.
///
/// # Errors
///
/// Returns [`AtmError`] when the default SQLite-backed retained runtime cannot
/// assemble its canonical boundary state.
pub fn with_default_peer_address_stores<T>(
    f: impl FnOnce(
        &(dyn atm_storage::RosterStore + Send + Sync),
        &(dyn atm_storage::PeerConfigStore + Send + Sync),
    ) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    let assembly = assemble_default_runtime()?;
    let roster_store = assembly.shared_roster_store_arc();
    let peer_config_store = assembly.peer_config_store();
    f(roster_store.as_ref(), peer_config_store.as_ref())
}

#[cfg(test)]
mod replacement_runtime_tests {
    use std::ffi::OsString;
    use std::time::Duration;

    use atm_core::boundary::TemplateSource;
    use atm_template_sc_compose::ScComposeTemplateComposer;
    use serde_json::Map;

    use super::{
        REPLACEMENT_DRAIN_DEADLINE, ShutdownSignal, assemble_host_runtime_with_template_composer,
        parse_peer_wire_mode, peer_stream_adapter_for_mode, write_ready_signal_if_requested,
    };

    #[test]
    fn ready_signal_is_absent_unless_requested() {
        let mut output = Vec::new();
        write_ready_signal_if_requested(&mut output, false).expect("disabled marker");
        assert!(output.is_empty());
    }

    #[test]
    fn ready_signal_is_emitted_for_the_started_replacement_runtime() {
        let mut output = Vec::new();
        write_ready_signal_if_requested(&mut output, true).expect("enabled marker");
        assert_eq!(output, b"ATM_DAEMON_READY\n");
    }

    #[test]
    fn replacement_runtime_uses_the_architecture_drain_deadline() {
        assert_eq!(REPLACEMENT_DRAIN_DEADLINE, Duration::from_secs(5));
    }

    #[test]
    fn peer_wire_mode_defaults_to_mutual_tls_and_accepts_only_launch_values() {
        let mutual_tls = parse_peer_wire_mode([OsString::from("atm-daemon")])
            .expect("mTLS is the secure default");
        assert_eq!(mutual_tls.security().as_launch_value(), "mutual-tls");

        let plaintext = parse_peer_wire_mode([
            OsString::from("atm-daemon"),
            OsString::from("--peer-wire-security=plaintext-test"),
        ])
        .expect("explicit plaintext test mode");
        assert_eq!(plaintext.security().as_launch_value(), "plaintext-test");

        let invalid = parse_peer_wire_mode([
            OsString::from("atm-daemon"),
            OsString::from("--peer-wire-security"),
            OsString::from("opportunistic"),
        ])
        .expect_err("no opportunistic or fallback mode exists");
        assert!(invalid.message().contains("mutual-tls"));
    }

    #[test]
    fn peer_wire_mode_rejects_duplicate_launch_values() {
        let error = parse_peer_wire_mode([
            OsString::from("atm-daemon"),
            OsString::from("--peer-wire-security"),
            OsString::from("mutual-tls"),
            OsString::from("--peer-wire-security=plaintext-test"),
        ])
        .expect_err("one launch mode must select the whole runtime");
        assert!(error.message().contains("only once"));
    }

    #[test]
    fn plaintext_test_release_mode_never_reads_invalid_tls_configuration() {
        let adapter = peer_stream_adapter_for_mode(
            atm_core::peer_wire::PeerWireMode::plaintext_test(),
            || -> Result<_, atm_core::error::AtmError> {
                panic!("plaintext-test must not inspect TLS peer configuration")
            },
        )
        .expect("plaintext-test directly preserves the peer HTTP pipeline");

        assert!(adapter.is_none());
    }

    #[test]
    fn shutdown_signal_labels_are_operator_actionable() {
        assert_eq!(ShutdownSignal::Interrupt.as_str(), "SIGINT");
        #[cfg(unix)]
        assert_eq!(ShutdownSignal::Terminate.as_str(), "SIGTERM");
    }

    #[test]
    fn replacement_bootstrap_injects_the_real_template_composer_port() {
        let raw = b"bootstrap adapter".to_vec();
        let adapter = ScComposeTemplateComposer::new();
        let temp = tempfile::tempdir().expect("temporary bootstrap directory");
        let assembly = assemble_host_runtime_with_template_composer(
            temp.path().to_path_buf(),
            std::sync::Arc::new(atm_core::LocalFileNonClaudeOutbound::new()),
            Some(std::sync::Arc::new(adapter)),
        )
        .expect("bootstrap accepts the core-owned template port");

        let composer = assembly
            .template_composer()
            .expect("port arrives through runtime assembly");
        let path = temp.path().join("bootstrap.txt.j2");
        std::fs::write(&path, &raw).expect("write bootstrap template");
        let source = TemplateSource::file_backed(
            raw.clone(),
            std::fs::canonicalize(&path).expect("canonical bootstrap template"),
        );
        let inspection = composer
            .inspect(&source)
            .expect("real inspection through port");
        assert_eq!(
            inspection.output_format,
            atm_core::boundary::TemplateOutputFormat::Text
        );
        assert_eq!(inspection.sha.as_str().len(), 64);
        let stored_source =
            TemplateSource::stored(raw, Some(atm_core::boundary::TemplateOutputFormat::Text));
        assert_eq!(
            composer
                .render_without_includes(&stored_source, &Map::new())
                .expect("real render through port")
                .text,
            "bootstrap adapter"
        );
    }

    #[cfg(unix)]
    #[test]
    fn root_uses_authenticated_loopback_without_making_uds_startup_mandatory() {
        assert!(super::unix_socket_config_for_uid(std::path::Path::new("/tmp"), 0).is_none());
    }
}
