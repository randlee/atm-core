//! Windows local HTTP transport.
//!
//! Windows has one local ingress: HTTP/1.1 over a daemon-owned loopback TCP
//! listener.  It authenticates the local client with the runtime capability
//! record before a request reaches the shared router.

#![cfg_attr(test, allow(dead_code, unused_imports))]

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use std::time::Instant;

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt as _;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

use atm_core::api::{
    ApiRouter, AuthenticatedIngress, RequestDeadline, read_http_request, write_http_response,
};
use atm_core::error::AtmError;
use atm_core::local_http::{LOCAL_CAPABILITY_HEADER, LocalCapability, LocalHttpEndpointRecord};
use ulid::Ulid;

use crate::SubsystemObservability;
use crate::active_connection_registry::ActiveConnectionRegistry;
use crate::active_connection_registry::TrackedDispatchHandle;
use crate::host_ownership::{HostOwnershipAdapter, HostOwnershipGuard};
use crate::lifecycle_control::LifecycleControlSourceAdapter;

const REQUEST_DEADLINE: Duration = Duration::from_secs(3);
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(25);
pub(crate) const MAX_CONCURRENT_CONNECTIONS: usize = 64;

/// Removes the runtime capability record when this listener stops.
#[derive(Debug)]
pub(crate) struct SocketEndpointGuard {
    record_path: PathBuf,
}

impl SocketEndpointGuard {
    fn new(record_path: PathBuf) -> Self {
        Self { record_path }
    }

    fn unpublish(&self) -> Result<(), AtmError> {
        revoke_record(&self.record_path)?;
        match fs::remove_file(&self.record_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(AtmError::daemon_unavailable(format!(
                "failed to remove local HTTP endpoint record {}: {source}",
                self.record_path.display()
            ))),
        }
    }
}

impl Drop for SocketEndpointGuard {
    fn drop(&mut self) {
        let _ = self.unpublish();
    }
}

pub(crate) struct RuntimeServeHooks<BeginShutdown, ReloadRuntimeView, PublishReady> {
    pub(crate) endpoint_guard: SocketEndpointGuard,
    pub(crate) graceful_drain_deadline: Duration,
    pub(crate) force_cancel_deadline: Duration,
    pub(crate) begin_shutdown: BeginShutdown,
    pub(crate) reload_runtime_view: ReloadRuntimeView,
    pub(crate) publish_ready: PublishReady,
}

pub(crate) struct PreparedRuntimeServer {
    _ownership: HostOwnershipGuard,
    listener: TcpListener,
    capability: LocalCapability,
    lifecycle: LifecycleControlSourceAdapter,
    endpoint_guard: Option<SocketEndpointGuard>,
}

impl PreparedRuntimeServer {
    fn bind(
        home_dir: &Path,
        observability: SubsystemObservability,
        host_ownership_observability: SubsystemObservability,
        lifecycle_observability: SubsystemObservability,
    ) -> Result<Self, AtmError> {
        let lifecycle =
            LifecycleControlSourceAdapter::install_with_observability(lifecycle_observability)?;
        let ownership =
            HostOwnershipAdapter::new_with_observability(host_ownership_observability).acquire()?;
        Self::bind_after_install(home_dir, observability, lifecycle, ownership)
    }

    #[cfg(test)]
    fn bind_at_home_for_test(
        home_dir: &Path,
        observability: SubsystemObservability,
        host_ownership_observability: SubsystemObservability,
        lifecycle_observability: SubsystemObservability,
    ) -> Result<Self, AtmError> {
        let lifecycle =
            LifecycleControlSourceAdapter::install_with_observability(lifecycle_observability)?;
        let ownership = HostOwnershipAdapter::new_with_observability(host_ownership_observability)
            .acquire_at_home_for_test(home_dir)?;
        Self::bind_after_install(home_dir, observability, lifecycle, ownership)
    }

    fn bind_after_install(
        home_dir: &Path,
        observability: SubsystemObservability,
        lifecycle: LifecycleControlSourceAdapter,
        ownership: HostOwnershipGuard,
    ) -> Result<Self, AtmError> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to bind local loopback HTTP listener: {source}"
            ))
        })?;
        listener.set_nonblocking(true).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to configure local loopback HTTP listener: {source}"
            ))
        })?;
        let capability = LocalCapability::generate()?;
        let record_path = atm_core::local_http::local_http_record_path(home_dir);
        publish_record(
            &record_path,
            ownership.instance_id(),
            listener.local_addr().map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to read local loopback HTTP address: {source}"
                ))
            })?,
            &capability,
        )?;
        observability.emit_or_warn(
            "bind_listener",
            "ok",
            "daemon local loopback HTTP listener prepared",
        );
        Ok(Self {
            _ownership: ownership,
            listener,
            capability,
            lifecycle,
            endpoint_guard: Some(SocketEndpointGuard::new(record_path)),
        })
    }

    pub(crate) fn take_endpoint_guard(&mut self) -> Result<SocketEndpointGuard, AtmError> {
        self.endpoint_guard.take().ok_or_else(|| {
            AtmError::daemon_unavailable("local HTTP endpoint guard missing during startup")
        })
    }

    pub(crate) fn serve_with_runtime_hooks<BeginShutdown, ReloadRuntimeView, PublishReady>(
        self,
        router: Arc<dyn ApiRouter + Send + Sync>,
        hooks: RuntimeServeHooks<BeginShutdown, ReloadRuntimeView, PublishReady>,
    ) -> Result<(), AtmError>
    where
        BeginShutdown: Fn() -> Result<(), AtmError>,
        ReloadRuntimeView: Fn() -> Result<(), AtmError>,
        PublishReady: Fn() -> Result<(), AtmError>,
    {
        let Self {
            _ownership,
            listener,
            capability,
            lifecycle,
            endpoint_guard: _,
        } = self;
        (hooks.publish_ready)()?;
        let registry = Arc::new(ActiveConnectionRegistry::default());
        let force_shutdown = Arc::new(AtomicBool::new(false));
        loop {
            if lifecycle.terminate_requested() {
                (hooks.begin_shutdown)()?;
                drain_active_connections_for_shutdown(
                    registry.as_ref(),
                    force_shutdown.as_ref(),
                    hooks.graceful_drain_deadline,
                    hooks.force_cancel_deadline,
                    Instant::now(),
                    REQUEST_DEADLINE,
                )?;
                drop(hooks.endpoint_guard);
                return Ok(());
            }
            if lifecycle.take_reload_requested() {
                (hooks.reload_runtime_view)()?;
            }
            match listener.accept() {
                Ok((stream, peer)) => {
                    if !peer.ip().is_loopback()
                        || registry.active_connections() >= MAX_CONCURRENT_CONNECTIONS
                    {
                        continue;
                    }
                    let router = Arc::clone(&router);
                    let capability = capability.clone();
                    let dispatch_registry = Arc::clone(&registry);
                    let force_shutdown = Arc::clone(&force_shutdown);
                    let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(1);
                    let join_handle = thread::spawn(move || {
                        let _active = dispatch_registry.register();
                        let _dispatch = dispatch_registry.register_dispatch_work();
                        let _ =
                            handle_connection(stream, router, &capability, force_shutdown.as_ref());
                        let _ = completion_tx.send(());
                    });
                    registry.push_dispatch_handle(
                        TrackedDispatchHandle {
                            completion_rx,
                            join_handle,
                        },
                        MAX_CONCURRENT_CONNECTIONS,
                    )?;
                }
                Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                    registry.reap_finished_dispatches()?;
                    thread::sleep(ACCEPT_POLL_INTERVAL);
                }
                Err(source) => {
                    return Err(AtmError::daemon_unavailable(format!(
                        "local loopback HTTP listener accept failed: {source}"
                    )));
                }
            }
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    router: Arc<dyn ApiRouter + Send + Sync>,
    capability: &LocalCapability,
    force_shutdown: &AtomicBool,
) -> Result<(), AtmError> {
    stream
        .set_read_timeout(Some(REQUEST_DEADLINE))
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to set local HTTP read deadline: {source}"
            ))
        })?;
    stream
        .set_write_timeout(Some(REQUEST_DEADLINE))
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to set local HTTP write deadline: {source}"
            ))
        })?;
    let Some(request) = read_http_request(&mut stream)? else {
        return Ok(());
    };
    let response = if force_shutdown.load(Ordering::SeqCst) {
        atm_core::ResponseEnvelope::Error(AtmError::daemon_unavailable(
            "daemon is shutting down and not accepting new requests",
        ))
    } else if !request
        .header(LOCAL_CAPABILITY_HEADER)
        .is_some_and(|value| capability.matches_header(value))
    {
        atm_core::ResponseEnvelope::Error(AtmError::validation(
            "local HTTP capability is missing, invalid, stale, or revoked",
        ))
    } else {
        match atm_core::api::decode_request(request) {
            Ok(request) => router
                .route(
                    request,
                    AuthenticatedIngress::Local,
                    RequestDeadline::after(REQUEST_DEADLINE),
                )
                .map(|response| response.into_inner())
                .unwrap_or_else(atm_core::ResponseEnvelope::Error),
            Err(error) => atm_core::ResponseEnvelope::Error(error),
        }
    };
    write_http_response(&mut stream, &response)
}

fn drain_active_connections_for_shutdown(
    registry: &ActiveConnectionRegistry,
    force_shutdown: &AtomicBool,
    graceful_drain_deadline: Duration,
    force_cancel_deadline: Duration,
    shutdown_started: Instant,
    tracked_dispatch_join_deadline: Duration,
) -> Result<(), AtmError> {
    tracing::info!(
        active_connections = registry.active_connections(),
        active_work_items = registry.active_work_items(),
        "daemon local HTTP shutdown signal received; starting graceful drain"
    );
    let graceful_deadline = shutdown_started + graceful_drain_deadline;
    let force_cancel_deadline = shutdown_started + force_cancel_deadline;
    while registry.active_work_items() > 0 && Instant::now() < graceful_deadline {
        registry.wait_for_connection_change(
            graceful_deadline.saturating_duration_since(Instant::now()),
        )?;
    }
    if registry.active_work_items() > 0 {
        force_shutdown.store(true, Ordering::SeqCst);
        registry.interrupt_all();
    }
    while registry.active_work_items() > 0 && Instant::now() < force_cancel_deadline {
        registry.wait_for_connection_change(
            force_cancel_deadline.saturating_duration_since(Instant::now()),
        )?;
    }
    registry.join_tracked_dispatches(tracked_dispatch_join_deadline)?;
    let remaining_work_items = registry.active_work_items();
    if remaining_work_items > 0 {
        return Err(AtmError::daemon_unavailable(format!(
            "forced cancel deadline elapsed with {remaining_work_items} active daemon work item(s)"
        )));
    }
    Ok(())
}

fn publish_record(
    record_path: &Path,
    daemon_instance_id: Ulid,
    endpoint: SocketAddr,
    capability: &LocalCapability,
) -> Result<(), AtmError> {
    if !endpoint.ip().is_loopback() {
        return Err(AtmError::validation(
            "local HTTP listener must bind only a loopback address",
        ));
    }
    let parent = record_path
        .parent()
        .ok_or_else(|| AtmError::validation("local HTTP record has no parent directory"))?;
    fs::create_dir_all(parent).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to create local HTTP runtime directory {}: {source}",
            parent.display()
        ))
    })?;
    let record =
        LocalHttpEndpointRecord::active(daemon_instance_id, Some(endpoint), None, capability);
    let contents = serde_json::to_vec_pretty(&record).map_err(AtmError::from)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(record_path)
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to create local HTTP record {}: {source}",
                record_path.display()
            ))
        })?;
    file.write_all(&contents).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to write local HTTP record {}: {source}",
            record_path.display()
        ))
    })?;
    file.sync_all().map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to sync local HTTP record {}: {source}",
            record_path.display()
        ))
    })?;
    #[cfg(unix)]
    fs::set_permissions(record_path, fs::Permissions::from_mode(0o600)).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to restrict local HTTP record {} to its owner: {source}",
            record_path.display()
        ))
    })?;
    #[cfg(windows)]
    restrict_record_to_current_owner(record_path)?;
    Ok(())
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Windows owner-only ACL FFI is confined to this function"
)]
fn restrict_record_to_current_owner(record_path: &Path) -> Result<(), AtmError> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        SetFileSecurityW,
    };

    let path = record_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // `OW` is the current file owner's SID. A protected DACL containing only
    // this ACE prevents inherited user/group read access to the bearer secret.
    let descriptor_text = "D:P(A;;FA;;;OW)\0".encode_utf16().collect::<Vec<_>>();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let mut descriptor_size = 0_u32;
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor_text.as_ptr(),
            1,
            &mut descriptor,
            &mut descriptor_size,
        )
    };
    if converted == 0 {
        return Err(AtmError::daemon_unavailable(
            "failed to create Windows owner-only ACL",
        ));
    }
    let applied = unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    unsafe { LocalFree(descriptor) };
    if applied == 0 {
        return Err(AtmError::daemon_unavailable(format!(
            "failed to restrict local HTTP record {} to its Windows owner",
            record_path.display()
        )));
    }
    Ok(())
}

fn revoke_record(record_path: &Path) -> Result<(), AtmError> {
    let contents = match fs::read(record_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(AtmError::daemon_unavailable(format!(
                "failed to read local HTTP record {} for revocation: {source}",
                record_path.display()
            )));
        }
    };
    let mut record: LocalHttpEndpointRecord =
        serde_json::from_slice(&contents).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to parse local HTTP record {} for revocation: {source}",
                record_path.display()
            ))
        })?;
    record.revoke();
    let contents = serde_json::to_vec_pretty(&record).map_err(AtmError::from)?;
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(record_path)
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to open local HTTP record {} for revocation: {source}",
                record_path.display()
            ))
        })?;
    file.write_all(&contents).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to write revoked local HTTP record {}: {source}",
            record_path.display()
        ))
    })?;
    file.sync_all().map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to sync revoked local HTTP record {}: {source}",
            record_path.display()
        ))
    })
}

#[derive(Debug, Clone)]
pub(crate) struct LocalHttpServerTransportAdapter {
    observability: SubsystemObservability,
    host_ownership_observability: SubsystemObservability,
    lifecycle_observability: SubsystemObservability,
}

impl LocalHttpServerTransportAdapter {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::new_with_observability(
            SubsystemObservability::disabled(crate::DaemonSubsystem::LocalHttpTransport),
            SubsystemObservability::disabled(crate::DaemonSubsystem::HostOwnership),
            SubsystemObservability::disabled(crate::DaemonSubsystem::LifecycleControl),
        )
    }
    pub(crate) fn new_with_observability(
        observability: SubsystemObservability,
        host_ownership_observability: SubsystemObservability,
        lifecycle_observability: SubsystemObservability,
    ) -> Self {
        Self {
            observability,
            host_ownership_observability,
            lifecycle_observability,
        }
    }

    pub(crate) fn prepare_runtime(&self) -> Result<PreparedRuntimeServer, AtmError> {
        let home_dir = atm_core::home::atm_home()?;
        PreparedRuntimeServer::bind(
            &home_dir,
            self.observability.clone(),
            self.host_ownership_observability.clone(),
            self.lifecycle_observability.clone(),
        )
    }

    #[cfg(test)]
    pub(crate) fn prepare_runtime_at_socket_path_for_home(
        &self,
        _socket_path: PathBuf,
        home_dir: &Path,
    ) -> Result<PreparedRuntimeServer, AtmError> {
        PreparedRuntimeServer::bind_at_home_for_test(
            home_dir,
            self.observability.clone(),
            self.host_ownership_observability.clone(),
            self.lifecycle_observability.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::net::{Ipv4Addr, TcpListener, TcpStream};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use atm_core::api::{read_http_response, write_http_request_with_headers};
    use atm_core::doctor::DoctorQuery;
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
    use ulid::Ulid;

    use super::{
        LOCAL_CAPABILITY_HEADER, LocalCapability, drain_active_connections_for_shutdown,
        handle_connection,
    };
    use crate::test_support::DoctorOnlyDispatcher;

    fn serve_one(capability: LocalCapability) -> (std::net::SocketAddr, thread::JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind loopback");
        let address = listener.local_addr().expect("address");
        let server = thread::spawn(move || {
            let (stream, peer) = listener.accept().expect("accept client");
            assert!(peer.ip().is_loopback());
            handle_connection(
                stream,
                Arc::new(DoctorOnlyDispatcher),
                &capability,
                &std::sync::atomic::AtomicBool::new(false),
            )
            .expect("serve local request");
        });
        (address, server)
    }

    #[test]
    fn loopback_tcp_capability_reaches_shared_router() {
        let capability = LocalCapability::generate().expect("capability");
        let header = capability.to_base64url();
        let (address, server) = serve_one(capability);
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        let mut stream = TcpStream::connect(address).expect("connect");

        write_http_request_with_headers(
            &mut stream,
            &request,
            &[(LOCAL_CAPABILITY_HEADER, header.as_str())],
        )
        .expect("write request");
        stream.flush().expect("flush request");
        let response = read_http_response(&mut stream, &request).expect("read response");

        assert!(matches!(response, ResponseEnvelope::Doctor(_)));
        server.join().expect("server join");
    }

    #[test]
    fn invalid_capability_is_rejected_before_router() {
        let capability = LocalCapability::generate().expect("capability");
        let (address, server) = serve_one(capability);
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        let mut stream = TcpStream::connect(address).expect("connect");

        write_http_request_with_headers(
            &mut stream,
            &request,
            &[(LOCAL_CAPABILITY_HEADER, "not-a-capability")],
        )
        .expect("write request");
        stream.flush().expect("flush request");
        let response = read_http_response(&mut stream, &request).expect("read response");

        assert!(matches!(response, ResponseEnvelope::Error(error) if error.is_validation()));
        server.join().expect("server join");
    }

    #[cfg(unix)]
    #[test]
    fn endpoint_record_is_owner_readable_only() {
        use std::os::unix::fs::PermissionsExt as _;

        let tempdir = tempfile::tempdir().expect("tempdir");
        let record = tempdir.path().join("local-http.json");
        let capability = LocalCapability::generate().expect("capability");

        super::publish_record(
            &record,
            Ulid::new(),
            "127.0.0.1:43101".parse().expect("loopback endpoint"),
            &capability,
        )
        .expect("publish record");

        assert_eq!(
            std::fs::metadata(record)
                .expect("record metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn tcp_listener_drain_honors_the_force_cancel_deadline() {
        use std::sync::atomic::AtomicBool;
        use std::sync::mpsc;
        use std::time::Instant;

        use crate::active_connection_registry::{ActiveConnectionRegistry, TrackedDispatchHandle};
        let registry = Arc::new(ActiveConnectionRegistry::default());
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let (registered_tx, registered_rx) = mpsc::sync_channel(1);
        let (completion_tx, completion_rx) = mpsc::sync_channel(1);
        let worker_registry = Arc::clone(&registry);
        let join_handle = thread::spawn(move || {
            let _connection = worker_registry.register();
            let _dispatch = worker_registry.register_dispatch_work();
            registered_tx.send(()).expect("signal registered worker");
            let _ = release_rx.recv();
            let _ = completion_tx.send(());
        });
        registry
            .push_dispatch_handle(
                TrackedDispatchHandle {
                    completion_rx,
                    join_handle,
                },
                super::MAX_CONCURRENT_CONNECTIONS,
            )
            .expect("track TCP listener worker");
        registered_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker registered before shutdown drain");

        let force_shutdown = AtomicBool::new(false);
        let started = Instant::now();
        let error = drain_active_connections_for_shutdown(
            registry.as_ref(),
            &force_shutdown,
            Duration::from_millis(10),
            Duration::from_millis(20),
            started,
            Duration::from_millis(25),
        )
        .expect_err("TCP listener shutdown must not wait indefinitely");

        assert!(force_shutdown.load(std::sync::atomic::Ordering::SeqCst));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(error.message().contains("shutdown join deadline"));
        let _ = release_tx.send(());
    }
}
