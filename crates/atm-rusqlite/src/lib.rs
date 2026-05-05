#![forbid(unsafe_code)]
#![allow(dead_code)]

//! Skeleton crate for Phase R rusqlite adapter work.

use atm_core::boundary;
use atm_core::error::AtmError;
use std::error::Error as StdError;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SqliteBoundaryStubError {
    Mail,
    Task,
    Roster,
}

impl fmt::Display for SqliteBoundaryStubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mail => f.write_str("sqlite mail-store scaffold is not wired"),
            Self::Task => f.write_str("sqlite task-store scaffold is not wired"),
            Self::Roster => f.write_str("sqlite roster-store scaffold is not wired"),
        }
    }
}

impl StdError for SqliteBoundaryStubError {}

fn sqlite_boundary_stub_error(message: &'static str, source: SqliteBoundaryStubError) -> AtmError {
    AtmError::validation(message)
        .with_recovery("Complete the Phase R sqlite boundary wiring before invoking this path.")
        .with_source(source)
}

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
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite mail-store bootstrap stub is not implemented yet",
            SqliteBoundaryStubError::Mail,
        ))
    }

    fn run_transaction(
        &self,
        _request: boundary::MailStoreTransactionRequest,
    ) -> Result<boundary::MailStoreTransactionResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite mail-store transaction stub is not implemented yet",
            SqliteBoundaryStubError::Mail,
        ))
    }

    fn upsert_message(
        &self,
        _request: boundary::MailStoreUpsertMessageRequest,
    ) -> Result<boundary::MailStoreUpsertMessageResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite mail-store message upsert stub is not implemented yet",
            SqliteBoundaryStubError::Mail,
        ))
    }

    fn load_message(
        &self,
        _request: boundary::MailStoreLoadMessageRequest,
    ) -> Result<boundary::MailStoreLoadMessageResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite mail-store message load stub is not implemented yet",
            SqliteBoundaryStubError::Mail,
        ))
    }

    fn upsert_visibility_state(
        &self,
        _request: boundary::MailStoreUpsertVisibilityStateRequest,
    ) -> Result<boundary::MailStoreUpsertVisibilityStateResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite mail-store visibility upsert stub is not implemented yet",
            SqliteBoundaryStubError::Mail,
        ))
    }

    fn load_visibility_state(
        &self,
        _request: boundary::MailStoreLoadVisibilityStateRequest,
    ) -> Result<boundary::MailStoreLoadVisibilityStateResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite mail-store visibility load stub is not implemented yet",
            SqliteBoundaryStubError::Mail,
        ))
    }

    fn record_ingest_replay_state(
        &self,
        _request: boundary::MailStoreRecordIngestReplayStateRequest,
    ) -> Result<boundary::MailStoreRecordIngestReplayStateResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite mail-store replay-state record stub is not implemented yet",
            SqliteBoundaryStubError::Mail,
        ))
    }

    fn load_ingest_replay_state(
        &self,
        _request: boundary::MailStoreLoadIngestReplayStateRequest,
    ) -> Result<boundary::MailStoreLoadIngestReplayStateResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite mail-store replay-state load stub is not implemented yet",
            SqliteBoundaryStubError::Mail,
        ))
    }

    fn health_snapshot(
        &self,
        _request: boundary::MailStoreHealthSnapshotRequest,
    ) -> Result<boundary::MailStoreHealthSnapshotResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite mail-store health stub is not implemented yet",
            SqliteBoundaryStubError::Mail,
        ))
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
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite task-store create stub is not implemented yet",
            SqliteBoundaryStubError::Task,
        ))
    }

    fn load_task(
        &self,
        _request: boundary::TaskStoreLoadTaskRequest,
    ) -> Result<boundary::TaskStoreLoadTaskResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite task-store load stub is not implemented yet",
            SqliteBoundaryStubError::Task,
        ))
    }

    fn update_task(
        &self,
        _request: boundary::TaskStoreUpdateTaskRequest,
    ) -> Result<boundary::TaskStoreUpdateTaskResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite task-store update stub is not implemented yet",
            SqliteBoundaryStubError::Task,
        ))
    }

    fn attach_message_link(
        &self,
        _request: boundary::TaskStoreAttachMessageLinkRequest,
    ) -> Result<boundary::TaskStoreAttachMessageLinkResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite task-store attach-link stub is not implemented yet",
            SqliteBoundaryStubError::Task,
        ))
    }

    fn detach_message_link(
        &self,
        _request: boundary::TaskStoreDetachMessageLinkRequest,
    ) -> Result<boundary::TaskStoreDetachMessageLinkResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite task-store detach-link stub is not implemented yet",
            SqliteBoundaryStubError::Task,
        ))
    }

    fn record_ack_transition(
        &self,
        _request: boundary::TaskStoreRecordAckTransitionRequest,
    ) -> Result<boundary::TaskStoreRecordAckTransitionResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite task-store ack-transition stub is not implemented yet",
            SqliteBoundaryStubError::Task,
        ))
    }

    fn query_task_metadata(
        &self,
        _request: boundary::TaskStoreQueryTaskMetadataRequest,
    ) -> Result<boundary::TaskStoreQueryTaskMetadataResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite task-store metadata stub is not implemented yet",
            SqliteBoundaryStubError::Task,
        ))
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
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite roster-store replace stub is not implemented yet",
            SqliteBoundaryStubError::Roster,
        ))
    }

    fn load_roster(
        &self,
        _request: boundary::RosterStoreLoadRosterRequest,
    ) -> Result<boundary::RosterStoreLoadRosterResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite roster-store load stub is not implemented yet",
            SqliteBoundaryStubError::Roster,
        ))
    }

    fn query_membership(
        &self,
        _request: boundary::RosterStoreQueryMembershipRequest,
    ) -> Result<boundary::RosterStoreQueryMembershipResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite roster-store membership stub is not implemented yet",
            SqliteBoundaryStubError::Roster,
        ))
    }

    fn health_snapshot(
        &self,
        _request: boundary::RosterStoreHealthSnapshotRequest,
    ) -> Result<boundary::RosterStoreHealthSnapshotResponse, AtmError> {
        Err(sqlite_boundary_stub_error(
            "Phase R SQLite roster-store health stub is not implemented yet",
            SqliteBoundaryStubError::Roster,
        ))
    }
}
