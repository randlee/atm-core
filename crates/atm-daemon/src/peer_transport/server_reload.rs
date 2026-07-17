use super::*;

pub(super) fn update_configured_listen_addrs(
    transport: &PeerServerTransport,
    listen_addrs: &[SocketAddr],
) -> Result<(), AtmError> {
    let mut configured = transport.listen_addrs.lock().map_err(|_| {
        AtmError::daemon_unavailable("peer listener config lock poisoned")
            .with_recovery("Restart atm-daemon before retrying cross-host peer listener reload.")
    })?;
    *configured = listen_addrs.to_vec();
    Ok(())
}

pub(super) fn drop_stale_listener_handles(
    state: &mut BTreeMap<SocketAddr, PeerServerHandle>,
    desired: &std::collections::BTreeSet<SocketAddr>,
) -> Result<(), AtmError> {
    let stale_addrs = state
        .keys()
        .copied()
        .filter(|addr| !desired.contains(addr))
        .collect::<Vec<_>>();
    for stale_addr in stale_addrs {
        if let Some(handle) = state.remove(&stale_addr) {
            shutdown_listener_handle(handle)?;
        }
    }
    Ok(())
}

pub(super) fn reload_requested_listeners(
    transport: &PeerServerTransport,
    state: &mut BTreeMap<SocketAddr, PeerServerHandle>,
    listen_addrs: Vec<SocketAddr>,
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
) -> Result<(Vec<PeerListenerOutcome>, Vec<String>), AtmError> {
    let mut outcomes = Vec::new();
    let mut failures = Vec::new();
    for listen_addr in listen_addrs {
        if let Some(handle) = state.get(&listen_addr) {
            outcomes.push(PeerListenerOutcome {
                listen_addr,
                bound_addr: Some(handle.bound_addr),
                error_message: None,
            });
            continue;
        }
        reload_single_listener(
            transport,
            state,
            dispatcher.clone(),
            listen_addr,
            &mut outcomes,
            &mut failures,
        );
    }
    Ok((outcomes, failures))
}

fn reload_single_listener(
    transport: &PeerServerTransport,
    state: &mut BTreeMap<SocketAddr, PeerServerHandle>,
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    listen_addr: SocketAddr,
    outcomes: &mut Vec<PeerListenerOutcome>,
    failures: &mut Vec<String>,
) {
    match transport.bind_and_spawn_listener(listen_addr, dispatcher) {
        Ok((bound_addr, handle)) => {
            transport.observability.emit_or_warn(
                "peer_listener_start",
                "ok",
                format!("daemon peer listener bound at {bound_addr}"),
            );
            state.insert(listen_addr, handle);
            outcomes.push(PeerListenerOutcome {
                listen_addr,
                bound_addr: Some(bound_addr),
                error_message: None,
            });
        }
        Err(error) => {
            tracing::warn!(
                subsystem = "peer_transport",
                action = "reload_listener",
                outcome = "degraded",
                %listen_addr,
                %error,
                "daemon peer listener reload failed to rebind the configured address"
            );
            transport.observability.emit_or_warn(
                "peer_listener_reload",
                "degraded",
                format!("daemon peer listener reload failed for {listen_addr}"),
            );
            transport.record_degraded(listen_addr, &error);
            failures.push(format!("{listen_addr}: {}", error.message));
            outcomes.push(PeerListenerOutcome {
                listen_addr,
                bound_addr: None,
                error_message: Some(error.message.clone()),
            });
        }
    }
}
