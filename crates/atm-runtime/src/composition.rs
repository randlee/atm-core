use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use atm_core::boundary::{
    self, ConfigDoctor, ConfigDoctorReport, NonClaudeOutbound, NotificationSink,
    RuntimeStorageFinalizer,
};
use atm_core::doctor::RuntimeDoctorPorts;
use atm_core::error::AtmError;
use atm_core::{
    LocalFileNonClaudeOutbound, LocalFileNotificationSink, LocalServiceRuntime,
    home::host_runtime_dir, load_atm_config,
};
use atm_storage::{MessageStore as SharedMessageStore, RosterStore as SharedRosterStore};
use atm_storage_rusqlite::SqliteStorageBackend;

use crate::legacy_storage_adapters::{
    StorageBackends, legacy_mail_store, legacy_roster_store, noop_task_store, runtime_doctor_ports,
};
use crate::replay_store::{SqliteRemoteReplayStore, SqliteRuntimeStorageFinalizer};
use crate::sqlite_observability::{RuntimeSqliteObservability, RuntimeSqliteObserver};

#[derive(Clone)]
pub struct RuntimeAssemblyInputs {
    pub sqlite_db_path: PathBuf,
    pub config_current_dir: PathBuf,
    pub sqlite_observer: Arc<dyn RuntimeSqliteObserver>,
    pub non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
    pub notification_sink: Arc<dyn NotificationSink + Send + Sync>,
}

impl fmt::Debug for RuntimeAssemblyInputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeAssemblyInputs")
            .field("sqlite_db_path", &self.sqlite_db_path)
            .field("config_current_dir", &self.config_current_dir)
            .field("sqlite_observer", &"dyn RuntimeSqliteObserver")
            .field("non_claude_outbound", &"dyn NonClaudeOutbound")
            .field("notification_sink", &"dyn NotificationSink")
            .finish()
    }
}

#[derive(Clone)]
pub struct RuntimeAssembly {
    pub service_runtime: LocalServiceRuntime,
    pub(crate) storage_backends:
        StorageBackends<Arc<dyn SharedMessageStore + Send + Sync>, Arc<dyn SharedRosterStore + Send + Sync>>,
    pub mail_store: Arc<dyn boundary::MailStore + Send + Sync>,
    pub roster_store: Arc<dyn boundary::RosterStore + Send + Sync>,
    pub task_store: Arc<dyn boundary::TaskStore + Send + Sync>,
    pub doctor_ports: RuntimeDoctorPorts,
    pub remote_replay_store: Arc<dyn boundary::RemoteReplayStore + Send + Sync>,
    pub storage_finalizer: Arc<dyn RuntimeStorageFinalizer + Send + Sync>,
}

impl fmt::Debug for RuntimeAssembly {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeAssembly")
            .field("service_runtime", &self.service_runtime)
            .field("storage_backends", &self.storage_backends)
            .field("mail_store", &"dyn MailStore")
            .field("roster_store", &"dyn RosterStore")
            .field("task_store", &"dyn TaskStore")
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
        Arc::clone(&inputs.notification_sink),
    )
}

fn assemble_sqlite_runtime_at_path(
    sqlite_db_path: &Path,
    config_current_dir: PathBuf,
    _sqlite_observer: Arc<dyn RuntimeSqliteObserver>,
    non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
    notification_sink: Arc<dyn NotificationSink + Send + Sync>,
) -> Result<RuntimeAssembly, AtmError> {
    let _sqlite_observability = Arc::new(RuntimeSqliteObservability::new(_sqlite_observer));
    let sqlite_backend = Arc::new(SqliteStorageBackend::new(sqlite_db_path)?);
    let shared_messages = sqlite_backend.message_store();
    let shared_rosters = sqlite_backend.roster_store();
    let storage_backends = StorageBackends {
        messages: shared_messages.clone(),
        rosters: shared_rosters.clone(),
    };
    let mail_store = legacy_mail_store(shared_messages);
    let roster_store = legacy_roster_store(shared_rosters);
    let task_store = noop_task_store();
    let service_runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        mail_store.clone(),
        task_store.clone(),
        roster_store.clone(),
        non_claude_outbound,
        notification_sink,
    );
    let doctor_ports = runtime_doctor_ports(Arc::new(RuntimeConfigDoctor {
        config_current_dir,
    }));
    let remote_replay_store: Arc<dyn boundary::RemoteReplayStore + Send + Sync> =
        Arc::new(SqliteRemoteReplayStore::new(Arc::clone(&sqlite_backend)));
    let storage_finalizer: Arc<dyn RuntimeStorageFinalizer + Send + Sync> =
        Arc::new(SqliteRuntimeStorageFinalizer::new(Arc::clone(&sqlite_backend)));
    Ok(RuntimeAssembly {
        service_runtime,
        storage_backends,
        mail_store,
        roster_store,
        task_store,
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
    let notification_path = host_runtime_dir()?.join("notifications.jsonl");
    if let Some(parent) = notification_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to create notification sink directory {}",
                parent.display()
            ))
            .with_recovery(
                "Create a writable ATM runtime directory before constructing the default local retained runtime.",
            )
            .with_source(source)
        })?;
    }
    let sqlite_backend = Arc::new(SqliteStorageBackend::new(atm_core::home::host_mail_db_path()?)?);
    let shared_messages = sqlite_backend.message_store();
    let shared_rosters = sqlite_backend.roster_store();
    let storage_backends = StorageBackends {
        messages: shared_messages.clone(),
        rosters: shared_rosters.clone(),
    };
    let mail_store = legacy_mail_store(shared_messages);
    let roster_store = legacy_roster_store(shared_rosters);
    let task_store = noop_task_store();
    let service_runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        mail_store.clone(),
        task_store.clone(),
        roster_store.clone(),
        Arc::new(LocalFileNonClaudeOutbound::new()),
        Arc::new(LocalFileNotificationSink::at_path(notification_path)),
    );
    let doctor_ports = runtime_doctor_ports(Arc::new(RuntimeConfigDoctor {
        config_current_dir,
    }));
    let remote_replay_store: Arc<dyn boundary::RemoteReplayStore + Send + Sync> =
        Arc::new(SqliteRemoteReplayStore::new(Arc::clone(&sqlite_backend)));
    let storage_finalizer: Arc<dyn RuntimeStorageFinalizer + Send + Sync> =
        Arc::new(SqliteRuntimeStorageFinalizer::new(Arc::clone(&sqlite_backend)));
    Ok(RuntimeAssembly {
        service_runtime,
        storage_backends,
        mail_store,
        roster_store,
        task_store,
        doctor_ports,
        remote_replay_store,
        storage_finalizer,
    })
}

impl RuntimeAssembly {
    pub fn mail_store_arc(&self) -> Arc<dyn boundary::MailStore + Send + Sync> {
        self.mail_store.clone()
    }

    pub fn task_store_arc(&self) -> Arc<dyn boundary::TaskStore + Send + Sync> {
        self.task_store.clone()
    }

    pub fn roster_store_arc(&self) -> Arc<dyn boundary::RosterStore + Send + Sync> {
        self.roster_store.clone()
    }
}

pub fn default_local_runtime() -> Result<LocalServiceRuntime, AtmError> {
    assemble_default_runtime().map(|assembly| assembly.service_runtime)
}

pub fn with_default_roster_store<T>(
    f: impl FnOnce(&(dyn boundary::RosterStore + Send + Sync)) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    let assembly = assemble_default_runtime()?;
    let result = f(assembly.roster_store.as_ref());
    let finalize_result = assembly.storage_finalizer.finalize_storage_shutdown();
    match (result, finalize_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(_)) => Err(error),
    }
}
