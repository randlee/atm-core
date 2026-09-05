//! Listener binding and endpoint publication for the replacement runtime.

use super::*;

pub(crate) async fn bind_configured_direct_peer_listener(
    config: &HttpRuntimeConfig,
    _health: &RuntimeHealth,
) -> Result<Option<TcpListener>, AtmError> {
    let Some(peer) = config.direct_peer_tcp.as_ref() else {
        return Ok(None);
    };
    let bind_address = SocketAddr::from(([0, 0, 0, 0], peer.port()));
    match TcpListener::bind(bind_address).await {
        Ok(listener) => Ok(Some(listener)),
        Err(error) => {
            tracing::warn!(%bind_address, error = %error, "replacement direct peer listener is unavailable; continuing with local listeners");
            Ok(None)
        }
    }
}

pub(crate) async fn bind_loopback_listener(
    config: &HttpRuntimeConfig,
    health: &RuntimeHealth,
) -> Result<(TcpListener, SocketAddr), AtmError> {
    let listener = TcpListener::bind(config.loopback_tcp.bind_address)
        .await
        .map_err(|source| {
            let error = AtmError::daemon_unavailable(format!(
                "failed to bind replacement HTTP runtime at {}",
                config.loopback_tcp.bind_address
            ))
            .with_cause(source);
            health.mark_not_ready(error.to_string());
            error
        })?;
    let local_address = listener.local_addr().map_err(|source| {
        let error = AtmError::daemon_unavailable("failed to read replacement HTTP runtime address")
            .with_cause(source);
        health.mark_not_ready(error.to_string());
        error
    })?;
    if !local_address.ip().is_loopback() {
        let error = AtmError::local_http_endpoint_non_loopback(
            "replacement HTTP runtime bound a non-loopback TCP address",
        );
        health.mark_not_ready(error.to_string());
        return Err(error);
    }
    Ok((listener, local_address))
}

#[cfg(unix)]
pub(crate) async fn bind_configured_unix_listener(
    config: &HttpRuntimeConfig,
    health: &RuntimeHealth,
) -> Result<Option<(UnixListener, UnixSocketPathGuard)>, AtmError> {
    let Some(socket) = config.unix_socket.clone() else {
        return Ok(None);
    };
    let lock_socket = socket.clone();
    let startup_lock =
        match tokio::task::spawn_blocking(move || UnixSocketStartupLock::acquire(&lock_socket))
            .await
        {
            Ok(Ok(lock)) => lock,
            Ok(Err(error)) => {
                health.mark_not_ready(error.to_string());
                return Err(error);
            }
            Err(source) => {
                let error = AtmError::daemon_unavailable(
                    "replacement Unix HTTP socket lock task ended unexpectedly",
                )
                .with_cause(source);
                health.mark_not_ready(error.to_string());
                return Err(error);
            }
        };
    if let Err(error) = reclaim_stale_unix_socket(&socket).await {
        health.mark_not_ready(error.to_string());
        return Err(error);
    }
    let result = match tokio::task::spawn_blocking(move || bind_unix_listener(&socket)).await {
        Ok(Ok(listener)) => Ok(Some(listener)),
        Ok(Err(error)) => {
            health.mark_not_ready(error.to_string());
            Err(error)
        }
        Err(source) => {
            let error = AtmError::daemon_unavailable(
                "replacement Unix HTTP socket setup task ended unexpectedly",
            )
            .with_cause(source);
            health.mark_not_ready(error.to_string());
            Err(error)
        }
    };
    drop(startup_lock);
    result
}

pub(crate) async fn publish_loopback_endpoint(
    config: &HttpRuntimeConfig,
    local_address: SocketAddr,
    health: &RuntimeHealth,
) -> Result<(LocalCapability, LoopbackEndpointRecordGuard), AtmError> {
    let capability = LocalCapability::generate()
        .inspect_err(|error| health.mark_not_ready(error.to_string()))?;
    let record_config = config.loopback_tcp.clone();
    let record_capability = capability.clone();
    let publication = tokio::task::spawn_blocking(move || {
        publish_loopback_endpoint_record(&record_config, local_address, &record_capability)
    })
    .await
    .map_err(|source| {
        let error = AtmError::daemon_unavailable(
            "replacement loopback endpoint publication task ended unexpectedly",
        )
        .with_cause(source);
        health.mark_not_ready(error.to_string());
        error
    })?;
    let endpoint_record =
        publication.inspect_err(|error| health.mark_not_ready(error.to_string()))?;
    Ok((capability, endpoint_record))
}
