use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use atm_core::boundary::{
    self, ConfigDoctor, ConfigDoctorReport, NonClaudeOutbound, RuntimeStorageFinalizer,
};
use atm_core::doctor::RuntimeDoctorPorts;
use atm_core::error::AtmError;
use atm_core::home::host_mail_db_path;
use atm_core::{LocalFileNonClaudeOutbound, LocalServiceRuntime, load_atm_config};
use atm_storage::{MessageStore as SharedMessageStore, RosterStore as SharedRosterStore};
use atm_storage_rusqlite::SqliteStorageBackend;

use crate::legacy_storage_adapters::{
    StorageBackends, boundary_mail_store_view, boundary_roster_store_view, runtime_doctor_ports,
};
use crate::replay_store::{SqliteRemoteReplayStore, SqliteRuntimeStorageFinalizer};
use crate::sqlite_observability::{RuntimeSqliteObservability, RuntimeSqliteObserver};

#[derive(Clone)]
pub struct RuntimeAssemblyInputs {
    pub sqlite_db_path: PathBuf,
    pub config_current_dir: PathBuf,
    pub sqlite_observer: Arc<dyn RuntimeSqliteObserver>,
    pub non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
}

impl fmt::Debug for RuntimeAssemblyInputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeAssemblyInputs")
            .field("sqlite_db_path", &self.sqlite_db_path)
            .field("config_current_dir", &self.config_current_dir)
            .field("sqlite_observer", &"dyn RuntimeSqliteObserver")
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
    pub remote_replay_store: Arc<dyn boundary::RemoteReplayStore + Send + Sync>,
    pub storage_finalizer: Arc<dyn RuntimeStorageFinalizer + Send + Sync>,
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
            .field("remote_replay_store", &"dyn RemoteReplayStore")
            .field("storage_finalizer", &"dyn RuntimeStorageFinalizer")
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

pub fn assemble_sqlite_runtime(inputs: RuntimeAssemblyInputs) -> Result<RuntimeAssembly, AtmError> {
    assemble_sqlite_runtime_at_path(
        &inputs.sqlite_db_path,
        inputs.config_current_dir.clone(),
        Arc::clone(&inputs.sqlite_observer),
        Arc::clone(&inputs.non_claude_outbound),
    )
}

fn assemble_sqlite_runtime_at_path(
    sqlite_db_path: &Path,
    config_current_dir: PathBuf,
    sqlite_observer: Arc<dyn RuntimeSqliteObserver>,
    non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
) -> Result<RuntimeAssembly, AtmError> {
    let sqlite_observability = Arc::new(RuntimeSqliteObservability::new(sqlite_observer));
    let sqlite_backend = Arc::new(SqliteStorageBackend::new_with_observability(
        sqlite_db_path,
        sqlite_observability,
    )?);
    let shared_messages = sqlite_backend.message_store();
    let shared_rosters = sqlite_backend.roster_store();
    let storage_backends = StorageBackends {
        messages: shared_messages.clone(),
        rosters: shared_rosters.clone(),
    };
    let service_runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        storage_backends.messages.clone(),
        storage_backends.rosters.clone(),
        sqlite_backend.nudge_template_override_store(),
        non_claude_outbound,
    );
    let doctor_ports = runtime_doctor_ports(Arc::new(RuntimeConfigDoctor { config_current_dir }));
    let remote_replay_store: Arc<dyn boundary::RemoteReplayStore + Send + Sync> =
        Arc::new(SqliteRemoteReplayStore::new(Arc::clone(&sqlite_backend)));
    let storage_finalizer: Arc<dyn RuntimeStorageFinalizer + Send + Sync> = Arc::new(
        SqliteRuntimeStorageFinalizer::new(Arc::clone(&sqlite_backend)),
    );
    Ok(RuntimeAssembly {
        service_runtime,
        storage_backends,
        nudge_template_override_store: sqlite_backend.nudge_template_override_store(),
        doctor_ports,
        remote_replay_store,
        storage_finalizer,
    })
}

pub fn assemble_default_runtime() -> Result<RuntimeAssembly, AtmError> {
    let config_current_dir = std::env::current_dir().map_err(|source| {
        AtmError::config("failed to resolve current directory for direct runtime assembly")
            .with_recovery(
                "Run the direct retained runtime path from a readable ATM workspace so config inspection and runtime assembly share one validated root.",
            )
            .with_source(source)
    })?;
    let sqlite_backend = Arc::new(SqliteStorageBackend::new_with_observability(
        host_mail_db_path()?,
        RuntimeSqliteObservability::disabled(),
    )?);
    let shared_messages = sqlite_backend.message_store();
    let shared_rosters = sqlite_backend.roster_store();
    let storage_backends = StorageBackends {
        messages: shared_messages.clone(),
        rosters: shared_rosters.clone(),
    };
    let service_runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        storage_backends.messages.clone(),
        storage_backends.rosters.clone(),
        sqlite_backend.nudge_template_override_store(),
        Arc::new(LocalFileNonClaudeOutbound::new()),
    );
    let doctor_ports = runtime_doctor_ports(Arc::new(RuntimeConfigDoctor { config_current_dir }));
    let remote_replay_store: Arc<dyn boundary::RemoteReplayStore + Send + Sync> =
        Arc::new(SqliteRemoteReplayStore::new(Arc::clone(&sqlite_backend)));
    let storage_finalizer: Arc<dyn RuntimeStorageFinalizer + Send + Sync> = Arc::new(
        SqliteRuntimeStorageFinalizer::new(Arc::clone(&sqlite_backend)),
    );
    Ok(RuntimeAssembly {
        service_runtime,
        storage_backends,
        nudge_template_override_store: sqlite_backend.nudge_template_override_store(),
        doctor_ports,
        remote_replay_store,
        storage_finalizer,
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

pub fn default_local_runtime() -> Result<LocalServiceRuntime, AtmError> {
    assemble_default_runtime().map(|assembly| assembly.service_runtime)
}

pub fn with_default_roster_store<T>(
    f: impl FnOnce(&(dyn boundary::RosterStore + Send + Sync)) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    let assembly = assemble_default_runtime()?;
    let roster_store = assembly.roster_store_arc();
    let result = f(roster_store.as_ref());
    let finalize_result = assembly.storage_finalizer.finalize_storage_shutdown();
    match (result, finalize_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(_)) => Err(error),
    }
}

pub fn with_default_nudge_template_override_store<T>(
    f: impl FnOnce(&(dyn boundary::NudgeTemplateOverrideStore + Send + Sync)) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    let sqlite_backend = Arc::new(SqliteStorageBackend::new_with_observability(
        host_mail_db_path()?,
        RuntimeSqliteObservability::disabled(),
    )?);
    let override_store = sqlite_backend.nudge_template_override_store();
    let result = f(override_store.as_ref());
    let finalize_result =
        SqliteRuntimeStorageFinalizer::new(sqlite_backend).finalize_storage_shutdown();
    match (result, finalize_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(_)) => Err(error),
    }
}
