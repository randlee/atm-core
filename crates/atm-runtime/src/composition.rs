use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use atm_core::boundary::{
    self, ConfigDoctor, ConfigDoctorReport, NonClaudeOutbound, NotificationSink, RosterStore,
    RuntimeBundle, RuntimeStorageFinalizer,
};
use atm_core::error::AtmError;
use atm_core::{
    LocalFileNonClaudeOutbound, LocalFileNotificationSink, LocalServiceRuntime,
    home::host_runtime_dir, load_atm_config,
};
use atm_rusqlite::{
    SqliteObservability, assemble_boundary_with_observability, assemble_default_boundary,
};

use crate::replay_store::{SqliteRemoteReplayStore, SqliteRuntimeStorageFinalizer};

#[derive(Clone)]
pub struct RuntimeAssemblyInputs {
    pub sqlite_db_path: PathBuf,
    pub sqlite_observability: Arc<dyn SqliteObservability>,
    pub non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
    pub notification_sink: Arc<dyn NotificationSink + Send + Sync>,
}

impl fmt::Debug for RuntimeAssemblyInputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeAssemblyInputs")
            .field("sqlite_db_path", &self.sqlite_db_path)
            .field("sqlite_observability", &"dyn SqliteObservability")
            .field("non_claude_outbound", &"dyn NonClaudeOutbound")
            .field("notification_sink", &"dyn NotificationSink")
            .finish()
    }
}

#[derive(Clone)]
pub struct RuntimeAssembly {
    pub service_runtime: LocalServiceRuntime,
    pub runtime_bundle: RuntimeBundle,
    pub storage_finalizer: Arc<dyn RuntimeStorageFinalizer + Send + Sync>,
}

impl fmt::Debug for RuntimeAssembly {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeAssembly")
            .field("service_runtime", &self.service_runtime)
            .field("runtime_bundle", &self.runtime_bundle)
            .field("storage_finalizer", &"dyn RuntimeStorageFinalizer")
            .finish()
    }
}

#[derive(Debug, Default)]
struct RuntimeConfigDoctor;

impl boundary::sealed::Sealed for RuntimeConfigDoctor {}

impl ConfigDoctor for RuntimeConfigDoctor {
    fn inspect_config(&self) -> Result<ConfigDoctorReport, AtmError> {
        let current_dir = std::env::current_dir().map_err(|source| {
            AtmError::config("failed to resolve current directory for config doctor")
                .with_recovery(
                    "Run the direct doctor path from a readable ATM workspace before retrying config inspection.",
                )
                .with_source(source)
        })?;
        let _ = load_atm_config(&current_dir)?;
        Ok(ConfigDoctorReport {
            findings: Vec::new(),
        })
    }
}

pub fn assemble_sqlite_runtime(inputs: RuntimeAssemblyInputs) -> Result<RuntimeAssembly, AtmError> {
    assemble_sqlite_runtime_at_path(
        &inputs.sqlite_db_path,
        Arc::clone(&inputs.sqlite_observability),
        Arc::clone(&inputs.non_claude_outbound),
        Arc::clone(&inputs.notification_sink),
    )
}

fn assemble_sqlite_runtime_at_path(
    sqlite_db_path: &Path,
    sqlite_observability: Arc<dyn SqliteObservability>,
    non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
    notification_sink: Arc<dyn NotificationSink + Send + Sync>,
) -> Result<RuntimeAssembly, AtmError> {
    let assembly = Arc::new(assemble_boundary_with_observability(
        sqlite_db_path,
        sqlite_observability,
    )?);
    let service_runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        assembly.mail_store_arc(),
        assembly.task_store_arc(),
        assembly.roster_store_arc(),
        non_claude_outbound,
        notification_sink,
    );
    let runtime_bundle = RuntimeBundle {
        mail_store: assembly.mail_store_arc(),
        task_store: assembly.task_store_arc(),
        roster_store: assembly.roster_store_arc(),
        mail_store_doctor: assembly.mail_store_doctor_arc(),
        task_store_doctor: assembly.task_store_doctor_arc(),
        roster_store_doctor: assembly.roster_store_doctor_arc(),
        config_doctor: Arc::new(RuntimeConfigDoctor),
        remote_replay_store: Arc::new(SqliteRemoteReplayStore::new(Arc::clone(&assembly))),
    };
    let storage_finalizer: Arc<dyn RuntimeStorageFinalizer + Send + Sync> =
        Arc::new(SqliteRuntimeStorageFinalizer::new(assembly));
    Ok(RuntimeAssembly {
        service_runtime,
        runtime_bundle,
        storage_finalizer,
    })
}

pub fn assemble_default_runtime() -> Result<RuntimeAssembly, AtmError> {
    let boundary = Arc::new(assemble_default_boundary()?);
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
    let service_runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        boundary.mail_store_arc(),
        boundary.task_store_arc(),
        boundary.roster_store_arc(),
        Arc::new(LocalFileNonClaudeOutbound::new()),
        Arc::new(LocalFileNotificationSink::at_path(notification_path)),
    );
    let runtime_bundle = RuntimeBundle {
        mail_store: boundary.mail_store_arc(),
        task_store: boundary.task_store_arc(),
        roster_store: boundary.roster_store_arc(),
        mail_store_doctor: boundary.mail_store_doctor_arc(),
        task_store_doctor: boundary.task_store_doctor_arc(),
        roster_store_doctor: boundary.roster_store_doctor_arc(),
        config_doctor: Arc::new(RuntimeConfigDoctor),
        remote_replay_store: Arc::new(SqliteRemoteReplayStore::new(Arc::clone(&boundary))),
    };
    let storage_finalizer: Arc<dyn RuntimeStorageFinalizer + Send + Sync> =
        Arc::new(SqliteRuntimeStorageFinalizer::new(boundary));
    Ok(RuntimeAssembly {
        service_runtime,
        runtime_bundle,
        storage_finalizer,
    })
}

pub fn default_local_runtime() -> Result<LocalServiceRuntime, AtmError> {
    assemble_default_runtime().map(|assembly| assembly.service_runtime)
}

pub fn with_default_roster_store<T>(
    f: impl FnOnce(&(dyn RosterStore + Send + Sync)) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    let assembly = assemble_default_runtime()?;
    f(assembly.runtime_bundle.roster_store.as_ref())
}
