use std::collections::BTreeMap;
use std::net::SocketAddr;

use atm_core::error::AtmError;
use atm_storage::{IsoTimestamp, PeerInterfaceBindingUpdate, PeerInterfaceKey};

use super::{ListenerRow, RuntimeComposition, load_peer_transport_config};

impl RuntimeComposition {
    pub(super) fn refresh_peer_listeners(&self) -> Result<(), AtmError> {
        let peer_transport_config = load_peer_transport_config(
            self.config_current_dir.clone(),
            &self.config_ingress,
            &self.composition_observability,
        )?;
        let rows = enabled_listener_rows(self)?;
        let rows = if rows.is_empty() {
            legacy_listener_rows(self, peer_transport_config.peer_listen_addr)
        } else {
            rows
        };
        let outcomes = self.peer_transport_runtime.reload_listeners(
            rows.iter().map(|row| row.listen_addr).collect(),
            self.request_dispatcher(),
        )?;
        let outcome_map = outcomes
            .into_iter()
            .map(|outcome| (outcome.listen_addr, outcome))
            .collect::<BTreeMap<_, _>>();
        let now = IsoTimestamp::now();
        for row in rows {
            let Some(key) = row.key else {
                continue;
            };
            let Some(outcome) = outcome_map.get(&row.listen_addr) else {
                continue;
            };
            self.peer_interface_config_store.record_binding_update(
                &PeerInterfaceBindingUpdate {
                    key,
                    observed_at: if outcome.error_message.is_none() {
                        Some(now)
                    } else {
                        None
                    },
                    refresh_deadline_at: None,
                    stale_at: outcome.error_message.as_ref().map(|_| now),
                    last_bound_at: outcome.bound_addr.map(|_| now),
                    last_bind_error: outcome.error_message.clone(),
                },
            )?;
        }
        Ok(())
    }
}

fn enabled_listener_rows(composition: &RuntimeComposition) -> Result<Vec<ListenerRow>, AtmError> {
    composition
        .peer_interface_config_store
        .list_interfaces()?
        .into_iter()
        .filter(|row| row.enabled)
        .map(|row| {
            Ok(ListenerRow {
                key: Some(listener_key_from_row(
                    &row.interface_name,
                    row.bind_addr,
                    row.port,
                )?),
                listen_addr: SocketAddr::new(row.bind_addr, row.port),
            })
        })
        .collect()
}

fn legacy_listener_rows(
    composition: &RuntimeComposition,
    peer_listen_addr: Option<SocketAddr>,
) -> Vec<ListenerRow> {
    let detail = "legacy daemon peer_listen_addr fallback is deprecated; configure durable listener rows with `atm daemon interfaces add` instead";
    tracing::warn!(
        subsystem = "composition",
        action = "peer_listener_legacy_config_fallback",
        outcome = "deprecated",
        "{}",
        detail
    );
    composition.composition_observability.emit_or_warn(
        "peer_listener_legacy_config_fallback",
        "deprecated",
        detail,
    );
    peer_listen_addr
        .into_iter()
        .map(|listen_addr| ListenerRow {
            key: None,
            listen_addr,
        })
        .collect()
}

fn listener_key_from_row(
    interface_name: &str,
    bind_addr: std::net::IpAddr,
    port: u16,
) -> Result<PeerInterfaceKey, AtmError> {
    PeerInterfaceKey::new(interface_name.to_string(), bind_addr, port).map_err(|error| {
        AtmError::daemon_unavailable(format!(
            "persisted daemon peer interface row `{interface_name}` at {bind_addr}:{port} is invalid for listener reload: {error}"
        ))
        .with_recovery(
            "Rewrite the invalid daemon interface row with `atm daemon interfaces remove ...` followed by `atm daemon interfaces add ...`, then retry the reload.",
        )
    })
}
