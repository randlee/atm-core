#![forbid(unsafe_code)]
#![allow(dead_code)]

//! Skeleton crate for Phase R rusqlite adapter work.

use atm_core::boundary;
use atm_core::error::AtmError;

/// Internal assembly root for Phase R SQLite-backed boundary placeholders.
#[derive(Debug, Default)]
pub(crate) struct SqliteBoundaryAssembly {
    mail_store: SqliteMailStore,
    task_store: SqliteTaskStore,
    roster_store: SqliteRosterStore,
}

impl SqliteBoundaryAssembly {
    pub(crate) fn new() -> Self {
        Self {
            mail_store: SqliteMailStore::new(),
            task_store: SqliteTaskStore::new(),
            roster_store: SqliteRosterStore::new(),
        }
    }

    pub(crate) fn mail_store(&self) -> &SqliteMailStore {
        &self.mail_store
    }

    pub(crate) fn task_store(&self) -> &SqliteTaskStore {
        &self.task_store
    }

    pub(crate) fn roster_store(&self) -> &SqliteRosterStore {
        &self.roster_store
    }
}

pub(crate) fn assemble_boundary() -> SqliteBoundaryAssembly {
    SqliteBoundaryAssembly::new()
}

#[derive(Debug, Default)]
struct SqliteMailStore;

impl SqliteMailStore {
    fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for SqliteMailStore {}

impl boundary::MailStore for SqliteMailStore {
    fn bootstrap(
        &self,
        _request: boundary::MailStoreBootstrapRequest,
    ) -> Result<boundary::MailStoreBootstrapResponse, AtmError> {
        todo!("Phase R SQLite mail-store bootstrap wiring is not implemented yet");
    }

    fn run_transaction(
        &self,
        _request: boundary::MailStoreTransactionRequest,
    ) -> Result<boundary::MailStoreTransactionResponse, AtmError> {
        todo!("Phase R SQLite mail-store transaction wiring is not implemented yet");
    }

    fn upsert_message(
        &self,
        _request: boundary::MailStoreUpsertMessageRequest,
    ) -> Result<boundary::MailStoreUpsertMessageResponse, AtmError> {
        todo!("Phase R SQLite mail-store message upsert wiring is not implemented yet");
    }

    fn load_message(
        &self,
        _request: boundary::MailStoreLoadMessageRequest,
    ) -> Result<boundary::MailStoreLoadMessageResponse, AtmError> {
        todo!("Phase R SQLite mail-store message load wiring is not implemented yet");
    }

    fn upsert_visibility_state(
        &self,
        _request: boundary::MailStoreUpsertVisibilityStateRequest,
    ) -> Result<boundary::MailStoreUpsertVisibilityStateResponse, AtmError> {
        todo!("Phase R SQLite mail-store visibility upsert wiring is not implemented yet");
    }

    fn load_visibility_state(
        &self,
        _request: boundary::MailStoreLoadVisibilityStateRequest,
    ) -> Result<boundary::MailStoreLoadVisibilityStateResponse, AtmError> {
        todo!("Phase R SQLite mail-store visibility load wiring is not implemented yet");
    }

    fn record_ingest_replay_state(
        &self,
        _request: boundary::MailStoreRecordIngestReplayStateRequest,
    ) -> Result<boundary::MailStoreRecordIngestReplayStateResponse, AtmError> {
        todo!("Phase R SQLite mail-store replay-state record wiring is not implemented yet");
    }

    fn load_ingest_replay_state(
        &self,
        _request: boundary::MailStoreLoadIngestReplayStateRequest,
    ) -> Result<boundary::MailStoreLoadIngestReplayStateResponse, AtmError> {
        todo!("Phase R SQLite mail-store replay-state load wiring is not implemented yet");
    }

    fn health_snapshot(
        &self,
        _request: boundary::MailStoreHealthSnapshotRequest,
    ) -> Result<boundary::MailStoreHealthSnapshotResponse, AtmError> {
        todo!("Phase R SQLite mail-store health wiring is not implemented yet");
    }
}

#[derive(Debug, Default)]
struct SqliteTaskStore;

impl SqliteTaskStore {
    fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for SqliteTaskStore {}

impl boundary::TaskStore for SqliteTaskStore {
    fn create_task(
        &self,
        _request: boundary::TaskStoreCreateTaskRequest,
    ) -> Result<boundary::TaskStoreCreateTaskResponse, AtmError> {
        todo!("Phase R SQLite task-store create wiring is not implemented yet");
    }

    fn load_task(
        &self,
        _request: boundary::TaskStoreLoadTaskRequest,
    ) -> Result<boundary::TaskStoreLoadTaskResponse, AtmError> {
        todo!("Phase R SQLite task-store load wiring is not implemented yet");
    }

    fn update_task(
        &self,
        _request: boundary::TaskStoreUpdateTaskRequest,
    ) -> Result<boundary::TaskStoreUpdateTaskResponse, AtmError> {
        todo!("Phase R SQLite task-store update wiring is not implemented yet");
    }

    fn attach_message_link(
        &self,
        _request: boundary::TaskStoreAttachMessageLinkRequest,
    ) -> Result<boundary::TaskStoreAttachMessageLinkResponse, AtmError> {
        todo!("Phase R SQLite task-store attach-link wiring is not implemented yet");
    }

    fn detach_message_link(
        &self,
        _request: boundary::TaskStoreDetachMessageLinkRequest,
    ) -> Result<boundary::TaskStoreDetachMessageLinkResponse, AtmError> {
        todo!("Phase R SQLite task-store detach-link wiring is not implemented yet");
    }

    fn record_ack_transition(
        &self,
        _request: boundary::TaskStoreRecordAckTransitionRequest,
    ) -> Result<boundary::TaskStoreRecordAckTransitionResponse, AtmError> {
        todo!("Phase R SQLite task-store ack-transition wiring is not implemented yet");
    }

    fn query_task_metadata(
        &self,
        _request: boundary::TaskStoreQueryTaskMetadataRequest,
    ) -> Result<boundary::TaskStoreQueryTaskMetadataResponse, AtmError> {
        todo!("Phase R SQLite task-store metadata wiring is not implemented yet");
    }
}

#[derive(Debug, Default)]
struct SqliteRosterStore;

impl SqliteRosterStore {
    fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for SqliteRosterStore {}

impl boundary::RosterStore for SqliteRosterStore {
    fn replace_roster(
        &self,
        _request: boundary::RosterStoreReplaceRosterRequest,
    ) -> Result<boundary::RosterStoreReplaceRosterResponse, AtmError> {
        todo!("Phase R SQLite roster-store replace wiring is not implemented yet");
    }

    fn load_roster(
        &self,
        _request: boundary::RosterStoreLoadRosterRequest,
    ) -> Result<boundary::RosterStoreLoadRosterResponse, AtmError> {
        todo!("Phase R SQLite roster-store load wiring is not implemented yet");
    }

    fn query_membership(
        &self,
        _request: boundary::RosterStoreQueryMembershipRequest,
    ) -> Result<boundary::RosterStoreQueryMembershipResponse, AtmError> {
        todo!("Phase R SQLite roster-store membership wiring is not implemented yet");
    }

    fn health_snapshot(
        &self,
        _request: boundary::RosterStoreHealthSnapshotRequest,
    ) -> Result<boundary::RosterStoreHealthSnapshotResponse, AtmError> {
        todo!("Phase R SQLite roster-store health wiring is not implemented yet");
    }
}
