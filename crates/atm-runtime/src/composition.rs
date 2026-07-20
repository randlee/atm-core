use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use atm_core::boundary::{self, ConfigDoctor, ConfigDoctorReport, NonClaudeOutbound};
use atm_core::doctor::RuntimeDoctorPorts;
use atm_core::error::AtmError;
use atm_core::home::HostRuntimeScope;
use atm_core::{LocalServiceRuntime, load_atm_config};
use atm_storage::{
    MessageStore as SharedMessageStore, RosterStore as SharedRosterStore, StorageFactory,
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
        doctor_ports,
    })
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
