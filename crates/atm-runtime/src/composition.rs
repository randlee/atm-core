use std::fmt;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

use atm_core::boundary::{self, ConfigDoctor, ConfigDoctorReport, NonClaudeOutbound};
use atm_core::doctor::RuntimeDoctorPorts;
use atm_core::error::AtmError;
use atm_core::home::HostRuntimeScope;
use atm_core::{LocalServiceRuntime, load_atm_config};
use atm_storage::{
    MessageStore as SharedMessageStore, PeerConfigStore, RosterStore as SharedRosterStore,
    StorageFactory,
};

use crate::legacy_storage_adapters::{
    StorageBackends, boundary_mail_store_view, boundary_roster_store_view, runtime_doctor_ports,
};

#[derive(Clone)]
pub struct RuntimeAssemblyInputs {
    pub host_runtime_scope: HostRuntimeScope,
    pub storage_factory: Arc<dyn StorageFactory>,
    pub config_current_dir: PathBuf,
    pub non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
}

impl fmt::Debug for RuntimeAssemblyInputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeAssemblyInputs")
            .field("host_runtime_scope", &self.host_runtime_scope)
            .field("storage_factory", &"dyn StorageFactory")
            .field("config_current_dir", &self.config_current_dir)
            .field("non_claude_outbound", &"dyn NonClaudeOutbound")
            .finish()
    }
}

#[derive(Clone)]
pub struct RuntimeAssembly {
    pub service_runtime: LocalServiceRuntime,
    pub(crate) storage_backends: StorageBackends<
        Arc<dyn SharedMessageStore + Send + Sync>,
        Arc<dyn SharedRosterStore + Send + Sync>,
    >,
    pub nudge_template_override_store: Arc<dyn boundary::NudgeTemplateOverrideStore + Send + Sync>,
    pub peer_config_store: Arc<dyn PeerConfigStore + Send + Sync>,
    pub doctor_ports: RuntimeDoctorPorts,
}

impl fmt::Debug for RuntimeAssembly {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeAssembly")
            .field("service_runtime", &self.service_runtime)
            .field("storage_backends", &self.storage_backends)
            .field(
                "nudge_template_override_store",
                &"dyn NudgeTemplateOverrideStore",
            )
            .field("peer_config_store", &"dyn PeerConfigStore")
            .field("doctor_ports", &self.doctor_ports)
            .finish()
    }
}

#[derive(Debug, Default)]
struct RuntimeConfigDoctor {
    config_current_dir: PathBuf,
}

impl boundary::sealed::Sealed for RuntimeConfigDoctor {}

impl ConfigDoctor for RuntimeConfigDoctor {
    fn inspect_config(&self) -> Result<ConfigDoctorReport, AtmError> {
        let _ = load_atm_config(&self.config_current_dir)?;
        Ok(ConfigDoctorReport {
            findings: Vec::new(),
        })
    }
}

pub fn assemble_runtime(inputs: RuntimeAssemblyInputs) -> Result<RuntimeAssembly, AtmError> {
    let storage = inputs
        .storage_factory
        .open(inputs.host_runtime_scope.durable_state_root.as_ref())?;
    let storage_backends = StorageBackends {
        messages: storage.message_store(),
        rosters: storage.roster_store(),
    };
    let nudge_template_override_store = storage.nudge_template_override_store();
    let peer_config_store = storage.peer_config_store();
    validate_enabled_peer_configuration(peer_config_store.as_ref())?;
    let service_runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        storage_backends.messages.clone(),
        storage_backends.rosters.clone(),
        Arc::clone(&nudge_template_override_store),
        inputs.non_claude_outbound,
    );
    let doctor_ports = runtime_doctor_ports(Arc::new(RuntimeConfigDoctor {
        config_current_dir: inputs.config_current_dir,
    }));
    Ok(RuntimeAssembly {
        service_runtime,
        storage_backends,
        nudge_template_override_store,
        peer_config_store,
        doctor_ports,
    })
}

/// Validate all enabled HTTPS configuration before the daemon publishes any
/// HTTPS service. AI.8 performs a bind preflight only; AI.9 owns the actual
/// listener lifetime and request handling.
fn validate_enabled_peer_configuration(
    store: &(dyn PeerConfigStore + Send + Sync),
) -> Result<(), AtmError> {
    for peer in store.list_trusted_peers()? {
        if peer.enabled && peer.fingerprint.trim().is_empty() {
            return Err(AtmError::validation(
                "enabled trusted peers require a non-empty pinned fingerprint",
            ));
        }
    }
    let enabled = store
        .list_interfaces()?
        .into_iter()
        .filter(|interface| interface.enabled)
        .collect::<Vec<_>>();
    if enabled.is_empty() {
        return Ok(());
    }
    let certificate = store.local_certificate()?.ok_or_else(|| {
        AtmError::validation(
            "enabled HTTPS interfaces require a configured local certificate reference",
        )
    })?;
    if certificate.fingerprint.trim().is_empty() || certificate.private_key_ref.trim().is_empty() {
        return Err(AtmError::validation(
            "enabled HTTPS interfaces require a non-empty certificate fingerprint and key reference",
        ));
    }
    for interface in enabled {
        TcpListener::bind(interface.bind_addr).map_err(|error| {
            AtmError::daemon_unavailable(format!(
                "HTTPS bind preflight failed for {}: {error}",
                interface.bind_addr
            ))
        })?;
    }
    Ok(())
}

impl RuntimeAssembly {
    pub fn message_store_arc(&self) -> Arc<dyn SharedMessageStore + Send + Sync> {
        self.storage_backends.messages.clone()
    }

    pub fn mail_store_arc(&self) -> Arc<dyn boundary::MailStore + Send + Sync> {
        boundary_mail_store_view(self.storage_backends.messages.clone())
    }

    pub fn roster_store_arc(&self) -> Arc<dyn boundary::RosterStore + Send + Sync> {
        boundary_roster_store_view(self.storage_backends.rosters.clone())
    }

    pub fn shared_roster_store_arc(&self) -> Arc<dyn SharedRosterStore + Send + Sync> {
        self.storage_backends.rosters.clone()
    }

    pub fn peer_config_store(&self) -> Arc<dyn PeerConfigStore + Send + Sync> {
        Arc::clone(&self.peer_config_store)
    }
}

/// Invoke the retained roster boundary through the runtime selected by
/// atm-core. This preserves fixture-scoped runtime installation in tests.
pub fn with_installed_roster_store<T>(
    f: impl FnOnce(&(dyn boundary::RosterStore + Send + Sync)) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    atm_core::with_default_local_service_runtime(|runtime| {
        let roster_store = boundary_roster_store_view(runtime.shared_roster_store_arc());
        f(roster_store.as_ref())
    })
}
