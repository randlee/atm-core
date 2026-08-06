//! Tokio HTTP runtime composition contract for ATM.
//!
//! This crate owns no listener or route implementation in AL.1.  It provides
//! the typed, consuming lifecycle and validates runtime-owned configuration
//! before a later adapter is allowed to bind or publish an endpoint.  See the
//! [Phase AL/AM runtime boundary checklist](../../../docs/plans/phase-al-am-runtime-boundary-checklist.md).
//!
//! The only ATM dependency is `atm-core`, specifically its existing sealed
//! [`atm_core::ApiRouter`] boundary and canonical [`atm_core::AtmError`].
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

use std::marker::PhantomData;
use std::net::SocketAddr;
use std::num::{NonZeroU32, NonZeroUsize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use atm_core::ApiRouter;
use atm_core::error::AtmError;

mod client;
mod message_handler;

pub use message_handler::{AuthenticatedConnector, canonical_message_router};

/// Validated configuration for the future maintained Tokio HTTP runtime.
///
/// The fields remain private so composition cannot bypass validation before a
/// listener is introduced in a later AL sprint.
#[derive(Debug, Clone)]
pub struct HttpRuntimeConfig {
    bind_address: SocketAddr,
    unix_socket: Option<UnixSocketConfig>,
    limits: RuntimeLimits,
    timeouts: RuntimeTimeouts,
    tls: Option<TlsMaterial>,
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
        tls: Option<TlsMaterial>,
    ) -> Self {
        Self {
            bind_address,
            unix_socket,
            limits,
            timeouts,
            tls,
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
    #[expect(
        dead_code,
        reason = "AL.5 owns Unix socket ownership application; AL.1 retains its validated configuration input"
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

/// File-backed TLS configuration. Parsing/trust construction is deferred to
/// the TLS adapter, but required material is checked before a listener exists.
#[derive(Debug, Clone)]
pub struct TlsMaterial {
    identity_certificate: PathBuf,
    identity_private_key: PathBuf,
    trust_store: PathBuf,
}

impl TlsMaterial {
    #[must_use]
    pub fn new(
        identity_certificate: PathBuf,
        identity_private_key: PathBuf,
        trust_store: PathBuf,
    ) -> Self {
        Self {
            identity_certificate,
            identity_private_key,
            trust_store,
        }
    }
}

/// Composition input that uses the existing sealed application boundary.
#[derive(Clone)]
pub struct HttpRuntimeBuilder {
    config: HttpRuntimeConfig,
    router: Arc<dyn ApiRouter>,
}

impl HttpRuntimeBuilder {
    #[must_use]
    pub fn new(config: HttpRuntimeConfig, router: Arc<dyn ApiRouter>) -> Self {
        Self { config, router }
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
            router: self.router,
            _state: PhantomData,
        })
    }
}

/// Validated but not started runtime state.
pub struct Configured;
/// Runtime lifecycle state after its future listener/start adapter begins.
pub struct Running;
/// Runtime lifecycle state while the future listener drains.
pub struct Draining;
/// Terminal lifecycle state with no live runtime-owned handles.
pub struct Stopped;

/// Non-cloneable lifecycle owner. State transitions consume this value.
pub struct HttpRuntime<State> {
    config: HttpRuntimeConfig,
    router: Arc<dyn ApiRouter>,
    _state: PhantomData<State>,
}

impl HttpRuntime<Configured> {
    /// Transitions the validated runtime to `Running`.
    ///
    /// AL.1 intentionally owns no listener or endpoint publisher; AL.2 adds
    /// the canonical handler and later adapter sprints provide the binding
    /// implementation. This transition is still consuming so consumers cannot
    /// express double-start or publish-before-running.
    pub async fn start(self) -> Result<HttpRuntime<Running>, AtmError> {
        Ok(self.into_state())
    }
}

impl HttpRuntime<Running> {
    /// Consumes the only running owner and begins the drain transition.
    #[must_use]
    pub fn begin_shutdown(self) -> HttpRuntime<Draining> {
        self.into_state()
    }
}

impl HttpRuntime<Draining> {
    /// Completes the drain transition.
    ///
    /// AL.1 owns no listener, connection task, or cancellation handle, so there
    /// is no work to time-bound here. The validated shutdown duration is
    /// reserved for the adapter that first owns drainable runtime work; that
    /// adapter must apply it to its actual drain operation rather than inventing
    /// a delay in this lifecycle-only transition.
    pub async fn finish(self) -> HttpRuntime<Stopped> {
        self.into_state()
    }
}

impl<State> HttpRuntime<State> {
    fn into_state<Next>(self) -> HttpRuntime<Next> {
        HttpRuntime {
            config: self.config,
            router: self.router,
            _state: PhantomData,
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
    if let Some(tls) = &config.tls {
        validate_file("tls.identity_certificate", &tls.identity_certificate)?;
        validate_file("tls.identity_private_key", &tls.identity_private_key)?;
        validate_file("tls.trust_store", &tls.trust_store)?;
    }
    Ok(())
}

/// Performs synchronous filesystem inspection during pre-bind construction.
///
/// This function is intentionally limited to `HttpRuntimeBuilder::build`,
/// before any Tokio runtime request task or listener exists. Future request or
/// connection paths must not call it; asynchronous runtime I/O belongs in the
/// adapter and requires a non-blocking implementation.
fn validate_file(field: &str, path: &std::path::Path) -> Result<(), AtmError> {
    if !path.is_file() {
        return Err(preflight(
            field,
            format!("{} is not a readable regular file", path.display()),
        ));
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

    use atm_core::ApiRouter;
    use atm_core::api::{ApiRequest, ApiResponse, AuthenticatedIngress, RequestDeadline};
    use atm_core::boundary;
    use atm_core::error::AtmError;

    use super::{
        HttpRuntimeBuilder, HttpRuntimeConfig, NonZeroDuration, RuntimeLimits, RuntimeTimeouts,
        UnixSocketConfig,
    };

    struct TestRouter;

    impl boundary::sealed::Sealed for TestRouter {}

    impl ApiRouter for TestRouter {
        fn route(
            &self,
            _request: ApiRequest,
            _ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> Result<ApiResponse, AtmError> {
            Err(AtmError::validation(
                "test router is not invoked by the AL.1 lifecycle contract",
            ))
        }
    }

    fn config(port: u16) -> HttpRuntimeConfig {
        HttpRuntimeConfig::new(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            None,
            limits(1024, 8),
            timeouts(Duration::from_secs(1), Duration::from_secs(1)),
            None,
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
            None,
        );
        let error = match HttpRuntimeBuilder::new(invalid_uds, Arc::new(TestRouter)).build() {
            Ok(_) => panic!("invalid UDS configuration must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert!(error.message().contains("unix_socket.path"), "{error:?}");
    }

    #[test]
    fn tls_preflight_rejects_missing_material() {
        use std::path::PathBuf;

        use super::TlsMaterial;

        let missing_tls = HttpRuntimeConfig::new(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4242),
            None,
            limits(1, 1),
            timeouts(Duration::from_secs(1), Duration::from_secs(1)),
            Some(TlsMaterial::new(
                PathBuf::from("/definitely-missing-atm-cert.pem"),
                PathBuf::from("/definitely-missing-atm-key.pem"),
                PathBuf::from("/definitely-missing-atm-trust.pem"),
            )),
        );
        let error = match HttpRuntimeBuilder::new(missing_tls, Arc::new(TestRouter)).build() {
            Ok(_) => panic!("missing TLS material must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert!(
            error.message().contains("tls.identity_certificate"),
            "{error:?}"
        );
        assert!(error.cause().is_some(), "{error:?}");
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
                None,
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
                None,
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
                None,
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
        let _stopped = draining.finish().await;
    }
}
