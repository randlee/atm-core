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
use atm_core::observability::{NullObservability, ObservabilityPort};
use atm_core::send::input::DEFAULT_MESSAGE_MAX_BYTES;
use atm_core::types::{AgentName, TeamName};
#[cfg(feature = "benchmark-harness")]
use atm_http_runtime::DirectPeerPlaintextDiagnostic;
use atm_http_runtime::{
    DirectPeerTcpConfig, HttpRuntimeBuilder, HttpRuntimeConfig, LoopbackTcpConfig, NonZeroDuration,
    RuntimeHealth, RuntimeLimits, RuntimeTimeouts, StorageAndNudgeRouter,
};
use atm_runtime::{RuntimeAssembly, RuntimeAssemblyInputs, assemble_runtime};
#[cfg(feature = "benchmark-harness")]
use atm_storage::{
    CertificateFingerprint, HostName, HttpsInterface, LocalCertificate, PrivateKeyRef, TrustedPeer,
    certificate_fingerprint,
};
use atm_storage_rusqlite::SqliteStorageFactory;

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
#[cfg(feature = "benchmark-harness")]
const BENCHMARK_PEER_HOST: &str = "localhost";
#[cfg(feature = "benchmark-harness")]
const BENCHMARK_PEER_PORT: u16 = 43_101;
#[cfg(feature = "benchmark-harness")]
const BENCHMARK_PEER_IDENTITY_FILE: &str = "benchmark-peer-tls.pem";

/// The explicit wire-security selection accepted only by the feature-gated
/// benchmark child.  Normal daemon startup remains mTLS-only.
#[cfg(feature = "benchmark-harness")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchmarkPeerWireSecurity {
    PlaintextTest,
    MutualTls,
}

#[cfg(feature = "benchmark-harness")]
impl BenchmarkPeerWireSecurity {
    /// Parse the benchmark child's required launch selection.
    pub fn parse(value: &str) -> Result<Self, AtmError> {
        match value {
            "plaintext-test" => Ok(Self::PlaintextTest),
            "mtls" => Ok(Self::MutualTls),
            _ => Err(AtmError::config(
                "--peer-wire-security must be `plaintext-test` or `mtls`",
            )),
        }
    }
}

/// Whether the benchmark child should bind the direct-peer listener for its
/// selected wire-security mode. UDS capacity profiles keep their established
/// local-only behavior while still passing an explicit launch selection.
#[cfg(feature = "benchmark-harness")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchmarkDirectPeerListener {
    Disabled,
    Enabled,
}

#[cfg(feature = "benchmark-harness")]
impl BenchmarkDirectPeerListener {
    /// Parse the benchmark child's explicit direct-peer listener selection.
    pub fn parse(value: &str) -> Result<Self, AtmError> {
        match value {
            "disabled" => Ok(Self::Disabled),
            "enabled" => Ok(Self::Enabled),
            _ => Err(AtmError::config(
                "--direct-peer-listener must be `disabled` or `enabled`",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectPeerWireSecurity {
    MutualTls,
    #[cfg(feature = "benchmark-harness")]
    PlaintextBenchmark,
}

#[cfg(feature = "benchmark-harness")]
impl From<BenchmarkPeerWireSecurity> for DirectPeerWireSecurity {
    fn from(value: BenchmarkPeerWireSecurity) -> Self {
        match value {
            BenchmarkPeerWireSecurity::PlaintextTest => Self::PlaintextBenchmark,
            BenchmarkPeerWireSecurity::MutualTls => Self::MutualTls,
        }
    }
}

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
    run_replacement_daemon_with_selector(
        observability,
        active_received_hook_selector,
        resolve_daemon_launch_identity(),
        DirectPeerWireSecurity::MutualTls,
        true,
    )
    .await
}

/// Starts the separately compiled benchmark daemon with an explicit hook mode.
///
/// This symbol is unavailable to the shipped `atm-daemon` binary because it
/// exists only behind the `benchmark-harness` feature.
#[cfg(feature = "benchmark-harness")]
pub async fn run_benchmark_daemon(
    hook_mode: BenchmarkHookMode,
    peer_wire_security: BenchmarkPeerWireSecurity,
    direct_peer_listener: BenchmarkDirectPeerListener,
) -> Result<(), AtmError> {
    run_replacement_daemon_with_selector(
        Arc::new(NullObservability),
        move |service_runtime| benchmark_received_hook_selector(service_runtime, hook_mode),
        resolve_daemon_launch_identity(),
        peer_wire_security.into(),
        direct_peer_listener == BenchmarkDirectPeerListener::Enabled,
    )
    .await
}

fn build_replacement_handler(
    assembly: RuntimeAssembly,
    observability: Arc<dyn ObservabilityPort + Send + Sync>,
    peer_io_adapter: Option<Arc<dyn atm_core::PeerIoAdapter>>,
    selector_factory: impl FnOnce(
        atm_core::LocalServiceRuntime,
    ) -> Arc<dyn atm_core::boundary::MessageReceivedHookSelector>,
    daemon_launch_identity: &DaemonLaunchIdentity,
    runtime_health: RuntimeHealth,
) -> Result<Arc<StorageAndNudgeRouter>, AtmError> {
    let selector = selector_factory(assembly.service_runtime.clone());
    let router = StorageAndNudgeRouter::new(
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
    });
    let router = match peer_io_adapter {
        Some(peer_io_adapter) => router.with_peer_io_adapter(peer_io_adapter),
        None => router,
    };
    Ok(Arc::new(router))
}

async fn run_replacement_daemon_with_selector(
    observability: Arc<dyn ObservabilityPort + Send + Sync>,
    selector_factory: impl FnOnce(
        atm_core::LocalServiceRuntime,
    ) -> Arc<dyn atm_core::boundary::MessageReceivedHookSelector>,
    daemon_launch_identity: DaemonLaunchIdentity,
    peer_wire_security: DirectPeerWireSecurity,
    direct_peer_requested: bool,
) -> Result<(), AtmError> {
    install_sqlite_retained_runtime_factory();
    let scope = current_host_runtime_scope()?;
    let _owner = acquire_daemon_owner(scope.owner_lock.clone()).await?;
    let runtime_health = RuntimeHealth::with_owner(std::process::id());
    let assembly = assemble_daemon_runtime()?;
    #[cfg(feature = "benchmark-harness")]
    if direct_peer_requested && peer_wire_security == DirectPeerWireSecurity::MutualTls {
        let identity_root = atm_core::home::atm_home()?;
        let store = assembly.peer_config_store();
        configure_disposable_benchmark_peer_tls(store.as_ref(), &identity_root)?;
    }
    let workflow_telemetry = assembly.workflow_telemetry.clone();
    let peer_io_adapter =
        optional_peer_io_adapter(&assembly, peer_wire_security, direct_peer_requested)?;
    let mut running = start_replacement_runtime(ReplacementStartupInputs {
        scope: &scope,
        owner: &_owner,
        assembly,
        observability,
        peer_io_adapter,
        selector_factory,
        daemon_launch_identity: &daemon_launch_identity,
        runtime_health,
        peer_wire_security,
        direct_peer_requested,
    })
    .await?;
    if let Err(error) = emit_ready_signal_if_requested() {
        // The process has not advertised readiness, so it must not retain an
        // otherwise-live listener when its supervisor handshake fails.
        let _ = running.begin_shutdown().finish().await;
        workflow_telemetry.shutdown().await;
        return Err(error);
    }
    if let ShutdownTrigger::ServerStopped = wait_for_replacement_shutdown(&mut running).await? {
        let result = match running.begin_shutdown().finish().await {
            Ok(_) => Err(AtmError::daemon_unavailable(
                "replacement HTTP runtime server stopped unexpectedly",
            )),
            Err(error) => Err(error),
        };
        workflow_telemetry.shutdown().await;
        return result;
    }
    let _stopped = running.begin_shutdown().finish().await?;
    workflow_telemetry.shutdown().await;
    Ok(())
}

/// Install the benchmark process's disposable, localhost-only mTLS identity.
///
/// The capacity runner supplies a fresh `ATM_HOME` and restores the complete
/// OS-user durable state after each run.  The certificate bundle therefore
/// stays outside durable SQLite state, while the ordinary durable peer-config
/// boundary holds only its fingerprint and path reference.  This intentionally
/// uses the same peer-tls adapter configuration the normal daemon consumes;
/// it is not an alternate HTTP or Rustls path.
#[cfg(feature = "benchmark-harness")]
fn configure_disposable_benchmark_peer_tls(
    store: &(dyn atm_storage::PeerConfigStore + Send + Sync),
    identity_root: &std::path::Path,
) -> Result<(), AtmError> {
    let generated = rcgen::generate_simple_self_signed(vec![BENCHMARK_PEER_HOST.to_owned()])
        .map_err(|source| {
            AtmError::certificate_operation(
                "could not generate disposable benchmark peer TLS certificate",
            )
            .with_cause(source)
        })?;
    let identity_path = identity_root.join(BENCHMARK_PEER_IDENTITY_FILE);
    write_disposable_benchmark_identity(
        &identity_path,
        &format!(
            "{}{}",
            generated.cert.pem(),
            generated.key_pair.serialize_pem()
        ),
    )?;
    let fingerprint = certificate_fingerprint(generated.cert.der());
    let fingerprint: CertificateFingerprint = fingerprint.parse().map_err(|source| {
        AtmError::certificate_operation(
            "generated benchmark peer TLS certificate has an invalid fingerprint",
        )
        .with_cause(source)
    })?;
    let host: HostName = BENCHMARK_PEER_HOST.parse().map_err(|source| {
        AtmError::certificate_operation("benchmark peer hostname is invalid").with_cause(source)
    })?;
    store.save_interface(&HttpsInterface {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), BENCHMARK_PEER_PORT),
        advertise_host: host.clone(),
        enabled: true,
    })?;
    store.save_local_certificate(&LocalCertificate {
        fingerprint: fingerprint.clone(),
        private_key_ref: identity_path
            .display()
            .to_string()
            .parse::<PrivateKeyRef>()
            .map_err(|source| {
                AtmError::certificate_operation(
                    "benchmark peer TLS identity path is not a valid private-key reference",
                )
                .with_cause(source)
            })?,
    })?;
    store.save_trusted_peer(&TrustedPeer {
        host,
        fingerprint,
        enabled: true,
        https_port: std::num::NonZeroU16::new(BENCHMARK_PEER_PORT)
            .expect("benchmark peer TLS port is non-zero"),
    })?;
    Ok(())
}

/// Write the short-lived certificate bundle without widening normal daemon
/// secret handling. Unix stores receive owner-only permissions; Windows relies
/// on its per-user temporary directory ACLs used by the benchmark harness.
#[cfg(feature = "benchmark-harness")]
fn write_disposable_benchmark_identity(path: &std::path::Path, pem: &str) -> Result<(), AtmError> {
    std::fs::create_dir_all(path.parent().ok_or_else(|| {
        AtmError::certificate_operation("benchmark peer TLS identity has no parent directory")
    })?)
    .map_err(|source| {
        AtmError::certificate_operation("could not create benchmark peer TLS identity directory")
            .with_cause(source)
    })?;
    write_disposable_benchmark_identity_contents(path, pem)
}

#[cfg(all(feature = "benchmark-harness", unix))]
fn write_disposable_benchmark_identity_contents(
    path: &std::path::Path,
    pem: &str,
) -> Result<(), AtmError> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|source| {
            AtmError::certificate_operation("could not create benchmark peer TLS identity bundle")
                .with_cause(source)
        })?;
    file.write_all(pem.as_bytes()).map_err(|source| {
        AtmError::certificate_operation("could not write benchmark peer TLS identity bundle")
            .with_cause(source)
    })
}

#[cfg(all(feature = "benchmark-harness", not(unix)))]
fn write_disposable_benchmark_identity_contents(
    path: &std::path::Path,
    pem: &str,
) -> Result<(), AtmError> {
    std::fs::write(path, pem).map_err(|source| {
        AtmError::certificate_operation("could not write benchmark peer TLS identity bundle")
            .with_cause(source)
    })
}

/// AO.2 composes concrete TLS only at the daemon bootstrap boundary. The
/// runtime receives this sealed, opaque core adapter and never observes a
/// certificate, peer store, or Rustls type.
fn optional_peer_io_adapter(
    assembly: &RuntimeAssembly,
    peer_wire_security: DirectPeerWireSecurity,
    direct_peer_requested: bool,
) -> Result<Option<Arc<dyn atm_core::PeerIoAdapter>>, AtmError> {
    #[cfg(not(feature = "benchmark-harness"))]
    let _ = peer_wire_security;
    if !direct_peer_requested {
        return Ok(None);
    }
    #[cfg(feature = "benchmark-harness")]
    if peer_wire_security == DirectPeerWireSecurity::PlaintextBenchmark {
        return Ok(None);
    }
    #[cfg(feature = "benchmark-harness")]
    if peer_wire_security == DirectPeerWireSecurity::MutualTls {
        // Benchmark mTLS is explicit: a generated identity/configuration
        // failure terminates the run rather than disabling the listener or
        // selecting the named plaintext diagnostic.
        return peer_tls::mtls_adapter(assembly.peer_config_store()).map(Some);
    }
    match peer_tls::mtls_adapter(assembly.peer_config_store()) {
        Ok(adapter) => Ok(Some(adapter)),
        Err(error) => {
            // mTLS is optional until a valid exchange configuration exists.
            // Disable the direct-peer listener rather than falling back to
            // plaintext; local transports continue serving normally.
            tracing::info!(error = %error, "direct peer mTLS is unavailable; listener remains disabled");
            Ok(None)
        }
    }
}

struct ReplacementStartupInputs<'a, F>
where
    F: FnOnce(
        atm_core::LocalServiceRuntime,
    ) -> Arc<dyn atm_core::boundary::MessageReceivedHookSelector>,
{
    scope: &'a atm_core::home::HostRuntimeScope,
    owner: &'a DaemonOwnerGuard,
    assembly: RuntimeAssembly,
    observability: Arc<dyn ObservabilityPort + Send + Sync>,
    peer_io_adapter: Option<Arc<dyn atm_core::PeerIoAdapter>>,
    selector_factory: F,
    daemon_launch_identity: &'a DaemonLaunchIdentity,
    runtime_health: RuntimeHealth,
    peer_wire_security: DirectPeerWireSecurity,
    direct_peer_requested: bool,
}

async fn start_replacement_runtime<F>(
    ReplacementStartupInputs {
        scope,
        owner,
        assembly,
        observability,
        peer_io_adapter,
        selector_factory,
        daemon_launch_identity,
        runtime_health,
        peer_wire_security,
        direct_peer_requested,
    }: ReplacementStartupInputs<'_, F>,
) -> Result<atm_http_runtime::HttpRuntime<atm_http_runtime::Running>, AtmError>
where
    F: FnOnce(
        atm_core::LocalServiceRuntime,
    ) -> Arc<dyn atm_core::boundary::MessageReceivedHookSelector>,
{
    // The shipped daemon always keeps the injected receiver hook active.
    // Benchmark-only selection is available only from the separate binary.
    let handler = build_replacement_handler(
        assembly,
        observability,
        peer_io_adapter.clone(),
        selector_factory,
        daemon_launch_identity,
        runtime_health.clone(),
    )?;
    let direct_peer_enabled = direct_peer_requested
        && (peer_io_adapter.is_some() || {
            #[cfg(feature = "benchmark-harness")]
            {
                peer_wire_security == DirectPeerWireSecurity::PlaintextBenchmark
            }
            #[cfg(not(feature = "benchmark-harness"))]
            {
                false
            }
        });
    let config = replacement_runtime_config(scope, owner, direct_peer_enabled, peer_wire_security)?;
    let builder = HttpRuntimeBuilder::new(config, handler).with_runtime_health(runtime_health);
    let builder = match peer_io_adapter {
        Some(peer_io_adapter) => builder.with_peer_io_adapter(peer_io_adapter),
        None => builder,
    };
    builder.build()?.start().await
}

fn replacement_runtime_config(
    scope: &atm_core::home::HostRuntimeScope,
    owner: &DaemonOwnerGuard,
    direct_peer_enabled: bool,
    peer_wire_security: DirectPeerWireSecurity,
) -> Result<HttpRuntimeConfig, AtmError> {
    #[cfg(not(feature = "benchmark-harness"))]
    let _ = peer_wire_security;
    let loopback = LoopbackTcpConfig::new(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        scope.runtime_root.as_ref().join(LOCAL_HTTP_RECORD_FILENAME),
        owner.instance_id(),
    );
    let config = HttpRuntimeConfig::new(
        loopback,
        unix_socket_config(scope)?,
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
    Ok(if direct_peer_enabled {
        let config = config.with_direct_peer_tcp(DirectPeerTcpConfig::standard());
        #[cfg(feature = "benchmark-harness")]
        let config = if peer_wire_security == DirectPeerWireSecurity::PlaintextBenchmark {
            config.with_plaintext_direct_peer_diagnostic(DirectPeerPlaintextDiagnostic::Benchmark)
        } else {
            config
        };
        config
    } else {
        config
    })
}

enum ShutdownTrigger {
    Signal,
    ServerStopped,
}

async fn wait_for_replacement_shutdown(
    running: &mut atm_http_runtime::HttpRuntime<atm_http_runtime::Running>,
) -> Result<ShutdownTrigger, AtmError> {
    tokio::select! {
        signal = wait_for_shutdown_signal() => {
            let signal = signal?;
            eprintln!("replacement ATM daemon received {}; starting graceful shutdown", signal.as_str());
            Ok(ShutdownTrigger::Signal)
        }
        _ = running.wait_for_server_stop() => {
            eprintln!("replacement ATM HTTP runtime server stopped unexpectedly; beginning cleanup");
            Ok(ShutdownTrigger::ServerStopped)
        }
    }
}

async fn acquire_daemon_owner(lock_path: PathBuf) -> Result<DaemonOwnerGuard, AtmError> {
    tokio::task::spawn_blocking(move || DaemonOwnerGuard::acquire_at(lock_path))
        .await
        .map_err(|source| {
            AtmError::daemon_unavailable("daemon owner-lock worker failed").with_cause(source)
        })?
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
    use std::time::Duration;

    use atm_core::boundary::TemplateSource;
    use atm_template_sc_compose::ScComposeTemplateComposer;
    use serde_json::Map;

    use super::{
        REPLACEMENT_DRAIN_DEADLINE, ShutdownSignal, assemble_host_runtime_with_template_composer,
        write_ready_signal_if_requested,
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

#[cfg(all(test, feature = "benchmark-harness"))]
mod benchmark_peer_tls_tests {
    use std::num::NonZeroU16;

    use atm_storage_rusqlite::SqliteStorageBackend;

    use super::{
        BENCHMARK_PEER_HOST, BENCHMARK_PEER_IDENTITY_FILE, BENCHMARK_PEER_PORT,
        configure_disposable_benchmark_peer_tls,
    };

    #[test]
    fn disposable_benchmark_configuration_is_valid_for_the_canonical_mtls_adapter() {
        let directory = tempfile::tempdir().expect("temporary benchmark identity directory");
        let backend = SqliteStorageBackend::new(directory.path().join("mail.db"))
            .expect("open disposable benchmark database");
        let store = backend.peer_config_store();

        configure_disposable_benchmark_peer_tls(store.as_ref(), directory.path())
            .expect("configure disposable benchmark mTLS");

        let certificate = store
            .local_certificate()
            .expect("load disposable benchmark certificate")
            .expect("benchmark certificate is durable");
        assert_eq!(
            certificate.private_key_ref.as_str(),
            directory
                .path()
                .join(BENCHMARK_PEER_IDENTITY_FILE)
                .display()
                .to_string(),
            "durable configuration references only the disposable identity path"
        );
        assert!(
            directory
                .path()
                .join(BENCHMARK_PEER_IDENTITY_FILE)
                .is_file(),
            "benchmark identity bundle is created under the disposable ATM_HOME"
        );
        let interfaces = store.list_interfaces().expect("load benchmark interface");
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].advertise_host.as_str(), BENCHMARK_PEER_HOST);
        assert_eq!(interfaces[0].bind_addr.port(), BENCHMARK_PEER_PORT);
        assert!(interfaces[0].enabled);
        let peer = store
            .trusted_peer(&BENCHMARK_PEER_HOST.parse().expect("benchmark peer host"))
            .expect("load benchmark trusted peer")
            .expect("benchmark peer is durable");
        assert_eq!(peer.fingerprint, certificate.fingerprint);
        assert_eq!(
            peer.https_port,
            NonZeroU16::new(BENCHMARK_PEER_PORT).expect("non-zero port")
        );
        assert!(peer.enabled);
        peer_tls::mtls_adapter(store)
            .expect("configured benchmark peer starts canonical mTLS adapter");
    }
}
