use super::*;

pub(super) fn build_direct_peer_server(
    listener: Option<TcpListener>,
    config: &HttpRuntimeConfig,
    handler: Arc<dyn CanonicalWriteHandler>,
) -> Option<DirectPeerServer> {
    let listener = listener?;
    Some(match config.peer_stream_adapter.as_ref() {
        Some(adapter) => DirectPeerServer::Authenticated(
            listener,
            Arc::clone(adapter),
            handler,
            config.limits,
            config.timeouts,
        ),
        None => DirectPeerServer::Plaintext(
            listener,
            canonical_api_router(
                handler,
                AuthenticatedConnector::peer_socket(),
                config.limits,
                config.timeouts,
            ),
        ),
    })
}

pub(super) fn validate_config(config: &HttpRuntimeConfig) -> Result<(), AtmError> {
    debug_assert!(config.limits.max_body_bytes > 0);
    debug_assert!(config.limits.max_connections > 0);
    debug_assert!(!config.timeouts.request.is_zero());
    config.peer_pool.validate()?;
    debug_assert!(!config.timeouts.shutdown.is_zero());
    validate_loopback_config(&config.loopback_tcp)?;
    if let Some(peer) = &config.direct_peer_tcp
        && peer.port() == 0
        && !peer.allow_ephemeral_test_port
    {
        return Err(preflight(
            "direct_peer_tcp.port",
            "must use a non-zero port",
        ));
    }
    if config.peer_stream_adapter.is_some() && config.direct_peer_tcp.is_none() {
        return Err(preflight(
            "peer_stream_adapter",
            "requires an enabled direct peer listener",
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
        validate_unix_socket_path(&socket.path)?;
        if socket.mode.get() & !0o777 != 0 {
            return Err(preflight(
                "unix_socket.mode",
                "must contain only permission bits",
            ));
        }
        if socket.mode.get() & 0o077 != 0 {
            return Err(preflight(
                "unix_socket.mode",
                "must grant access only to the configured owner",
            ));
        }
        if socket.mode.get() & 0o200 == 0 {
            return Err(preflight(
                "unix_socket.mode",
                "must grant the configured owner write permission",
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn validate_unix_socket_path(path: &Path) -> Result<(), AtmError> {
    if path.as_os_str().is_empty() {
        return Err(preflight("unix_socket.path", "must not be empty"));
    }
    Ok(())
}

fn preflight(field: &str, cause: impl std::fmt::Display) -> AtmError {
    AtmError::config(format!("invalid runtime configuration field `{field}`")).with_cause(cause)
}
