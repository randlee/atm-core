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
use atm_core::atm_temp::{ProcessEnvSource, is_atm_temp_unset};
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
use atm_core::{resolve_atm_temp, validate_sweep_config};
use atm_herdr::HerdrProcessAdapter;
use atm_http_runtime::{
    AcceptedPeerStream, DirectPeerTcpConfig, EstablishedPeerStream, HttpRuntimeBuilder,
    HttpRuntimeConfig, LoopbackTcpConfig, NonZeroDuration, PeerPoolConfig, PeerStreamAdapter,
    PeerStreamFuture, RuntimeHealth, RuntimeLimits, RuntimeTimeouts, StorageAndNudgeRouter,
};
use atm_runtime::{RuntimeAssembly, RuntimeAssemblyInputs, assemble_runtime};
use atm_storage_rusqlite::SqliteStorageFactory;
use peer_tls::MtlsPeerStreamAdapter;
use tokio::net::TcpStream;

mod atm_temp_config;
mod atm_temp_sweeper_runtime;
mod bare_cli_runtime;
mod owner_gate;
mod peer_launch_config;
mod queue_drain;
mod received_hook_selector;
mod replacement_handler;

use atm_temp_config::daemon_atm_config;
use atm_temp_sweeper_runtime::AtmTempSweeperRuntime;
use bare_cli_runtime::BareCliRuntime;
use replacement_handler::{
    ReplacementHandlerConfig, SelectedPeerAdapterSelection, build_replacement_handler,
};

pub use owner_gate::DaemonOwnerGuard;
pub use peer_launch_config::{
    parse_direct_peer_port, parse_peer_pool_config, parse_peer_wire_mode,
};
pub use received_hook_selector::active_received_hook_selector;
pub use received_hook_selector::active_received_hook_selector_with_health;
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
    let direct_peer_port = parse_direct_peer_port(std::env::args_os())?;
    let peer_pool_config = parse_peer_pool_config(std::env::args_os())?;
    run_replacement_daemon_with_selector(
        observability,
        |service_runtime, herdr_process, runtime_health, bare_cli| {
            received_hook_selector::active_received_hook_selector_with_health_and_fifo(
                service_runtime,
                herdr_process,
                runtime_health,
                bare_cli.fifo(),
                bare_cli.queue_full_drops(),
            )
        },
        resolve_daemon_launch_identity(),
        peer_wire_mode,
        DirectPeerTcpConfig::configured(direct_peer_port),
        peer_pool_config,
        None,
    )
    .await
}

/// Starts the separately compiled benchmark daemon with an explicit hook mode.
///
/// This symbol is unavailable to the shipped `atm-daemon` binary because it
/// exists only behind the `benchmark-harness` feature. It retains the same
/// explicit peer-wire launch mode as the production Tokio/Axum daemon.
#[cfg(feature = "benchmark-harness")]
pub async fn run_benchmark_daemon(hook_mode: BenchmarkHookMode) -> Result<(), AtmError> {
    let peer_wire_mode = parse_peer_wire_mode(std::env::args_os())?;
    let direct_peer_port = parse_direct_peer_port(std::env::args_os())?;
    let peer_pool_config = parse_peer_pool_config(std::env::args_os())?;
    run_replacement_daemon_with_selector(
        Arc::new(NullObservability),
        move |service_runtime, herdr_process, runtime_health, bare_cli| {
            received_hook_selector::benchmark_received_hook_selector_with_health_and_fifo(
                service_runtime,
                hook_mode,
                herdr_process,
                runtime_health,
                bare_cli.fifo(),
                bare_cli.queue_full_drops(),
            )
        },
        resolve_daemon_launch_identity(),
        peer_wire_mode,
        DirectPeerTcpConfig::configured(direct_peer_port),
        peer_pool_config,
        Some(Arc::new(
            received_hook_selector::BenchmarkNoopHerdrProcessAdapter,
        )),
    )
    .await
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

fn replacement_runtime_config(
    scope: &atm_core::home::HostRuntimeScope,
    owner: &DaemonOwnerGuard,
    direct_peer_tcp: DirectPeerTcpConfig,
    peer_stream_adapter: &Option<Arc<dyn PeerStreamAdapter>>,
    peer_pool_config: PeerPoolConfig,
) -> Result<HttpRuntimeConfig, AtmError> {
    let loopback = LoopbackTcpConfig::new(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        scope.runtime_root.as_ref().join(LOCAL_HTTP_RECORD_FILENAME),
        owner.instance_id(),
    );
    Ok(replacement_runtime_config_with_direct_peer(
        loopback,
        unix_socket_config(scope)?,
        direct_peer_tcp,
        peer_stream_adapter,
        peer_pool_config,
    ))
}

/// Builds the maintained runtime configuration after bootstrap has selected
/// its one wire mode. Production supplies the fixed direct-peer port; tests
/// may supply an isolated ephemeral listener while exercising this exact
/// composition path.
fn replacement_runtime_config_with_direct_peer(
    loopback: LoopbackTcpConfig,
    unix_socket: Option<atm_http_runtime::UnixSocketConfig>,
    direct_peer_tcp: DirectPeerTcpConfig,
    peer_stream_adapter: &Option<Arc<dyn PeerStreamAdapter>>,
    peer_pool_config: PeerPoolConfig,
) -> HttpRuntimeConfig {
    let config = HttpRuntimeConfig::new(
        loopback,
        unix_socket,
        RuntimeLimits::new(
            NonZeroUsize::new(DEFAULT_MESSAGE_MAX_BYTES + CANONICAL_WRITE_ENVELOPE_OVERHEAD_BYTES)
                .expect("non-zero body limit"),
            NonZeroUsize::new(128).expect("non-zero connection limit"),
        ),
        RuntimeTimeouts::new(
            NonZeroDuration::new(Duration::from_secs(3)).expect("non-zero request timeout"),
            NonZeroDuration::new(REPLACEMENT_DRAIN_DEADLINE).expect("non-zero shutdown timeout"),
        ),
    )
    .with_direct_peer_tcp(direct_peer_tcp)
    .with_peer_pool_config(peer_pool_config);
    match peer_stream_adapter {
        Some(adapter) => config.with_peer_stream_adapter(Arc::clone(adapter)),
        None => config,
    }
}

fn record_peer_wire_mode_selection(
    observability: &dyn ObservabilityPort,
    daemon_launch_identity: &DaemonLaunchIdentity,
    peer_wire_mode: PeerWireMode,
    peer_stream_adapter: &Option<Arc<dyn PeerStreamAdapter>>,
) {
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
}

async fn run_replacement_daemon_with_selector(
    observability: Arc<dyn ObservabilityPort + Send + Sync>,
    selector_factory: impl FnOnce(
        atm_core::LocalServiceRuntime,
        Arc<dyn HerdrProcessAdapter>,
        RuntimeHealth,
        BareCliRuntime,
    ) -> Arc<dyn atm_core::boundary::MessageReceivedHookSelector>,
    daemon_launch_identity: DaemonLaunchIdentity,
    peer_wire_mode: PeerWireMode,
    direct_peer_tcp: DirectPeerTcpConfig,
    peer_pool_config: PeerPoolConfig,
    herdr_process: Option<Arc<dyn HerdrProcessAdapter>>,
) -> Result<(), AtmError> {
    install_sqlite_retained_runtime_factory();
    let scope = current_host_runtime_scope()?;
    let _owner = DaemonOwnerGuard::acquire_at(scope.owner_lock.clone())?;
    let runtime_health = RuntimeHealth::with_owner(std::process::id());
    let bare_cli = BareCliRuntime::default();
    let atm_temp_sweeper =
        start_atm_temp_sweeper(Arc::clone(&observability), daemon_launch_identity.clone())?;
    let assembly = assemble_daemon_runtime()?;
    let workflow_telemetry = assembly.workflow_telemetry.clone();
    let peer_stream_adapter = bootstrap_peer_stream_adapter(&assembly, peer_wire_mode)?;
    let (handler, recovery_sweep) = build_replacement_handler(
        assembly,
        ReplacementHandlerConfig {
            observability: Arc::clone(&observability),
            selector_factory,
            daemon_launch_identity: daemon_launch_identity.clone(),
            peer_wire_mode,
            peer_adapter_selection: SelectedPeerAdapterSelection {
                adapter: peer_stream_adapter.clone(),
                pool_config: peer_pool_config,
            },
            runtime_health: runtime_health.clone(),
            bare_cli,
            herdr_process,
        },
    )?;
    let config = replacement_runtime_config(
        &scope,
        &_owner,
        direct_peer_tcp,
        &peer_stream_adapter,
        peer_pool_config,
    )?;
    record_peer_wire_mode_selection(
        observability.as_ref(),
        &daemon_launch_identity,
        peer_wire_mode,
        &peer_stream_adapter,
    );
    let running = start_replacement_runtime(config, handler.clone(), runtime_health).await?;
    run_until_shutdown(
        running,
        handler,
        workflow_telemetry,
        recovery_sweep,
        atm_temp_sweeper,
    )
    .await
}

/// Advertises readiness, then waits for either a shutdown signal or an
/// unexpected server stop, draining every supervised subsystem exactly once
/// on every exit path (ready-signal failure, unexpected stop, or ordinary
/// shutdown).
async fn run_until_shutdown(
    running: atm_http_runtime::HttpRuntime<atm_http_runtime::Running>,
    handler: Arc<StorageAndNudgeRouter>,
    workflow_telemetry: atm_runtime::WorkflowTelemetryRuntime,
    recovery_sweep: queue_drain::RecoverySweepHandle,
    atm_temp_sweeper: AtmTempSweeperRuntime,
) -> Result<(), AtmError> {
    if let Err(error) = emit_ready_signal_if_requested() {
        // The process has not advertised readiness, so it must not retain an
        // otherwise-live listener when its supervisor handshake fails.
        let _ = shutdown_replacement_daemon(
            running,
            handler.as_ref(),
            workflow_telemetry,
            recovery_sweep,
            atm_temp_sweeper,
        )
        .await;
        return Err(error);
    }
    await_runtime_or_shutdown(
        running,
        handler.as_ref(),
        workflow_telemetry,
        recovery_sweep,
        atm_temp_sweeper,
    )
    .await
}

async fn start_replacement_runtime(
    config: HttpRuntimeConfig,
    handler: Arc<StorageAndNudgeRouter>,
    runtime_health: RuntimeHealth,
) -> Result<atm_http_runtime::HttpRuntime<atm_http_runtime::Running>, AtmError> {
    // `StorageAndNudgeRouter` forwards `RuntimeMaintenance::start` to the
    // `HerdrQueueWakePump` composed in `build_replacement_handler`. Without
    // registering the router itself as the runtime's maintenance object
    // here, `HttpRuntime` never spawns that forwarding call and the queue
    // wake pump silently never starts in production.
    let maintenance = Arc::clone(&handler) as Arc<dyn atm_http_runtime::RuntimeMaintenance>;
    let runtime_handler: Arc<dyn atm_http_runtime::CanonicalWriteHandler> = handler;
    HttpRuntimeBuilder::new(config, runtime_handler)
        .with_runtime_health(runtime_health)
        .with_maintenance(maintenance)
        .build()?
        .start()
        .await
}

async fn await_runtime_or_shutdown(
    mut running: atm_http_runtime::HttpRuntime<atm_http_runtime::Running>,
    handler: &StorageAndNudgeRouter,
    workflow_telemetry: atm_runtime::WorkflowTelemetryRuntime,
    recovery_sweep: queue_drain::RecoverySweepHandle,
    atm_temp_sweeper: AtmTempSweeperRuntime,
) -> Result<(), AtmError> {
    tokio::select! {
        signal = wait_for_shutdown_signal() => {
            let signal = signal?;
            eprintln!("replacement ATM daemon received {}; starting graceful shutdown", signal.as_str());
            false
        }
        _ = running.wait_for_server_stop() => {
            eprintln!("replacement ATM HTTP runtime server stopped unexpectedly; beginning cleanup");
            let result = shutdown_replacement_daemon(
                running,
                handler,
                workflow_telemetry,
                recovery_sweep,
                atm_temp_sweeper,
            )
            .await;
            return match result {
                Ok(()) => Err(AtmError::daemon_unavailable(
                    "replacement HTTP runtime server stopped unexpectedly",
                )),
                Err(error) => Err(error),
            };
        }
    };
    shutdown_replacement_daemon(
        running,
        handler,
        workflow_telemetry,
        recovery_sweep,
        atm_temp_sweeper,
    )
    .await
}

/// Resolves `$ATM_TEMP` (defaulting and emitting the one-time fallback
/// warning when unset, per ADR-055 decision (a)) and starts its periodic
/// TTL sweeper. A resolution or sweep-config failure fails daemon boot
/// closed: an operator who explicitly set an invalid `ATM_TEMP`, or a zero
/// sweep interval/TTL, gets an actionable error rather than a silently
/// disabled sweeper.
fn start_atm_temp_sweeper(
    observability: Arc<dyn ObservabilityPort + Send + Sync>,
    daemon_launch_identity: DaemonLaunchIdentity,
) -> Result<AtmTempSweeperRuntime, AtmError> {
    let env = ProcessEnvSource;
    let atm_temp =
        resolve_atm_temp(&env).map_err(|error| AtmError::config(format!("ATM_TEMP: {error}")))?;
    if is_atm_temp_unset(&env) {
        tracing::warn!(
            default_path = %atm_temp.path().display(),
            override_env = "ATM_TEMP",
            "ATM_TEMP is unset; using the default scratch root (set ATM_TEMP to override)"
        );
    }
    let config = daemon_atm_config(&env).unwrap_or_default();
    let sweep_config = validate_sweep_config(config.sweep_interval_seconds, config.sweep_ttl_days)
        .map_err(|error| AtmError::config(format!("$ATM_TEMP sweep config: {error}")))?;
    Ok(AtmTempSweeperRuntime::start(
        atm_temp.path().to_path_buf(),
        sweep_config,
        observability,
        daemon_launch_identity,
    ))
}

fn bootstrap_peer_stream_adapter(
    assembly: &RuntimeAssembly,
    peer_wire_mode: PeerWireMode,
) -> Result<Option<Arc<dyn PeerStreamAdapter>>, AtmError> {
    peer_stream_adapter_for_mode(peer_wire_mode, || {
        Ok(Arc::new(BootstrapMtlsStreamAdapter {
            adapter: Arc::new(MtlsPeerStreamAdapter::from_peer_config(
                assembly.peer_config_store.as_ref(),
            )?),
        }) as Arc<dyn PeerStreamAdapter>)
    })
}

/// Drain the Axum runtime before releasing outbound peer drivers and the
/// best-effort workflow telemetry worker. Every terminal daemon path uses the
/// same sequence so a failed ready handshake cannot leave either subsystem
/// alive after the listener is gone.
async fn shutdown_replacement_daemon(
    running: atm_http_runtime::HttpRuntime<atm_http_runtime::Running>,
    handler: &StorageAndNudgeRouter,
    workflow_telemetry: atm_runtime::WorkflowTelemetryRuntime,
    recovery_sweep: queue_drain::RecoverySweepHandle,
    atm_temp_sweeper: AtmTempSweeperRuntime,
) -> Result<(), AtmError> {
    let stopped = running.begin_shutdown().finish().await;
    recovery_sweep.shutdown(REPLACEMENT_DRAIN_DEADLINE).await;
    handler
        .shutdown_peer_connections(REPLACEMENT_DRAIN_DEADLINE)
        .await;
    workflow_telemetry.shutdown().await;
    atm_temp_sweeper.shutdown().await;
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
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::num::NonZeroU16;
    use std::sync::Arc;
    use std::time::Duration;

    use atm_core::api::ApiRequest;
    use atm_core::api::RequestDeadline;
    use atm_core::boundary::{
        BuiltInPostSendDispatch, MessageReceivedHookSelector, RosterEntry, TemplateSource,
    };
    use atm_core::doctor::{DoctorSeverity, HerdrPresenceDoctor};
    use atm_core::observability::NullObservability;
    use atm_core::peer_wire::PeerWireMode;
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
    use atm_core::send::{SendMessageSource, WriteRequest};
    use atm_core::types::{AgentName, ModelName, TeamName};
    use atm_http_runtime::{
        DirectPeerTcpConfig, HttpRuntimeBuilder, LoopbackTcpConfig, PeerPoolConfig, RuntimeHealth,
        direct_peer_tcp_client,
    };
    use atm_runtime_test_support::open_isolated_sqlite_boundary;
    use atm_storage::{MessageKey, RosterHarness, RosterMemberKind, RosterSnapshot};
    use atm_template_sc_compose::ScComposeTemplateComposer;
    use serde_json::Map;

    use super::peer_launch_config::parse_peer_pool_config_with_environment;
    use super::replacement_handler::{HerdrPresenceDoctorAdapter, herdr_presence_finding};
    use super::{
        DaemonLaunchIdentity, REPLACEMENT_DRAIN_DEADLINE, ReplacementHandlerConfig,
        SelectedPeerAdapterSelection, ShutdownSignal, assemble_host_runtime_with_template_composer,
        build_replacement_handler, parse_direct_peer_port, parse_peer_wire_mode,
        peer_stream_adapter_for_mode, replacement_runtime_config_with_direct_peer,
        start_replacement_runtime, write_ready_signal_if_requested,
    };

    /// Test-owned receiver selection prevents an external tmux/graft action
    /// from obscuring the bootstrap's direct-peer persistence proof.
    struct NoReceivedHookSelector;

    impl atm_core::boundary::sealed::Sealed for NoReceivedHookSelector {}

    impl MessageReceivedHookSelector for NoReceivedHookSelector {
        fn select_emitter(
            &self,
            _dispatch: &BuiltInPostSendDispatch,
        ) -> Option<&dyn atm_core::boundary::AsyncMessageReceivedHookEmitter> {
            None
        }
    }

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
    fn direct_peer_port_defaults_and_accepts_one_explicit_nonzero_value() {
        assert_eq!(
            parse_direct_peer_port([OsString::from("atm-daemon")]).expect("standard port"),
            NonZeroU16::new(atm_http_runtime::DIRECT_PEER_TCP_PORT).expect("non-zero"),
        );
        assert_eq!(
            parse_direct_peer_port([
                OsString::from("atm-daemon"),
                OsString::from("--direct-peer-port=43102"),
            ])
            .expect("explicit benchmark port"),
            NonZeroU16::new(43102).expect("non-zero"),
        );
    }

    #[test]
    fn direct_peer_port_rejects_zero_and_duplicates() {
        let zero = parse_direct_peer_port([
            OsString::from("atm-daemon"),
            OsString::from("--direct-peer-port"),
            OsString::from("0"),
        ])
        .expect_err("zero cannot bind a durable launch port");
        assert!(zero.message().contains("non-zero"));

        let duplicate = parse_direct_peer_port([
            OsString::from("atm-daemon"),
            OsString::from("--direct-peer-port=43102"),
            OsString::from("--direct-peer-port=43103"),
        ])
        .expect_err("one daemon has one direct-peer listener");
        assert!(duplicate.message().contains("only once"));
    }

    #[test]
    fn peer_pool_config_defaults_and_launch_flags_are_validated() {
        let empty = HashMap::<String, OsString>::new();
        let defaults =
            parse_peer_pool_config_with_environment([OsString::from("atm-daemon")], |name| {
                empty.get(name).cloned()
            })
            .expect("pool defaults are valid");
        assert_eq!(defaults, PeerPoolConfig::default());

        let explicit = parse_peer_pool_config_with_environment(
            [
                OsString::from("atm-daemon"),
                OsString::from("--peer-pool-max-per-peer=7"),
                OsString::from("--peer-pool-max-pooled-total"),
                OsString::from("19"),
                OsString::from("--peer-pool-idle-timeout-ms=250"),
            ],
            |name| empty.get(name).cloned(),
        )
        .expect("positive explicit pool values are valid");
        assert_eq!(explicit.max_per_peer, 7);
        assert_eq!(explicit.max_pooled_total, 19);
        assert_eq!(explicit.idle_timeout, Duration::from_millis(250));

        for argument in [
            "--peer-pool-max-per-peer=0",
            "--peer-pool-max-pooled-total=-1",
            "--peer-pool-idle-timeout-ms=garbage",
        ] {
            assert!(
                parse_peer_pool_config_with_environment(
                    [OsString::from("atm-daemon"), OsString::from(argument)],
                    |name| empty.get(name).cloned(),
                )
                .is_err()
            );
        }
    }

    #[test]
    fn peer_pool_launch_flags_override_environment_and_duplicates_fail() {
        let environment = HashMap::from([
            (
                String::from("ATM_PEER_POOL_MAX_PER_PEER"),
                OsString::from("3"),
            ),
            (
                String::from("ATM_PEER_POOL_MAX_POOLED_TOTAL"),
                OsString::from("11"),
            ),
            (
                String::from("ATM_PEER_POOL_IDLE_TIMEOUT_MS"),
                OsString::from("700"),
            ),
        ]);
        let overridden = parse_peer_pool_config_with_environment(
            [
                OsString::from("atm-daemon"),
                OsString::from("--peer-pool-max-per-peer=5"),
            ],
            |name| environment.get(name).cloned(),
        )
        .expect("launch flag overrides environment");
        assert_eq!(overridden.max_per_peer, 5);
        assert_eq!(overridden.max_pooled_total, 11);
        assert_eq!(overridden.idle_timeout, Duration::from_millis(700));

        let duplicate = parse_peer_pool_config_with_environment(
            [
                OsString::from("atm-daemon"),
                OsString::from("--peer-pool-max-per-peer=2"),
                OsString::from("--peer-pool-max-per-peer=3"),
            ],
            |name| environment.get(name).cloned(),
        )
        .expect_err("one launch value must control each pool setting");
        assert!(duplicate.message().contains("only once"));
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

    #[tokio::test]
    async fn plaintext_test_bootstrap_runs_direct_peer_write_without_tls_configuration() {
        let temporary_root = tempfile::tempdir().expect("temporary bootstrap runtime root");
        let assembly = open_isolated_sqlite_boundary(temporary_root.path())
            .expect("assemble isolated daemon runtime")
            .for_daemon();
        let team: TeamName = "test-team".parse().expect("team");
        assembly
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: ["recipient", "sender"]
                    .into_iter()
                    .map(|agent_name| RosterEntry {
                        team_name: team.clone(),
                        agent_name: agent_name.parse().expect("agent"),
                        member_kind: RosterMemberKind::Permanent,
                        harness: RosterHarness::PythonGraft,
                        agent_type: atm_core::schema::AgentType::default(),
                        model: ModelName::default(),
                        recipient_pane_id: None,
                        metadata_json: Map::new(),
                    })
                    .collect(),
                refreshed_at: None,
            })
            .expect("seed direct-peer recipient roster");
        let message_store = assembly.message_store_arc();
        let peer_stream_adapter =
            peer_stream_adapter_for_mode(PeerWireMode::plaintext_test(), || {
                panic!("plaintext bootstrap must not inspect invalid TLS peer configuration")
            })
            .expect("plaintext bootstrap selects no TLS stream adapter");
        let runtime_health = RuntimeHealth::with_owner(std::process::id());
        let (handler, _recovery_sweep) = build_replacement_handler(
            assembly,
            ReplacementHandlerConfig {
                observability: Arc::new(NullObservability),
                selector_factory: |_, _, _, _| {
                    Arc::new(NoReceivedHookSelector)
                        as Arc<dyn atm_core::boundary::MessageReceivedHookSelector>
                },
                daemon_launch_identity: DaemonLaunchIdentity::default(),
                peer_wire_mode: PeerWireMode::plaintext_test(),
                peer_adapter_selection: SelectedPeerAdapterSelection {
                    adapter: peer_stream_adapter.clone(),
                    pool_config: PeerPoolConfig::default(),
                },
                runtime_health: runtime_health.clone(),
                bare_cli: Default::default(),
                herdr_process: None,
            },
        )
        .expect("compose the replacement daemon handler");
        let config = replacement_runtime_config_with_direct_peer(
            LoopbackTcpConfig::new(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                temporary_root.path().join("local-http.json"),
                ulid::Ulid::new(),
            ),
            None,
            DirectPeerTcpConfig::ephemeral_for_test(),
            &peer_stream_adapter,
            PeerPoolConfig::default(),
        );
        let running = HttpRuntimeBuilder::new(config, handler)
            .with_runtime_health(runtime_health)
            .build()
            .expect("validate plaintext bootstrap runtime")
            .start()
            .await
            .expect("start plaintext direct-peer listener");
        let peer_port = running
            .direct_peer_address()
            .expect("ephemeral direct peer listener bound")
            .port();
        let client = direct_peer_tcp_client(
            "127.0.0.1".parse().expect("direct peer host"),
            NonZeroU16::new(peer_port).expect("non-zero direct peer port"),
            Duration::from_secs(3),
        )
        .expect("direct peer client");
        let request = WriteRequest::new(
            temporary_root.path().join("home"),
            temporary_root.path().join("workspace"),
            "sender".parse::<AgentName>().expect("sender"),
            "recipient@test-team",
            team,
            SendMessageSource::Inline("bootstrap plaintext direct-peer proof".to_owned()),
            None,
            false,
            None,
            false,
        )
        .expect("direct peer write request");
        let response = client
            .execute(ApiRequest::new(RequestEnvelope::Write(Box::new(request))))
            .await
            .expect("plaintext direct peer write response")
            .into_inner();
        let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response else {
            panic!("plaintext direct peer write must return the canonical send response");
        };
        assert!(
            message_store
                .load_message(&MessageKey::from(outcome.message_id))
                .expect("inspect durable direct peer message")
                .is_some(),
            "plaintext mode reaches the same durable storage pipeline without TLS configuration"
        );
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("plaintext bootstrap runtime drains");
    }

    /// Phase-end CRITICAL regression: `HerdrQueueWakePump` was composed into
    /// `StorageAndNudgeRouter::with_maintenance` (AQ2.7) but the production
    /// `start_replacement_runtime` entry point never registered the router
    /// itself as the `HttpRuntime`'s maintenance object, so the pump was
    /// never spawned outside of tests that call `pump.tick_once()` directly.
    ///
    /// This test goes through the exact production entry point
    /// (`start_replacement_runtime`, not a hand-assembled
    /// `HttpRuntimeBuilder`) and asserts the pump actually ran by awaiting
    /// `RuntimeHealth::subscribe_herdr_queue_tick()`'s own `changed()`
    /// signal -- the pump's real completion event, fired from
    /// `record_herdr_queue_tick` every time `tick_once` runs -- instead of
    /// polling `snapshot()` on a fixed sleep cadence. The subscription is
    /// created before the runtime starts so the pump's first (immediate)
    /// tick cannot race ahead of it.
    #[tokio::test]
    async fn replacement_runtime_start_actually_runs_the_herdr_queue_wake_pump() {
        let temporary_root = tempfile::tempdir().expect("temporary bootstrap runtime root");
        let assembly = open_isolated_sqlite_boundary(temporary_root.path())
            .expect("assemble isolated daemon runtime")
            .for_daemon();
        let peer_stream_adapter =
            peer_stream_adapter_for_mode(PeerWireMode::plaintext_test(), || {
                panic!("plaintext bootstrap must not inspect invalid TLS peer configuration")
            })
            .expect("plaintext bootstrap selects no TLS stream adapter");
        let runtime_health = RuntimeHealth::with_owner(std::process::id());
        let mut herdr_queue_tick = runtime_health.subscribe_herdr_queue_tick();
        let (handler, _recovery_sweep) = build_replacement_handler(
            assembly,
            ReplacementHandlerConfig {
                observability: Arc::new(NullObservability),
                selector_factory: |_, _, _, _| {
                    Arc::new(NoReceivedHookSelector)
                        as Arc<dyn atm_core::boundary::MessageReceivedHookSelector>
                },
                daemon_launch_identity: DaemonLaunchIdentity::default(),
                peer_wire_mode: PeerWireMode::plaintext_test(),
                peer_adapter_selection: SelectedPeerAdapterSelection {
                    adapter: peer_stream_adapter.clone(),
                    pool_config: PeerPoolConfig::default(),
                },
                runtime_health: runtime_health.clone(),
                bare_cli: Default::default(),
                herdr_process: None,
            },
        )
        .expect("compose the replacement daemon handler");
        let config = replacement_runtime_config_with_direct_peer(
            LoopbackTcpConfig::new(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                temporary_root.path().join("local-http.json"),
                ulid::Ulid::new(),
            ),
            None,
            DirectPeerTcpConfig::ephemeral_for_test(),
            &peer_stream_adapter,
            PeerPoolConfig::default(),
        );

        let running = start_replacement_runtime(config, handler, runtime_health.clone())
            .await
            .expect("start the real replacement runtime entry point");

        let observed_tick = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if herdr_queue_tick.borrow_and_update().is_some() {
                    return;
                }
                herdr_queue_tick
                    .changed()
                    .await
                    .expect("the RuntimeHealth handle stays alive for the test's duration");
            }
        })
        .await;

        running
            .begin_shutdown()
            .finish()
            .await
            .expect("replacement runtime drains");

        observed_tick.expect(
            "HerdrQueueWakePump must actually run through the production \
             `start_replacement_runtime` path so its own `RuntimeHealth::record_herdr_queue_tick` \
             completion signal fires, without any fixed wall-clock sleep",
        );
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

    #[tokio::test]
    async fn doctor_presence_probe_uses_bypass_and_degrades_outages() {
        let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        fake.queue_get_result(Err(atm_herdr::HerdrError::AgentNotFound));
        let roster = atm_core::team_admin::MembersList {
            team: "team".parse().expect("team"),
            members: vec![atm_core::team_admin::MemberSummary {
                name: "receiver".parse().expect("agent"),
                agent_id: "receiver".to_owned(),
                agent_type: "worker".to_owned(),
                harness: atm_core::boundary::RosterHarness::CodexCli,
                model: ModelName::new("gpt-5").expect("model"),
                joined_at: None,
                tmux_pane_id: None,
                backend: Some("herdr".to_owned()),
                herdr_session: Some("team-a".to_owned()),
                local_backend: Some(atm_core::LocalMessageReceivedBackend::Herdr {
                    session: Some(atm_core::HerdrSession::new("team-a").expect("session")),
                }),
                home_dir: std::path::PathBuf::from("/tmp").into(),
                live_cwd: None,
                host: None,
                extra: serde_json::Map::new(),
            }],
        };
        let findings = HerdrPresenceDoctorAdapter {
            process: fake.clone(),
        }
        .probe(&roster, RequestDeadline::after(Duration::from_secs(2)))
        .await;
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].code,
            atm_core::error_codes::AtmErrorCode::HerdrAgentNotVisible
        );
        assert!(matches!(
            fake.calls().as_slice(),
            [atm_herdr::testing::FakeHerdrCall::Get {
                breaker_policy: atm_herdr::BreakerPolicy::Bypass,
                ..
            }]
        ));

        let outage_fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        outage_fake.queue_get_result(Err(atm_herdr::HerdrError::ServerUnavailable));
        let outage_findings = HerdrPresenceDoctorAdapter {
            process: outage_fake,
        }
        .probe(&roster, RequestDeadline::after(Duration::from_secs(2)))
        .await;
        assert_eq!(outage_findings.len(), 1);
        assert_eq!(outage_findings[0].severity, DoctorSeverity::Info);
        assert!(
            outage_findings[0]
                .message
                .starts_with("Herdr presence probe skipped:")
        );
    }

    #[test]
    fn doctor_and_emitter_share_herdr_outcome_classification() {
        let errors = [
            atm_herdr::HerdrError::AgentBlocked,
            atm_herdr::HerdrError::AgentNotFound,
            atm_herdr::HerdrError::AgentNotReady,
            atm_herdr::HerdrError::AgentTargetAmbiguous,
            atm_herdr::HerdrError::AgentNotRunning,
            atm_herdr::HerdrError::AgentPromptStalled,
            atm_herdr::HerdrError::ServerNotRunning,
            atm_herdr::HerdrError::ProtocolMismatch,
            atm_herdr::HerdrError::Timeout,
            atm_herdr::HerdrError::InvalidAgentName,
            atm_herdr::HerdrError::EmptyAgentPrompt,
            atm_herdr::HerdrError::ServerUnavailable,
            atm_herdr::HerdrError::InternalError,
            atm_herdr::HerdrError::TimedOut,
            atm_herdr::HerdrError::Unavailable {
                retry_after: Duration::from_secs(1),
            },
            atm_herdr::HerdrError::Advisory {
                code: "future_code".to_owned(),
            },
        ];
        for error in errors {
            let outcome = error.emission_outcome();
            let finding = herdr_presence_finding(error.clone());
            if matches!(error, atm_herdr::HerdrError::AgentNotFound) {
                assert_eq!(
                    finding.code,
                    atm_core::error_codes::AtmErrorCode::HerdrAgentNotVisible
                );
            } else {
                assert!(finding.message.contains(outcome), "{outcome}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn root_uses_authenticated_loopback_without_making_uds_startup_mandatory() {
        assert!(super::unix_socket_config_for_uid(std::path::Path::new("/tmp"), 0).is_none());
    }
}
