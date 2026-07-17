use std::net::SocketAddr;

use atm_core::error::AtmError;
use atm_storage::{IsoTimestamp, PeerInterfaceBindingUpdate, PeerInterfaceKey};
use std::collections::BTreeMap;

use super::{ListenerRow, RuntimeComposition};

impl RuntimeComposition {
    pub(super) fn refresh_peer_listeners(&self) -> Result<(), AtmError> {
        let rows = enabled_listener_rows(self)?;
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
