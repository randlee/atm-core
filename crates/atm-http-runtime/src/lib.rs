//! Tokio HTTP runtime composition contract for ATM.
//!
//! This crate owns the replacement Tokio/Axum listener and canonical typed
//! message route. It validates runtime-owned configuration before binding and
//! keeps lifecycle ownership with the Tokio task that serves that route. See
//! the [Phase AL/AM runtime boundary checklist](../../../docs/plans/phase-al-am-runtime-boundary-checklist.md).
//!
//! The only ATM dependency is `atm-core`, specifically its existing sealed
//! canonical [`atm_core::AtmError`] and the existing core storage and hook
//! contracts supplied by replacement composition.
//! Runtime construction never accepts a storage backend, tmux, graft, CLI, or
//! daemon-bootstrap type.

//! The state-owning handle deliberately has consuming transitions.  This is
//! part of the public contract, not merely an implementation detail:
//!
//! ```compile_fail
//! use atm_http_runtime::{Configured, HttpRuntime};
//!
//! async fn cannot_start_twice(runtime: HttpRuntime<Configured>) {
//!     let running = runtime.start().await.expect("first transition");
//!     let _ = runtime.start().await; // use after move: does not compile
//!     let _ = running;
//! }
//! ```
//!
//! ```compile_fail
//! use atm_http_runtime::{Configured, HttpRuntime};
//!
//! fn cannot_shutdown_before_start(runtime: HttpRuntime<Configured>) {
//!     let _ = runtime.begin_shutdown(); // method is not available yet
//! }
//! ```

use std::net::SocketAddr;
use std::num::{NonZeroU32, NonZeroUsize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use atm_core::error::AtmError;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

mod client;
mod message_handler;
mod storage_and_nudge_router;

#[cfg(unix)]
pub use client::unix_socket_client;
pub use message_handler::{
    AuthenticatedConnector, CanonicalWriteHandler, canonical_message_router,
};
pub use storage_and_nudge_router::StorageAndNudgeRouter;

/// Validated configuration for the maintained Tokio HTTP runtime.
///
/// The fields remain private so composition cannot bypass validation before a
/// listener is introduced in a later AL sprint.
#[derive(Debug, Clone)]
pub struct HttpRuntimeConfig {
    bind_address: SocketAddr,
    unix_socket: Option<UnixSocketConfig>,
    limits: RuntimeLimits,
    timeouts: RuntimeTimeouts,
}

impl HttpRuntimeConfig {
    /// Creates runtime configuration with one required TCP bind and an optional
    /// additive Unix-domain socket bind.
    ///
    /// A Unix socket never replaces the TCP bind: later AL adapters may serve
    /// both transports, so `bind_address` must name a publishable non-zero TCP
    /// port whether or not `unix_socket` is configured.
    #[must_use]
    pub fn new(
        bind_address: SocketAddr,
        unix_socket: Option<UnixSocketConfig>,
        limits: RuntimeLimits,
        timeouts: RuntimeTimeouts,
    ) -> Self {
        Self {
            bind_address,
            unix_socket,
            limits,
            timeouts,
        }
    }
}

/// Unix-domain socket preflight input.
///
/// The data type remains available on every target so shared configuration can
/// be decoded consistently, but a configured Unix socket is accepted only on
/// Unix. AL.1 never binds it; later Unix adapter work owns binding, ownership,
/// and permission application.
#[derive(Debug, Clone)]
pub struct UnixSocketConfig {
    #[cfg_attr(
        not(unix),
        expect(
            dead_code,
            reason = "AL.5 owns Unix socket ownership application; AL.1 retains its validated configuration input"
        )
    )]
    path: PathBuf,
    #[cfg_attr(
        not(unix),
        expect(
            dead_code,
            reason = "AL.5 owns Unix socket ownership application; AL.1 retains its validated configuration input"
        )
    )]
    owner_uid: NonZeroU32,
    #[cfg_attr(
        not(unix),
        expect(
            dead_code,
            reason = "AL.5 owns Unix socket ownership application; AL.1 retains its validated configuration input"
        )
    )]
    mode: NonZeroU32,
}

impl UnixSocketConfig {
    #[must_use]
    pub fn new(path: PathBuf, owner_uid: NonZeroU32, mode: NonZeroU32) -> Self {
        Self {
            path,
            owner_uid,
            mode,
        }
    }
}

/// Bounded HTTP admission settings.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeLimits {
    max_body_bytes: usize,
    max_connections: usize,
}

impl RuntimeLimits {
    #[must_use]
    pub const fn new(max_body_bytes: NonZeroUsize, max_connections: NonZeroUsize) -> Self {
        Self {
            max_body_bytes: max_body_bytes.get(),
            max_connections: max_connections.get(),
        }
    }
}

/// A non-zero duration required by runtime timeout configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonZeroDuration(Duration);

impl NonZeroDuration {
    #[must_use]
    pub const fn new(value: Duration) -> Option<Self> {
        if value.is_zero() {
            None
        } else {
            Some(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

/// Absolute limits used by the future framework adapter.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeTimeouts {
    request: Duration,
    shutdown: Duration,
}

impl RuntimeTimeouts {
    #[must_use]
    pub const fn new(request: NonZeroDuration, shutdown: NonZeroDuration) -> Self {
        Self {
            request: request.get(),
            shutdown: shutdown.get(),
        }
    }
}

/// Composition input for the replacement async write boundary.
#[derive(Clone)]
pub struct HttpRuntimeBuilder {
    config: HttpRuntimeConfig,
    handler: Arc<dyn CanonicalWriteHandler>,
}

impl HttpRuntimeBuilder {
    #[must_use]
    pub fn new(config: HttpRuntimeConfig, handler: Arc<dyn CanonicalWriteHandler>) -> Self {
        Self { config, handler }
    }

    /// Validates all runtime-owned input without binding or publishing.
    ///
    /// # Errors
    ///
    /// Returns the existing configuration error with the invalid field and cause.
    pub fn build(self) -> Result<HttpRuntime<Configured>, AtmError> {
        validate_config(&self.config)?;
        Ok(HttpRuntime {
            config: self.config,
            handler: self.handler,
            state: Configured,
        })
    }
}

/// Validated but not started runtime state.
pub struct Configured;
/// Runtime lifecycle state while its owned Axum server is accepting requests.
pub struct Running {
    local_address: SocketAddr,
    shutdown_tx: watch::Sender<()>,
    server_task: JoinHandle<std::io::Result<()>>,
}
/// Runtime lifecycle state after cancellation and while the Axum task drains.
pub struct Draining {
    server_task: JoinHandle<std::io::Result<()>>,
}
/// Terminal lifecycle state with no live runtime-owned handles.
pub struct Stopped;

/// Non-cloneable lifecycle owner. State transitions consume this value.
pub struct HttpRuntime<State> {
    config: HttpRuntimeConfig,
    handler: Arc<dyn CanonicalWriteHandler>,
    state: State,
}

impl HttpRuntime<Configured> {
    /// Binds the replacement listener(s) and starts their one owned Axum task.
    ///
    /// The caller supplies the Tokio runtime. This method never creates a
    /// nested runtime and all request handling runs through the one typed
    /// route built from the injected application boundary.
    ///
    /// # Errors
    ///
    /// Returns `AtmError` when the configured TCP address cannot be bound or
    /// its local address cannot be read. A configured Unix socket is bound
    /// additively and uses the same router as the TCP listener.
    pub async fn start(self) -> Result<HttpRuntime<Running>, AtmError> {
        let listener = TcpListener::bind(self.config.bind_address)
            .await
            .map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to bind replacement HTTP runtime at {}: {source}",
                    self.config.bind_address
                ))
            })?;
        let local_address = listener.local_addr().map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to read replacement HTTP runtime address: {source}"
            ))
        })?;
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let router = canonical_message_router(
            Arc::clone(&self.handler),
            AuthenticatedConnector::local(),
            self.config.limits,
            self.config.timeouts,
        );
        #[cfg(unix)]
        let unix_listener = self
            .config
            .unix_socket
            .as_ref()
            .map(bind_unix_listener)
            .transpose()?;
        let server_task = {
            #[cfg(unix)]
            if let Some((unix_listener, socket_cleanup)) = unix_listener {
                let tcp_shutdown = shutdown_rx.clone();
                let uds_shutdown = shutdown_rx;
                let tcp_router = router.clone();
                tokio::spawn(async move {
                    // The guard owns cleanup for precisely the inode bound by
                    // this runtime. It cannot unlink a replacement socket.
                    let _socket_cleanup = socket_cleanup;
                    tokio::try_join!(
                        axum::serve(listener, tcp_router)
                            .with_graceful_shutdown(wait_for_shutdown(tcp_shutdown)),
                        axum::serve(unix_listener, router)
                            .with_graceful_shutdown(wait_for_shutdown(uds_shutdown)),
                    )
                    .map(|_| ())
                })
            } else {
                tokio::spawn(async move {
                    axum::serve(listener, router)
                        .with_graceful_shutdown(wait_for_shutdown(shutdown_rx))
                        .await
                })
            }
            #[cfg(not(unix))]
            tokio::spawn(async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(wait_for_shutdown(shutdown_rx))
                    .await
            })
        };
        Ok(HttpRuntime {
            config: self.config,
            handler: self.handler,
            state: Running {
                local_address,
                shutdown_tx,
                server_task,
            },
        })
    }
}

async fn wait_for_shutdown(mut shutdown_rx: watch::Receiver<()>) {
    let _ = shutdown_rx.changed().await;
}

#[cfg(unix)]
fn bind_unix_listener(
    socket: &UnixSocketConfig,
) -> Result<(UnixListener, UnixSocketPathGuard), AtmError> {
    use std::fs;
    use std::io::ErrorKind;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    match fs::symlink_metadata(&socket.path) {
        Ok(_) => {
            return Err(AtmError::config("Unix HTTP socket path is already occupied").with_cause(
                format!(
                    "refusing to replace existing path `{}`; remove only the stale owner-owned socket before retrying",
                    socket.path.display()
                ),
            ));
        }
        Err(source) if source.kind() == ErrorKind::NotFound => {}
        Err(source) => {
            return Err(AtmError::config("cannot inspect Unix HTTP socket path").with_cause(source));
        }
    }
    let listener = UnixListener::bind(&socket.path).map_err(|source| {
        AtmError::daemon_unavailable("failed to bind replacement Unix HTTP socket")
            .with_cause(source)
    })?;
    let cleanup = UnixSocketPathGuard::capture(&socket.path)?;
    fs::set_permissions(&socket.path, fs::Permissions::from_mode(socket.mode.get())).map_err(
        |source| {
            AtmError::daemon_unavailable("failed to set replacement Unix HTTP socket permissions")
                .with_cause(source)
        },
    )?;
    let metadata = fs::metadata(&socket.path).map_err(|source| {
        AtmError::daemon_unavailable("failed to inspect replacement Unix HTTP socket permissions")
            .with_cause(source)
    })?;
    if metadata.uid() != socket.owner_uid.get() {
        return Err(AtmError::config(
            "replacement Unix HTTP socket owner does not match configuration",
        )
        .with_cause(format!(
            "configured uid {} but bound socket is owned by uid {}",
            socket.owner_uid.get(),
            metadata.uid()
        )));
    }
    if metadata.mode() & 0o777 != socket.mode.get() {
        return Err(AtmError::config(
            "replacement Unix HTTP socket permissions do not match configuration",
        )
        .with_cause(format!(
            "configured mode {:o} but bound socket mode is {:o}",
            socket.mode.get(),
            metadata.mode() & 0o777
        )));
    }
    Ok((listener, cleanup))
}

/// Removes only the socket inode created by this runtime during shutdown.
#[cfg(unix)]
#[derive(Debug)]
struct UnixSocketPathGuard {
    path: PathBuf,
    device: u64,
    inode: u64,
}

#[cfg(unix)]
impl UnixSocketPathGuard {
    fn capture(path: &std::path::Path) -> Result<Self, AtmError> {
        use std::fs;
        use std::os::unix::fs::MetadataExt;

        let metadata = fs::metadata(path).map_err(|source| {
            AtmError::daemon_unavailable("failed to inspect bound Unix HTTP socket")
                .with_cause(source)
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

#[cfg(unix)]
impl Drop for UnixSocketPathGuard {
    fn drop(&mut self) {
        use std::fs;
        use std::os::unix::fs::MetadataExt;

        let is_our_socket = fs::metadata(&self.path)
            .is_ok_and(|metadata| metadata.dev() == self.device && metadata.ino() == self.inode);
        if is_our_socket {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl HttpRuntime<Running> {
    /// Returns the actual listener address selected at start.
    #[must_use]
    pub const fn local_address(&self) -> SocketAddr {
        self.state.local_address
    }

    /// Consumes the only running owner and begins the drain transition.
    #[must_use]
    pub fn begin_shutdown(self) -> HttpRuntime<Draining> {
        let _ = self.state.shutdown_tx.send(());
        HttpRuntime {
            config: self.config,
            handler: self.handler,
            state: Draining {
                server_task: self.state.server_task,
            },
        }
    }
}

impl HttpRuntime<Draining> {
    /// Completes the drain transition.
    ///
    /// The runtime waits only for its actual Axum task. A shutdown deadline
    /// aborts and joins that task so no replacement-runtime task is detached.
    ///
    /// # Errors
    ///
    /// Returns `AtmError` when the server fails while draining or exceeds the
    /// configured shutdown bound.
    pub async fn finish(self) -> Result<HttpRuntime<Stopped>, AtmError> {
        let mut server_task = self.state.server_task;
        let finished = tokio::time::timeout(self.config.timeouts.shutdown, &mut server_task).await;
        match finished {
            Ok(Ok(Ok(()))) => Ok(HttpRuntime {
                config: self.config,
                handler: self.handler,
                state: Stopped,
            }),
            Ok(Ok(Err(source))) => Err(AtmError::daemon_unavailable(format!(
                "replacement HTTP runtime stopped with an I/O error: {source}"
            ))),
            Ok(Err(source)) => Err(AtmError::daemon_unavailable(format!(
                "replacement HTTP runtime task ended unexpectedly: {source}"
            ))),
            Err(_) => {
                server_task.abort();
                let _ = server_task.await;
                Err(AtmError::daemon_unavailable(
                    "replacement HTTP runtime exceeded its shutdown deadline",
                ))
            }
        }
    }
}

fn validate_config(config: &HttpRuntimeConfig) -> Result<(), AtmError> {
    debug_assert!(config.limits.max_body_bytes > 0);
    debug_assert!(config.limits.max_connections > 0);
    debug_assert!(!config.timeouts.request.is_zero());
    debug_assert!(!config.timeouts.shutdown.is_zero());
    if config.bind_address.port() == 0 {
        return Err(preflight(
            "bind_address",
            "port 0 cannot be published; Unix socket configuration is additive and cannot replace TCP",
        ));
    }
    #[cfg(not(unix))]
    if config.unix_socket.is_some() {
        return Err(preflight(
            "unix_socket",
            "Unix-domain socket configuration is unsupported on this platform",
        ));
    }
    #[cfg(unix)]
    if let Some(socket) = &config.unix_socket {
        if socket.path.as_os_str().is_empty() {
            return Err(preflight("unix_socket.path", "must not be empty"));
        }
        if socket.mode.get() & !0o777 != 0 {
            return Err(preflight(
                "unix_socket.mode",
                "must contain only permission bits",
            ));
        }
    }
    Ok(())
}

fn preflight(field: &str, cause: impl std::fmt::Display) -> AtmError {
    AtmError::config(format!("invalid runtime configuration field `{field}`")).with_cause(cause)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::num::{NonZeroU32, NonZeroUsize};
    use std::sync::Arc;
    use std::time::Duration;

    #[cfg(unix)]
    use atm_core::api::ApiRequest;
    use atm_core::api::{ApiResponse, AuthenticatedIngress, RequestDeadline};
    use atm_core::error::AtmError;
    #[cfg(unix)]
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
    #[cfg(unix)]
    use atm_core::send::{SendMessageSource, SendRequest};
    #[cfg(unix)]
    use atm_core::test_support::{TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
    #[cfg(unix)]
    use atm_core::types::{AgentName, TeamName};

    use super::{
        CanonicalWriteHandler, HttpRuntimeBuilder, HttpRuntimeConfig, NonZeroDuration,
        RuntimeLimits, RuntimeTimeouts, UnixSocketConfig,
    };

    struct TestRouter;

    impl atm_core::boundary::sealed::Sealed for TestRouter {}

    impl CanonicalWriteHandler for TestRouter {
        fn write(
            &self,
            _request: atm_core::send::WriteRequest,
            _ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<ApiResponse, AtmError>> + Send + '_>,
        > {
            Box::pin(async {
                Err(AtmError::validation(
                    "test handler is not invoked by the lifecycle contract",
                ))
            })
        }
    }

    #[cfg(unix)]
    struct CanonicalUdsRouter;

    #[cfg(unix)]
    impl atm_core::boundary::sealed::Sealed for CanonicalUdsRouter {}

    #[cfg(unix)]
    impl CanonicalWriteHandler for CanonicalUdsRouter {
        fn write(
            &self,
            _request: atm_core::send::WriteRequest,
            _ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<ApiResponse, AtmError>> + Send + '_>,
        > {
            Box::pin(async {
                Ok(ApiResponse::new(ResponseEnvelope::Error(
                    AtmError::validation("canonical UDS test handler reached"),
                )))
            })
        }
    }

    #[cfg(unix)]
    fn write_request() -> RequestEnvelope {
        RequestEnvelope::Write(Box::new(
            SendRequest::new(
                ".".into(),
                ".".into(),
                AgentName::from_validated(TEST_SENDER),
                TEST_RECIPIENT,
                TeamName::from_validated(TEST_TEAM),
                SendMessageSource::Inline("Unix runtime test".to_owned()),
                None,
                false,
                None,
                false,
            )
            .expect("test write request"),
        ))
    }

    #[cfg(unix)]
    fn available_tcp_port() -> u16 {
        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("reserve a test TCP port");
        let port = listener.local_addr().expect("read reserved port").port();
        drop(listener);
        port
    }

    #[cfg(unix)]
    fn owner_uid(path: &std::path::Path) -> NonZeroU32 {
        use std::os::unix::fs::MetadataExt;

        NonZeroU32::new(
            std::fs::metadata(path)
                .expect("test directory metadata")
                .uid(),
        )
        .expect("test process must not use uid zero")
    }

    #[cfg(unix)]
    fn uds_config(socket_path: std::path::PathBuf, owner_uid: NonZeroU32) -> HttpRuntimeConfig {
        HttpRuntimeConfig::new(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), available_tcp_port()),
            Some(UnixSocketConfig::new(
                socket_path,
                owner_uid,
                NonZeroU32::new(0o600).expect("owner-only socket mode"),
            )),
            limits(1024, 8),
            timeouts(Duration::from_secs(1), Duration::from_secs(1)),
        )
    }

    fn config(port: u16) -> HttpRuntimeConfig {
        HttpRuntimeConfig::new(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            None,
            limits(1024, 8),
            timeouts(Duration::from_secs(1), Duration::from_secs(1)),
        )
    }

    fn limits(max_body_bytes: usize, max_connections: usize) -> RuntimeLimits {
        RuntimeLimits::new(
            NonZeroUsize::new(max_body_bytes).expect("test limit is non-zero"),
            NonZeroUsize::new(max_connections).expect("test limit is non-zero"),
        )
    }

    fn timeouts(request: Duration, shutdown: Duration) -> RuntimeTimeouts {
        RuntimeTimeouts::new(
            NonZeroDuration::new(request).expect("test timeout is non-zero"),
            NonZeroDuration::new(shutdown).expect("test timeout is non-zero"),
        )
    }

    #[test]
    fn invalid_configuration_fails_before_lifecycle_start() {
        let error = match HttpRuntimeBuilder::new(config(0), Arc::new(TestRouter)).build() {
            Ok(_) => panic!("invalid bind configuration must fail before any listener exists"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert!(error.message().contains("bind_address"));
        assert!(
            error
                .message()
                .contains("Repair the active ATM configuration and retry.")
        );
        assert!(!error.message().contains("reinstall/restart daemon"));
        assert_eq!(
            error.cause(),
            Some(
                "port 0 cannot be published; Unix socket configuration is additive and cannot replace TCP"
            )
        );
    }

    #[test]
    fn runtime_config_leaves_cannot_represent_zero_values() {
        assert!(NonZeroUsize::new(0).is_none());
        assert!(NonZeroU32::new(0).is_none());
        assert!(NonZeroDuration::new(Duration::ZERO).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn unix_socket_preflight_rejects_invalid_material() {
        use std::path::PathBuf;

        let invalid_uds = HttpRuntimeConfig::new(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4242),
            Some(UnixSocketConfig::new(
                PathBuf::new(),
                NonZeroU32::new(1).expect("test uid is non-zero"),
                NonZeroU32::new(0o1000).expect("test mode is non-zero"),
            )),
            limits(1, 1),
            timeouts(Duration::from_secs(1), Duration::from_secs(1)),
        );
        let error = match HttpRuntimeBuilder::new(invalid_uds, Arc::new(TestRouter)).build() {
            Ok(_) => panic!("invalid UDS configuration must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert!(error.message().contains("unix_socket.path"), "{error:?}");
    }

    #[cfg(unix)]
    #[test]
    fn unix_socket_is_additive_and_cannot_replace_tcp_bind() {
        use tempfile::tempdir;

        let temporary_directory = tempdir().expect("temporary directory");
        let unix_socket = UnixSocketConfig::new(
            temporary_directory
                .path()
                .join("atm-http-runtime-test.sock"),
            NonZeroU32::new(1).expect("test uid is non-zero"),
            NonZeroU32::new(0o600).expect("test mode is non-zero"),
        );
        HttpRuntimeBuilder::new(
            HttpRuntimeConfig::new(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4242),
                Some(unix_socket.clone()),
                limits(1, 1),
                timeouts(Duration::from_secs(1), Duration::from_secs(1)),
            ),
            Arc::new(TestRouter),
        )
        .build()
        .expect("TCP and UDS configuration is valid together");

        let error = match HttpRuntimeBuilder::new(
            HttpRuntimeConfig::new(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                Some(unix_socket),
                limits(1, 1),
                timeouts(Duration::from_secs(1), Duration::from_secs(1)),
            ),
            Arc::new(TestRouter),
        )
        .build()
        {
            Ok(_) => panic!("UDS cannot replace a required TCP bind"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert_eq!(
            error.cause(),
            Some(
                "port 0 cannot be published; Unix socket configuration is additive and cannot replace TCP"
            )
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn unix_socket_uses_the_shared_client_router_and_owner_only_endpoint() {
        use std::os::unix::fs::MetadataExt;

        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let socket_path = temporary_directory.path().join("atm-http-runtime.sock");
        let configured = HttpRuntimeBuilder::new(
            uds_config(socket_path.clone(), owner_uid(temporary_directory.path())),
            Arc::new(CanonicalUdsRouter),
        )
        .build()
        .expect("valid UDS configuration");
        let running = configured.start().await.expect("UDS runtime starts");

        let metadata = std::fs::metadata(&socket_path).expect("bound UDS metadata");
        assert_eq!(metadata.uid(), owner_uid(temporary_directory.path()).get());
        assert_eq!(metadata.mode() & 0o777, 0o600);

        let client = super::unix_socket_client(&socket_path, Duration::from_secs(1))
            .expect("shared Unix client");
        let response = client
            .execute(ApiRequest::new(write_request()))
            .await
            .expect("canonical error remains a typed API response");
        assert!(matches!(
            response.into_inner(),
            ResponseEnvelope::Error(error) if error.message().contains("canonical UDS test handler reached")
        ));

        running
            .begin_shutdown()
            .finish()
            .await
            .expect("UDS request drains with the runtime");
        assert!(
            !socket_path.exists(),
            "the runtime removes only its own Unix socket during shutdown"
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn unix_socket_owner_mismatch_fails_closed_without_leaving_an_endpoint() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let socket_path = temporary_directory.path().join("atm-http-runtime.sock");
        let actual_owner = owner_uid(temporary_directory.path()).get();
        let configured_owner = if actual_owner == 1 { 2 } else { 1 };
        let configured = HttpRuntimeBuilder::new(
            uds_config(
                socket_path.clone(),
                NonZeroU32::new(configured_owner).expect("non-zero mismatched uid"),
            ),
            Arc::new(CanonicalUdsRouter),
        )
        .build()
        .expect("configuration shape is valid before bind ownership check");

        let error = match configured.start().await {
            Ok(_) => panic!("runtime must reject a bound socket owned by another uid"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert!(error.message().contains("socket owner"));
        assert!(
            !socket_path.exists(),
            "failed UDS startup must not leave a reachable endpoint"
        );
    }

    #[cfg(not(unix))]
    #[test]
    fn unix_socket_configuration_is_rejected_on_non_unix_targets() {
        use std::path::PathBuf;

        let error = match HttpRuntimeBuilder::new(
            HttpRuntimeConfig::new(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4242),
                Some(UnixSocketConfig::new(
                    PathBuf::from("atm-http-runtime-test.sock"),
                    NonZeroU32::new(1).expect("test uid is non-zero"),
                    NonZeroU32::new(0o600).expect("test mode is non-zero"),
                )),
                limits(1, 1),
                timeouts(Duration::from_secs(1), Duration::from_secs(1)),
            ),
            Arc::new(TestRouter),
        )
        .build()
        {
            Ok(_) => panic!("non-Unix targets cannot configure a Unix socket"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert_eq!(
            error.cause(),
            Some("Unix-domain socket configuration is unsupported on this platform")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lifecycle_is_consuming_and_requires_validated_configuration() {
        let configured = HttpRuntimeBuilder::new(config(4242), Arc::new(TestRouter))
            .build()
            .expect("valid configuration");
        let running = configured.start().await.expect("AL.1 start transition");
        let draining = running.begin_shutdown();
        let _stopped = draining.finish().await.expect("runtime must drain");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_binds_serves_and_joins_the_axum_task() {
        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("reserve a test port");
        let port = listener.local_addr().expect("read test port").port();
        drop(listener);

        let configured = HttpRuntimeBuilder::new(config(port), Arc::new(TestRouter))
            .build()
            .expect("valid configuration");
        let running = configured.start().await.expect("replacement server starts");
        let response = reqwest::Client::new()
            .get(format!(
                "http://{}/v1/atm/messages",
                running.local_address()
            ))
            .send()
            .await
            .expect("replacement server responds");
        assert_eq!(response.status(), reqwest::StatusCode::METHOD_NOT_ALLOWED);
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("replacement server joins after shutdown");
    }
}
