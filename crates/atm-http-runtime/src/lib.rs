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
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use atm_core::ApiRouter;
use atm_core::error::AtmError;

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

/// UDS preflight input. It is configuration only; AL.1 never binds it.
#[derive(Debug, Clone)]
pub struct UnixSocketConfig {
    path: PathBuf,
    owner_uid: Option<u32>,
    mode: u32,
}

impl UnixSocketConfig {
    #[must_use]
    pub fn new(path: PathBuf, owner_uid: Option<u32>, mode: u32) -> Self {
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
    pub const fn new(max_body_bytes: usize, max_connections: usize) -> Self {
        Self {
            max_body_bytes,
            max_connections,
        }
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
    pub const fn new(request: Duration, shutdown: Duration) -> Self {
        Self { request, shutdown }
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
    /// Returns `AtmError::bind_preflight` with the invalid field and cause.
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
    /// Completes the drain transition. No listener exists in AL.1.
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
    if config.bind_address.port() == 0 {
        return Err(preflight("bind_address", "port 0 cannot be published"));
    }
    if config.limits.max_body_bytes == 0 {
        return Err(preflight(
            "limits.max_body_bytes",
            "must be greater than zero",
        ));
    }
    if config.limits.max_connections == 0 {
        return Err(preflight(
            "limits.max_connections",
            "must be greater than zero",
        ));
    }
    validate_timeout("timeouts.request", config.timeouts.request)?;
    validate_timeout("timeouts.shutdown", config.timeouts.shutdown)?;
    if let Some(socket) = &config.unix_socket {
        if socket.path.as_os_str().is_empty() {
            return Err(preflight("unix_socket.path", "must not be empty"));
        }
        if socket.mode & !0o777 != 0 {
            return Err(preflight(
                "unix_socket.mode",
                "must contain only permission bits",
            ));
        }
        if socket.owner_uid.is_none() {
            return Err(preflight("unix_socket.owner_uid", "must be configured"));
        }
    }
    if let Some(tls) = &config.tls {
        validate_file("tls.identity_certificate", &tls.identity_certificate)?;
        validate_file("tls.identity_private_key", &tls.identity_private_key)?;
        validate_file("tls.trust_store", &tls.trust_store)?;
    }
    Ok(())
}

fn validate_timeout(field: &str, value: Duration) -> Result<(), AtmError> {
    if value.is_zero() {
        return Err(preflight(field, "must be greater than zero"));
    }
    Ok(())
}

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
    AtmError::bind_preflight(format!("invalid runtime configuration field `{field}`"))
        .with_cause(cause)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::time::Duration;

    use atm_core::ApiRouter;
    use atm_core::api::{ApiRequest, ApiResponse, AuthenticatedIngress, RequestDeadline};
    use atm_core::boundary;
    use atm_core::error::AtmError;

    use super::{HttpRuntimeBuilder, HttpRuntimeConfig, RuntimeLimits, RuntimeTimeouts};

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
            RuntimeLimits::new(1024, 8),
            RuntimeTimeouts::new(Duration::from_secs(1), Duration::from_secs(1)),
            None,
        )
    }

    #[test]
    fn invalid_configuration_fails_before_lifecycle_start() {
        let error = match HttpRuntimeBuilder::new(config(0), Arc::new(TestRouter)).build() {
            Ok(_) => panic!("invalid bind configuration must fail before any listener exists"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_BIND_PREFLIGHT_FAILED");
        assert!(error.message().contains("bind_address"));
        assert_eq!(error.cause(), Some("port 0 cannot be published"));
    }

    #[test]
    fn every_runtime_preflight_field_has_a_typed_diagnostic() {
        let router = || Arc::new(TestRouter);
        let cases = [
            (
                "limits.max_body_bytes",
                HttpRuntimeConfig::new(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4242),
                    None,
                    RuntimeLimits::new(0, 8),
                    RuntimeTimeouts::new(Duration::from_secs(1), Duration::from_secs(1)),
                    None,
                ),
            ),
            (
                "limits.max_connections",
                HttpRuntimeConfig::new(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4242),
                    None,
                    RuntimeLimits::new(1, 0),
                    RuntimeTimeouts::new(Duration::from_secs(1), Duration::from_secs(1)),
                    None,
                ),
            ),
            (
                "timeouts.request",
                HttpRuntimeConfig::new(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4242),
                    None,
                    RuntimeLimits::new(1, 1),
                    RuntimeTimeouts::new(Duration::ZERO, Duration::from_secs(1)),
                    None,
                ),
            ),
            (
                "timeouts.shutdown",
                HttpRuntimeConfig::new(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4242),
                    None,
                    RuntimeLimits::new(1, 1),
                    RuntimeTimeouts::new(Duration::from_secs(1), Duration::ZERO),
                    None,
                ),
            ),
        ];

        for (field, config) in cases {
            let error = match HttpRuntimeBuilder::new(config, router()).build() {
                Ok(_) => panic!("invalid configuration must be rejected before bind"),
                Err(error) => error,
            };
            assert_eq!(error.code().as_str(), "ATM_BIND_PREFLIGHT_FAILED");
            assert!(error.message().contains(field), "{error:?}");
            assert!(error.cause().is_some(), "{error:?}");
        }
    }

    #[test]
    fn uds_and_tls_preflight_reject_missing_or_invalid_material() {
        use std::path::PathBuf;

        use super::{TlsMaterial, UnixSocketConfig};

        let invalid_uds = HttpRuntimeConfig::new(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4242),
            Some(UnixSocketConfig::new(PathBuf::new(), None, 0o1000)),
            RuntimeLimits::new(1, 1),
            RuntimeTimeouts::new(Duration::from_secs(1), Duration::from_secs(1)),
            None,
        );
        let error = match HttpRuntimeBuilder::new(invalid_uds, Arc::new(TestRouter)).build() {
            Ok(_) => panic!("invalid UDS configuration must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_BIND_PREFLIGHT_FAILED");
        assert!(error.message().contains("unix_socket.path"), "{error:?}");

        let missing_tls = HttpRuntimeConfig::new(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4242),
            None,
            RuntimeLimits::new(1, 1),
            RuntimeTimeouts::new(Duration::from_secs(1), Duration::from_secs(1)),
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
        assert_eq!(error.code().as_str(), "ATM_BIND_PREFLIGHT_FAILED");
        assert!(
            error.message().contains("tls.identity_certificate"),
            "{error:?}"
        );
        assert!(error.cause().is_some(), "{error:?}");
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
