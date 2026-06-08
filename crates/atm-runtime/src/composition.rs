use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use atm_core::boundary::{
    self, ConfigDoctor, ConfigDoctorReport, NonClaudeOutbound, NotificationSink, RosterStore,
    RuntimeBundle, RuntimeStorageFinalizer,
};
use atm_core::error::AtmError;
use atm_core::home::{host_mail_db_path, host_runtime_dir};
use atm_core::{
    LocalFileNonClaudeOutbound, LocalFileNotificationSink, LocalServiceRuntime, load_atm_config,
};
use atm_storage::SqliteObservability;
use atm_storage_rusqlite::{
    SqliteIngestReplayStateRecord, SqliteMailHealthSnapshot, SqliteMailboxMetadataCounts,
    SqliteMailboxMetadataRow, SqliteMessageStateRecord, SqliteRosterHealthSnapshot,
    SqliteStorageBackend,
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

#[derive(Clone)]
pub struct SqliteBoundaryAdapters {
    backend: Arc<SqliteStorageBackend>,
    mail_store: Arc<SqliteMailStoreBoundary>,
    task_store: Arc<UnsupportedSqliteTaskStore>,
    roster_store: Arc<SqliteRosterStoreBoundary>,
}

impl fmt::Debug for SqliteBoundaryAdapters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SqliteBoundaryAdapters")
            .field("backend", &self.backend)
            .finish()
    }
}

impl SqliteBoundaryAdapters {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, AtmError> {
        Self::new_with_observability(path, RuntimeSqliteObservability::disabled())
    }

    pub fn new_with_observability(
        path: impl AsRef<Path>,
        observability: Arc<dyn SqliteObservability>,
    ) -> Result<Self, AtmError> {
        let backend = Arc::new(SqliteStorageBackend::new_with_observability(
            path.as_ref(),
            observability,
        )?);
        Ok(Self::from_backend(backend))
    }

    pub fn default_production() -> Result<Self, AtmError> {
        Self::new(host_mail_db_path()?)
    }

    fn from_backend(backend: Arc<SqliteStorageBackend>) -> Self {
        Self {
            mail_store: Arc::new(SqliteMailStoreBoundary::new(Arc::clone(&backend))),
            task_store: Arc::new(UnsupportedSqliteTaskStore),
            roster_store: Arc::new(SqliteRosterStoreBoundary::new(Arc::clone(&backend))),
            backend,
        }
    }

    pub fn mail_store(&self) -> &(dyn boundary::MailStore + Send + Sync) {
        self.mail_store.as_ref()
    }

    pub fn mail_store_arc(&self) -> Arc<dyn boundary::MailStore + Send + Sync> {
        self.mail_store.clone()
    }

    pub fn task_store(&self) -> &(dyn boundary::TaskStore + Send + Sync) {
        self.task_store.as_ref()
    }

    pub fn task_store_arc(&self) -> Arc<dyn boundary::TaskStore + Send + Sync> {
        self.task_store.clone()
    }

    pub fn roster_store(&self) -> &(dyn boundary::RosterStore + Send + Sync) {
        self.roster_store.as_ref()
    }

    pub fn roster_store_arc(&self) -> Arc<dyn boundary::RosterStore + Send + Sync> {
        self.roster_store.clone()
    }

    pub fn mail_store_doctor_arc(&self) -> Arc<dyn boundary::MailStoreDoctor + Send + Sync> {
        self.mail_store.clone()
    }

    pub fn task_store_doctor_arc(&self) -> Arc<dyn boundary::TaskStoreDoctor + Send + Sync> {
        self.task_store.clone()
    }

    pub fn roster_store_doctor_arc(&self) -> Arc<dyn boundary::RosterStoreDoctor + Send + Sync> {
        self.roster_store.clone()
    }

    pub fn checkpoint_wal(&self) -> Result<(), AtmError> {
        self.backend.checkpoint_wal()
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

#[derive(Debug, Clone)]
struct SqliteMailStoreBoundary {
    backend: Arc<SqliteStorageBackend>,
}

impl SqliteMailStoreBoundary {
    fn new(backend: Arc<SqliteStorageBackend>) -> Self {
        Self { backend }
    }
}

impl boundary::sealed::Sealed for SqliteMailStoreBoundary {}

impl boundary::MailStore for SqliteMailStoreBoundary {
    #[allow(deprecated)]
    fn bootstrap(
        &self,
        request: boundary::MailStoreBootstrapRequest,
    ) -> Result<boundary::MailStoreBootstrapResponse, AtmError> {
        self.backend.inspect_mail_store()?;
        Ok(boundary::MailStoreBootstrapResponse {
            team: request.team,
            bootstrapped: true,
            opened: true,
        })
    }

    fn upsert_message(&self, record: boundary::MailStoreMessageRecord) -> Result<(), AtmError> {
        self.backend.save_message_record(
            record.team,
            record.agent,
            record.message_key,
            record.envelope,
        )
    }

    fn load_message(
        &self,
        team: &atm_core::types::TeamName,
        agent: &atm_core::types::AgentName,
        message_key: &boundary::MessageKey,
    ) -> Result<Option<boundary::MailStoreMessageRecord>, AtmError> {
        self.backend.load_message_record(message_key).map(|record| {
            record
                .filter(|record| &record.team == team && &record.agent == agent)
                .map(|record| boundary::MailStoreMessageRecord {
                    team: record.team,
                    agent: record.agent,
                    message_key: record.message_key,
                    envelope: record.envelope,
                })
        })
    }

    fn query_mailbox_metadata(
        &self,
        team: &atm_core::types::TeamName,
        agent: &atm_core::types::AgentName,
        limit: Option<usize>,
    ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError> {
        self.backend
            .query_mailbox_metadata(team, agent, limit)
            .map(|rows| rows.into_iter().map(mailbox_metadata_row).collect())
    }

    fn query_mailbox_metadata_counts(
        &self,
        team: &atm_core::types::TeamName,
        agent: &atm_core::types::AgentName,
    ) -> Result<boundary::MailStoreMailboxMetadataCounts, AtmError> {
        self.backend
            .query_mailbox_metadata_counts(team, agent)
            .map(mailbox_metadata_counts)
    }

    fn upsert_message_state(
        &self,
        request: boundary::UpsertMailMessageStateRequest,
    ) -> Result<boundary::UpsertMailMessageStateResponse, AtmError> {
        self.backend
            .upsert_message_state(SqliteMessageStateRecord {
                team: request.team,
                agent: request.agent,
                actor: request.actor,
                message_key: request.state.message_key.clone(),
                read: request.state.read,
                pending_ack_at: request.state.pending_ack_at,
                acknowledged_at: request.state.acknowledged_at,
                expires_at: request.state.expires_at,
                deleted_at: request.state.deleted_at,
                updated_at: request.state.updated_at,
            })?;
        Ok(boundary::UpsertMailMessageStateResponse {
            state: request.state,
        })
    }

    fn load_message_state(
        &self,
        request: boundary::LoadMailMessageStateRequest,
    ) -> Result<boundary::LoadMailMessageStateResponse, AtmError> {
        self.backend
            .load_message_state(
                &request.team,
                &request.agent,
                &request.actor,
                &request.message_key,
            )
            .map(|state| boundary::LoadMailMessageStateResponse {
                state: state.map(message_state_record),
            })
    }

    fn record_ingest_replay_state(
        &self,
        team: &atm_core::types::TeamName,
        agent: &atm_core::types::AgentName,
        source: &boundary::ReplaySource,
        state: &boundary::MailStoreIngestReplayState,
    ) -> Result<(), AtmError> {
        self.backend
            .record_ingest_replay_state(&SqliteIngestReplayStateRecord {
                team: team.clone(),
                agent: agent.clone(),
                source: source.as_str().to_string(),
                last_fingerprint: state.last_fingerprint.as_ref().map(ToString::to_string),
                last_ingested_at: state.last_ingested_at,
                ingested_rows: state.ingested_rows,
            })
    }

    fn load_ingest_replay_state(
        &self,
        team: &atm_core::types::TeamName,
        agent: &atm_core::types::AgentName,
        source: &boundary::ReplaySource,
    ) -> Result<Option<boundary::MailStoreIngestReplayState>, AtmError> {
        self.backend
            .load_ingest_replay_state(team, agent, source.as_str())
            .and_then(|state| {
                state
                    .map(|state| {
                        Ok(boundary::MailStoreIngestReplayState {
                            team: state.team,
                            agent: state.agent,
                            source: boundary::ReplaySource::new(state.source)?,
                            last_fingerprint: state
                                .last_fingerprint
                                .map(boundary::MessageFingerprint::new),
                            last_ingested_at: state.last_ingested_at,
                            ingested_rows: state.ingested_rows,
                        })
                    })
                    .transpose()
            })
    }

    fn health_snapshot(
        &self,
        team: &atm_core::types::TeamName,
        agent: &atm_core::types::AgentName,
    ) -> Result<boundary::MailStoreHealthSnapshot, AtmError> {
        self.backend
            .mail_health_snapshot(team, agent)
            .map(mail_health_snapshot)
    }
}

impl boundary::MailStoreDoctor for SqliteMailStoreBoundary {
    fn inspect_mail_store(&self) -> Result<boundary::MailStoreDoctorReport, AtmError> {
        self.backend.inspect_mail_store()?;
        Ok(boundary::MailStoreDoctorReport {
            findings: Vec::new(),
        })
    }
}

#[derive(Debug, Clone)]
struct SqliteRosterStoreBoundary {
    backend: Arc<SqliteStorageBackend>,
}

impl SqliteRosterStoreBoundary {
    fn new(backend: Arc<SqliteStorageBackend>) -> Self {
        Self { backend }
    }
}

impl boundary::sealed::Sealed for SqliteRosterStoreBoundary {}

impl boundary::RosterStore for SqliteRosterStoreBoundary {
    fn replace_roster(
        &self,
        team: &atm_core::types::TeamName,
        members: &[boundary::RosterMemberRecord],
        _source: Option<&boundary::ReplaySource>,
    ) -> Result<(), AtmError> {
        self.backend.replace_roster(team.clone(), members.to_vec())
    }

    fn load_roster(
        &self,
        team: &atm_core::types::TeamName,
    ) -> Result<Vec<boundary::RosterMemberRecord>, AtmError> {
        self.backend.load_roster_members(team)
    }

    fn query_membership(
        &self,
        team: &atm_core::types::TeamName,
        member: &atm_core::types::AgentName,
    ) -> Result<Option<boundary::RosterMemberRecord>, AtmError> {
        self.load_roster(team).map(|members| {
            members
                .into_iter()
                .find(|entry| &entry.agent_name == member)
        })
    }

    fn list_teams(&self) -> Result<Vec<atm_core::types::TeamName>, AtmError> {
        self.backend.roster_store().list_teams()
    }

    fn health_snapshot(
        &self,
        team: &atm_core::types::TeamName,
    ) -> Result<boundary::RosterStoreHealthSnapshot, AtmError> {
        self.backend
            .roster_health_snapshot(team)
            .map(roster_health_snapshot)
    }
}

impl boundary::RosterStoreDoctor for SqliteRosterStoreBoundary {
    fn inspect_roster_store(&self) -> Result<boundary::RosterStoreDoctorReport, AtmError> {
        self.backend.inspect_roster_store()?;
        Ok(boundary::RosterStoreDoctorReport {
            findings: Vec::new(),
        })
    }
}

#[derive(Debug, Clone)]
struct UnsupportedSqliteTaskStore;

impl UnsupportedSqliteTaskStore {
    fn unsupported(action: &str) -> AtmError {
        AtmError::validation(format!(
            "sqlite runtime task storage is out of scope for AC.3 and cannot {action}"
        ))
        .with_recovery(
            "Use the Claude-code task surface or defer task storage until a canonical task-store phase is approved.",
        )
    }
}

impl boundary::sealed::Sealed for UnsupportedSqliteTaskStore {}

impl boundary::TaskStore for UnsupportedSqliteTaskStore {
    fn create_task(
        &self,
        _request: boundary::TaskStoreCreateTaskRequest,
    ) -> Result<boundary::TaskStoreCreateTaskResponse, AtmError> {
        Err(Self::unsupported("create tasks"))
    }

    fn load_task(
        &self,
        _request: boundary::TaskStoreLoadTaskRequest,
    ) -> Result<boundary::TaskStoreLoadTaskResponse, AtmError> {
        Err(Self::unsupported("load tasks"))
    }

    fn update_task(
        &self,
        _request: boundary::TaskStoreUpdateTaskRequest,
    ) -> Result<boundary::TaskStoreUpdateTaskResponse, AtmError> {
        Err(Self::unsupported("update tasks"))
    }

    fn attach_message_link(
        &self,
        _request: boundary::TaskStoreAttachMessageLinkRequest,
    ) -> Result<boundary::TaskStoreAttachMessageLinkResponse, AtmError> {
        Err(Self::unsupported("attach task/message links"))
    }

    fn detach_message_link(
        &self,
        _request: boundary::TaskStoreDetachMessageLinkRequest,
    ) -> Result<boundary::TaskStoreDetachMessageLinkResponse, AtmError> {
        Err(Self::unsupported("detach task/message links"))
    }

    fn record_ack_transition(
        &self,
        _request: boundary::TaskStoreRecordAckTransitionRequest,
    ) -> Result<boundary::TaskStoreRecordAckTransitionResponse, AtmError> {
        Err(Self::unsupported("record ack transitions"))
    }

    fn query_task_metadata(
        &self,
        _request: boundary::TaskStoreQueryTaskMetadataRequest,
    ) -> Result<boundary::TaskStoreQueryTaskMetadataResponse, AtmError> {
        Err(Self::unsupported("query task metadata"))
    }
}

impl boundary::TaskStoreDoctor for UnsupportedSqliteTaskStore {
    fn inspect_task_store(&self) -> Result<boundary::TaskStoreDoctorReport, AtmError> {
        Ok(boundary::TaskStoreDoctorReport::default())
    }
}

fn mailbox_metadata_row(row: SqliteMailboxMetadataRow) -> boundary::MailStoreMailboxMetadataRow {
    boundary::MailStoreMailboxMetadataRow {
        message_key: row.message_key,
        message_id: row.message_id,
        parent_message_id: row.parent_message_id,
        thread_mode: row.thread_mode,
        from_agent: row.from_agent,
        summary: row.summary,
        message_at: row.message_at,
        read: row.read,
        pending_ack: row.pending_ack,
        acknowledged_at: row.acknowledged_at,
        expires_at: row.expires_at,
        task_id: row.task_id,
    }
}

fn mailbox_metadata_counts(
    counts: SqliteMailboxMetadataCounts,
) -> boundary::MailStoreMailboxMetadataCounts {
    boundary::MailStoreMailboxMetadataCounts {
        total_messages: counts.total_messages,
        unread_message_count: counts.unread_message_count,
        pending_ack_messages: counts.pending_ack_messages,
    }
}

fn message_state_record(state: SqliteMessageStateRecord) -> boundary::MailMessageState {
    boundary::MailMessageState {
        team: state.team,
        agent: state.agent,
        actor: state.actor,
        message_key: state.message_key,
        read: state.read,
        pending_ack_at: state.pending_ack_at,
        acknowledged_at: state.acknowledged_at,
        expires_at: state.expires_at,
        deleted_at: state.deleted_at,
        updated_at: state.updated_at,
    }
}

fn mail_health_snapshot(snapshot: SqliteMailHealthSnapshot) -> boundary::MailStoreHealthSnapshot {
    boundary::MailStoreHealthSnapshot {
        team: snapshot.team,
        agent: snapshot.agent,
        total_messages: snapshot.total_messages,
        pending_ack_messages: snapshot.pending_ack_messages,
        read_message_count: snapshot.read_message_count,
        latest_message_timestamp: snapshot.latest_message_timestamp,
    }
}

fn roster_health_snapshot(
    snapshot: SqliteRosterHealthSnapshot,
) -> boundary::RosterStoreHealthSnapshot {
    boundary::RosterStoreHealthSnapshot {
        team: snapshot.team,
        member_count: snapshot.member_count,
        stale: snapshot.stale,
        refreshed_at: snapshot.refreshed_at,
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
    sqlite_observer: Arc<dyn RuntimeSqliteObserver>,
    non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
    notification_sink: Arc<dyn NotificationSink + Send + Sync>,
) -> Result<RuntimeAssembly, AtmError> {
    let sqlite_observability = Arc::new(RuntimeSqliteObservability::new(sqlite_observer));
    let adapters = Arc::new(SqliteBoundaryAdapters::new_with_observability(
        sqlite_db_path,
        sqlite_observability,
    )?);
    build_runtime_assembly(
        adapters,
        config_current_dir,
        non_claude_outbound,
        notification_sink,
    )
}

pub fn assemble_default_runtime() -> Result<RuntimeAssembly, AtmError> {
    let config_current_dir = std::env::current_dir().map_err(|source| {
        AtmError::config("failed to resolve current directory for direct runtime assembly")
            .with_recovery(
                "Run the direct retained runtime path from a readable ATM workspace so config inspection and runtime assembly share one validated root.",
            )
            .with_source(source)
    })?;
    let adapters = Arc::new(SqliteBoundaryAdapters::default_production()?);
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
    build_runtime_assembly(
        adapters,
        config_current_dir,
        Arc::new(LocalFileNonClaudeOutbound::new()),
        Arc::new(LocalFileNotificationSink::at_path(notification_path)),
    )
}

fn build_runtime_assembly(
    adapters: Arc<SqliteBoundaryAdapters>,
    config_current_dir: PathBuf,
    non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
    notification_sink: Arc<dyn NotificationSink + Send + Sync>,
) -> Result<RuntimeAssembly, AtmError> {
    let service_runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        adapters.mail_store_arc(),
        adapters.task_store_arc(),
        adapters.roster_store_arc(),
        non_claude_outbound,
        notification_sink,
    );
    let runtime_bundle = RuntimeBundle {
        mail_store: adapters.mail_store_arc(),
        task_store: adapters.task_store_arc(),
        roster_store: adapters.roster_store_arc(),
        mail_store_doctor: adapters.mail_store_doctor_arc(),
        task_store_doctor: adapters.task_store_doctor_arc(),
        roster_store_doctor: adapters.roster_store_doctor_arc(),
        config_doctor: Arc::new(RuntimeConfigDoctor { config_current_dir }),
        remote_replay_store: Arc::new(SqliteRemoteReplayStore::new(Arc::clone(&adapters.backend))),
    };
    let storage_finalizer: Arc<dyn RuntimeStorageFinalizer + Send + Sync> = Arc::new(
        SqliteRuntimeStorageFinalizer::new(Arc::clone(&adapters.backend)),
    );
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
    let result = f(assembly.runtime_bundle.roster_store.as_ref());
    let finalize_result = assembly.storage_finalizer.finalize_storage_shutdown();
    match (result, finalize_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(_)) => Err(error),
    }
}
