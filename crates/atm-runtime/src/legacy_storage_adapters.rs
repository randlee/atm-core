use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex};

#[allow(
    deprecated,
    reason = "AC.4 keeps the legacy mail bootstrap surface as a temporary compile bridge until atm-core consumer cutover is complete."
)]
use atm_core::boundary::{
    self, ConfigDoctor, LoadMailMessageStateRequest, LoadMailMessageStateResponse, MailStore,
    MailStoreDoctor, MailStoreDoctorReport, MailStoreHealthSnapshot, MailStoreIngestReplayState,
    MailStoreMailboxMetadataCounts, MailStoreMailboxMetadataRow, Message, ReplaySource,
    RosterStoreDoctor, RosterStoreDoctorReport, TaskStore, TaskStoreAttachMessageLinkRequest,
    TaskStoreAttachMessageLinkResponse, TaskStoreCreateTaskRequest, TaskStoreCreateTaskResponse,
    TaskStoreDetachMessageLinkRequest, TaskStoreDetachMessageLinkResponse,
    TaskStoreLoadTaskRequest, TaskStoreLoadTaskResponse, TaskStoreQueryTaskMetadataRequest,
    TaskStoreQueryTaskMetadataResponse, TaskStoreRecordAckTransitionRequest,
    TaskStoreRecordAckTransitionResponse, TaskStoreTaskRecord, TaskStoreUpdateTaskRequest,
    TaskStoreUpdateTaskResponse, UpsertMailMessageStateRequest, UpsertMailMessageStateResponse,
};
use atm_core::doctor::RuntimeDoctorPorts;
use atm_core::error::AtmError;
use atm_storage::contract::{
    Message as SharedMessage, MessageQuery, MessageStore as SharedMessageStore, RosterSnapshot,
    RosterStore as SharedRosterStore,
};
use atm_storage::{AgentName, TeamName};

#[derive(Clone)]
pub(crate) struct StorageBackends<M, R>
where
    M: Deref<Target = dyn SharedMessageStore + Send + Sync>,
    R: Deref<Target = dyn SharedRosterStore + Send + Sync>,
{
    pub(crate) messages: M,
    pub(crate) rosters: R,
}

impl<M, R> std::fmt::Debug for StorageBackends<M, R>
where
    M: Deref<Target = dyn SharedMessageStore + Send + Sync>,
    R: Deref<Target = dyn SharedRosterStore + Send + Sync>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageBackends")
            .field("messages", &std::any::type_name::<M>())
            .field("rosters", &std::any::type_name::<R>())
            .finish()
    }
}

#[derive(Clone, Default)]
struct NoopTaskStore;

#[derive(Clone, Default)]
struct DefaultMailStoreDoctor;

#[derive(Clone, Default)]
struct DefaultRosterStoreDoctor;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ReplayStateKey {
    team: String,
    agent: String,
    source: String,
}

#[derive(Clone, Default)]
struct InMemoryIngestReplayState {
    // Cloned boundary views must observe and mutate the same replay-state map so
    // a read path and a write path in the same runtime composition do not fork
    // in-memory replay cursors while AC.4 keeps this temporary compile bridge.
    states: Arc<Mutex<HashMap<ReplayStateKey, MailStoreIngestReplayState>>>,
}

#[derive(Clone)]
struct BoundaryMailStoreView {
    store: Arc<dyn SharedMessageStore + Send + Sync>,
    replay_state: InMemoryIngestReplayState,
}

#[derive(Clone)]
struct BoundaryRosterStoreView {
    store: Arc<dyn SharedRosterStore + Send + Sync>,
}

impl BoundaryMailStoreView {
    fn new(store: Arc<dyn SharedMessageStore + Send + Sync>) -> Self {
        Self {
            store,
            replay_state: InMemoryIngestReplayState::default(),
        }
    }

    fn shared_query(team: &TeamName, agent: &AgentName, limit: Option<usize>) -> MessageQuery {
        MessageQuery {
            team: team.clone(),
            agent: agent.clone(),
            sender: None,
            task_id: None,
            limit,
        }
    }

    fn mailbox_row(message: SharedMessage) -> MailStoreMailboxMetadataRow {
        MailStoreMailboxMetadataRow {
            message_key: message.message_key.clone(),
            message_id: message.envelope.message_id,
            parent_message_id: message.envelope.parent_message_id,
            thread_mode: message.envelope.thread_mode,
            from_agent: message.envelope.from,
            summary: message.envelope.summary,
            message_at: message.envelope.timestamp,
            read: message.envelope.read,
            pending_ack: message.envelope.pending_ack_at.is_some(),
            acknowledged_at: message.envelope.acknowledged_at,
            expires_at: message.envelope.expires_at,
            task_id: message.envelope.task_id,
        }
    }

    fn load_matching_message(
        &self,
        team: &TeamName,
        agent: &AgentName,
        message_key: &atm_storage::MessageKey,
    ) -> Result<Option<SharedMessage>, AtmError> {
        let loaded = self.store.load_message(message_key)?;
        Ok(loaded.filter(|message| &message.team == team && &message.agent == agent))
    }
}

impl BoundaryRosterStoreView {
    fn new(store: Arc<dyn SharedRosterStore + Send + Sync>) -> Self {
        Self { store }
    }
}

pub(crate) fn runtime_doctor_ports(
    config_doctor: Arc<dyn ConfigDoctor + Send + Sync>,
) -> RuntimeDoctorPorts {
    RuntimeDoctorPorts {
        config_doctor,
        mail_store_doctor: Arc::new(DefaultMailStoreDoctor),
        roster_store_doctor: Arc::new(DefaultRosterStoreDoctor),
    }
}
impl boundary::sealed::Sealed for BoundaryMailStoreView {}
impl boundary::sealed::Sealed for BoundaryRosterStoreView {}
impl boundary::sealed::Sealed for NoopTaskStore {}
impl boundary::sealed::Sealed for DefaultMailStoreDoctor {}
impl boundary::sealed::Sealed for DefaultRosterStoreDoctor {}

impl MailStore for BoundaryMailStoreView {
    fn upsert_message(&self, record: Message) -> Result<(), AtmError> {
        self.store.save_message(&SharedMessage {
            team: record.team,
            agent: record.agent,
            message_key: record.message_key,
            envelope: record.envelope,
        })
    }

    fn load_message(
        &self,
        team: &TeamName,
        agent: &AgentName,
        message_key: &atm_storage::MessageKey,
    ) -> Result<Option<Message>, AtmError> {
        Ok(self
            .load_matching_message(team, agent, message_key)?
            .map(|message| Message {
                team: message.team,
                agent: message.agent,
                message_key: message.message_key,
                envelope: message.envelope,
            }))
    }

    fn query_mailbox_metadata(
        &self,
        team: &TeamName,
        agent: &AgentName,
        limit: Option<usize>,
    ) -> Result<Vec<MailStoreMailboxMetadataRow>, AtmError> {
        self.store
            .list_messages(&Self::shared_query(team, agent, limit))
            .map(|messages| messages.into_iter().map(Self::mailbox_row).collect())
    }

    fn query_mailbox_metadata_counts(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<MailStoreMailboxMetadataCounts, AtmError> {
        let rows = self.query_mailbox_metadata(team, agent, None)?;
        Ok(MailStoreMailboxMetadataCounts {
            total_messages: rows.len() as u64,
            unread_message_count: rows.iter().filter(|row| !row.read).count() as u64,
            pending_ack_messages: rows.iter().filter(|row| row.pending_ack).count() as u64,
        })
    }

    fn upsert_message_state(
        &self,
        request: UpsertMailMessageStateRequest,
    ) -> Result<UpsertMailMessageStateResponse, AtmError> {
        if request.state.deleted_at.is_some() {
            self.store.delete_message(&request.state.message_key)?;
            return Ok(UpsertMailMessageStateResponse {
                state: request.state,
            });
        }
        let mut message = self
            .load_matching_message(&request.team, &request.agent, &request.state.message_key)?
            .ok_or_else(|| {
                AtmError::mailbox_read(format!(
                    "message {} was not found for {}@{} while updating mailbox state",
                    request.state.message_key.as_ref(),
                    request.agent.as_str(),
                    request.team.as_str(),
                ))
            })?;
        message.envelope.read = request.state.read;
        message.envelope.pending_ack_at = request.state.pending_ack_at;
        message.envelope.acknowledged_at = request.state.acknowledged_at;
        message.envelope.expires_at = request.state.expires_at;
        self.store.save_message(&message)?;
        Ok(UpsertMailMessageStateResponse {
            state: request.state,
        })
    }

    fn load_message_state(
        &self,
        request: LoadMailMessageStateRequest,
    ) -> Result<LoadMailMessageStateResponse, AtmError> {
        Ok(LoadMailMessageStateResponse {
            state: self
                .load_matching_message(&request.team, &request.agent, &request.message_key)?
                .map(|message| boundary::MailMessageState {
                    team: request.team,
                    agent: request.agent,
                    actor: request.actor,
                    message_key: request.message_key,
                    read: message.envelope.read,
                    pending_ack_at: message.envelope.pending_ack_at,
                    acknowledged_at: message.envelope.acknowledged_at,
                    expires_at: message.envelope.expires_at,
                    deleted_at: None,
                    updated_at: None,
                }),
        })
    }

    fn record_ingest_replay_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
        source: &ReplaySource,
        state: &MailStoreIngestReplayState,
    ) -> Result<(), AtmError> {
        let key = ReplayStateKey {
            team: team.as_str().to_string(),
            agent: agent.as_str().to_string(),
            source: source.as_str().to_string(),
        };
        self.replay_state
            .states
            .lock()
            .map_err(|_| {
                AtmError::daemon_unavailable("ingest replay state lock poisoned").with_recovery(
                    "If the lock is poisoned restart the daemon process to reset the in-process state.",
                )
            })?
            .insert(key, state.clone());
        Ok(())
    }

    fn load_ingest_replay_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
        source: &ReplaySource,
    ) -> Result<Option<MailStoreIngestReplayState>, AtmError> {
        let key = ReplayStateKey {
            team: team.as_str().to_string(),
            agent: agent.as_str().to_string(),
            source: source.as_str().to_string(),
        };
        Ok(self
            .replay_state
            .states
            .lock()
            .map_err(|_| {
                AtmError::daemon_unavailable("ingest replay state lock poisoned").with_recovery(
                    "If the lock is poisoned restart the daemon process to reset the in-process state.",
                )
            })?
            .get(&key)
            .cloned())
    }

    fn health_snapshot(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<MailStoreHealthSnapshot, AtmError> {
        let rows = self.query_mailbox_metadata(team, agent, None)?;
        let latest_message_timestamp = rows.iter().map(|row| row.message_at).max();
        Ok(MailStoreHealthSnapshot {
            team: team.clone(),
            agent: agent.clone(),
            total_messages: rows.len() as u64,
            pending_ack_messages: rows.iter().filter(|row| row.pending_ack).count() as u64,
            read_message_count: rows.iter().filter(|row| row.read).count() as u64,
            latest_message_timestamp,
        })
    }
}

impl boundary::RosterStore for BoundaryRosterStoreView {
    fn replace_roster(
        &self,
        team: &TeamName,
        members: &[boundary::RosterEntry],
        _source: Option<&ReplaySource>,
    ) -> Result<(), AtmError> {
        self.store.save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members: members.to_vec(),
            refreshed_at: None,
        })
    }

    fn load_roster(&self, team: &TeamName) -> Result<Vec<boundary::RosterEntry>, AtmError> {
        self.store
            .load_roster(team)
            .map(|snapshot| snapshot.members)
    }

    fn query_membership(
        &self,
        team: &TeamName,
        member: &AgentName,
    ) -> Result<Option<boundary::RosterEntry>, AtmError> {
        let snapshot = self.store.load_roster(team)?;
        Ok(snapshot
            .members
            .into_iter()
            .find(|entry| &entry.agent_name == member))
    }

    fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
        self.store.list_teams()
    }

    fn health_snapshot(
        &self,
        team: &TeamName,
    ) -> Result<boundary::RosterStoreHealthSnapshot, AtmError> {
        let snapshot = self.store.load_roster(team)?;
        Ok(boundary::RosterStoreHealthSnapshot {
            team: team.clone(),
            member_count: snapshot.members.len() as u64,
            stale: false,
            refreshed_at: snapshot.refreshed_at,
        })
    }
}

impl TaskStore for NoopTaskStore {
    fn create_task(
        &self,
        _request: TaskStoreCreateTaskRequest,
    ) -> Result<TaskStoreCreateTaskResponse, AtmError> {
        Err(AtmError::validation(
            "task storage is out of scope for the current runtime assembly",
        )
        .with_recovery(
            "Use the message and roster storage surfaces only until a task-storage backend is explicitly approved.",
        ))
    }

    fn load_task(
        &self,
        request: TaskStoreLoadTaskRequest,
    ) -> Result<TaskStoreLoadTaskResponse, AtmError> {
        let _ = request;
        Ok(TaskStoreLoadTaskResponse { record: None })
    }

    fn update_task(
        &self,
        _request: TaskStoreUpdateTaskRequest,
    ) -> Result<TaskStoreUpdateTaskResponse, AtmError> {
        Err(AtmError::validation(
            "task storage is out of scope for the current runtime assembly",
        )
        .with_recovery(
            "Use the message and roster storage surfaces only until a task-storage backend is explicitly approved.",
        ))
    }

    fn attach_message_link(
        &self,
        _request: TaskStoreAttachMessageLinkRequest,
    ) -> Result<TaskStoreAttachMessageLinkResponse, AtmError> {
        Err(AtmError::validation(
            "task storage is out of scope for the current runtime assembly",
        )
        .with_recovery(
            "Use the message and roster storage surfaces only until a task-storage backend is explicitly approved.",
        ))
    }

    fn detach_message_link(
        &self,
        _request: TaskStoreDetachMessageLinkRequest,
    ) -> Result<TaskStoreDetachMessageLinkResponse, AtmError> {
        Err(AtmError::validation(
            "task storage is out of scope for the current runtime assembly",
        )
        .with_recovery(
            "Use the message and roster storage surfaces only until a task-storage backend is explicitly approved.",
        ))
    }

    fn record_ack_transition(
        &self,
        _request: TaskStoreRecordAckTransitionRequest,
    ) -> Result<TaskStoreRecordAckTransitionResponse, AtmError> {
        Err(AtmError::validation(
            "task storage is out of scope for the current runtime assembly",
        )
        .with_recovery(
            "Use the message and roster storage surfaces only until a task-storage backend is explicitly approved.",
        ))
    }

    fn query_task_metadata(
        &self,
        _request: TaskStoreQueryTaskMetadataRequest,
    ) -> Result<TaskStoreQueryTaskMetadataResponse, AtmError> {
        Ok(TaskStoreQueryTaskMetadataResponse {
            records: Vec::<TaskStoreTaskRecord>::new(),
        })
    }
}

impl MailStoreDoctor for DefaultMailStoreDoctor {
    fn inspect_mail_store(&self) -> Result<MailStoreDoctorReport, AtmError> {
        // This bridge doctor has no backend-owned failure mode; it only reports
        // the absence of extra diagnostics while AC.4 keeps the compile bridge.
        Ok(MailStoreDoctorReport::default())
    }
}

impl RosterStoreDoctor for DefaultRosterStoreDoctor {
    fn inspect_roster_store(&self) -> Result<RosterStoreDoctorReport, AtmError> {
        // This bridge doctor has no backend-owned failure mode; it only reports
        // the absence of extra diagnostics while AC.4 keeps the compile bridge.
        Ok(RosterStoreDoctorReport::default())
    }
}

pub(crate) fn noop_task_store() -> Arc<dyn TaskStore + Send + Sync> {
    Arc::new(NoopTaskStore)
}

pub(crate) fn boundary_mail_store_view(
    store: Arc<dyn SharedMessageStore + Send + Sync>,
) -> Arc<dyn MailStore + Send + Sync> {
    Arc::new(BoundaryMailStoreView::new(store))
}

pub(crate) fn boundary_roster_store_view(
    store: Arc<dyn SharedRosterStore + Send + Sync>,
) -> Arc<dyn boundary::RosterStore + Send + Sync> {
    Arc::new(BoundaryRosterStoreView::new(store))
}
