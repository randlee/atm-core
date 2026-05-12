use std::sync::Arc;

use crate::boundary;
use crate::error::AtmError;

#[derive(Debug, Default)]
pub(crate) struct UnsupportedMailStoreAdapter;

#[derive(Debug, Default)]
pub(crate) struct UnsupportedTaskStoreAdapter;

#[derive(Debug, Default)]
pub(crate) struct UnsupportedRosterStoreAdapter;

pub(crate) fn unsupported_mail_store() -> Arc<dyn boundary::MailStore + Send + Sync> {
    Arc::new(UnsupportedMailStoreAdapter)
}

pub(crate) fn unsupported_task_store() -> Arc<dyn boundary::TaskStore + Send + Sync> {
    Arc::new(UnsupportedTaskStoreAdapter)
}

pub(crate) fn unsupported_roster_store() -> Arc<dyn boundary::RosterStore + Send + Sync> {
    Arc::new(UnsupportedRosterStoreAdapter)
}

fn unsupported(what: &str) -> AtmError {
    AtmError::validation(format!(
        "retained runtime placeholder adapter does not implement {what}"
    ))
    .with_recovery(
        "Use the owning Phase R adapter crate for this boundary operation instead of the retained runtime placeholder.",
    )
}

impl boundary::sealed::Sealed for UnsupportedMailStoreAdapter {}

impl boundary::MailStore for UnsupportedMailStoreAdapter {
    fn bootstrap(
        &self,
        request: boundary::MailStoreBootstrapRequest,
    ) -> Result<boundary::MailStoreBootstrapResponse, AtmError> {
        Ok(boundary::MailStoreBootstrapResponse {
            team: request.team,
            bootstrapped: true,
            opened: true,
        })
    }

    fn run_transaction(
        &self,
        request: boundary::MailStoreTransactionRequest,
    ) -> Result<boundary::MailStoreTransactionResponse, AtmError> {
        Ok(boundary::MailStoreTransactionResponse {
            team: request.team,
            committed: true,
            operations_executed: request.requested_operations.len(),
        })
    }

    fn upsert_message(
        &self,
        request: boundary::MailStoreUpsertMessageRequest,
    ) -> Result<boundary::MailStoreUpsertMessageResponse, AtmError> {
        Ok(boundary::MailStoreUpsertMessageResponse {
            record: request.record,
            inserted: true,
        })
    }

    fn load_message(
        &self,
        _request: boundary::MailStoreLoadMessageRequest,
    ) -> Result<boundary::MailStoreLoadMessageResponse, AtmError> {
        Err(unsupported("MailStore::load_message"))
    }

    fn query_mailbox_metadata(
        &self,
        _request: boundary::MailStoreQueryMailboxMetadataRequest,
    ) -> Result<boundary::MailStoreQueryMailboxMetadataResponse, AtmError> {
        Err(unsupported("MailStore::query_mailbox_metadata"))
    }

    fn query_mailbox_metadata_counts(
        &self,
        _request: boundary::MailStoreQueryMailboxMetadataCountsRequest,
    ) -> Result<boundary::MailStoreQueryMailboxMetadataCountsResponse, AtmError> {
        Err(unsupported("MailStore::query_mailbox_metadata_counts"))
    }

    fn upsert_message_state(
        &self,
        request: boundary::UpsertMailMessageStateRequest,
    ) -> Result<boundary::UpsertMailMessageStateResponse, AtmError> {
        Ok(boundary::UpsertMailMessageStateResponse {
            state: request.state,
        })
    }

    fn load_message_state(
        &self,
        _request: boundary::LoadMailMessageStateRequest,
    ) -> Result<boundary::LoadMailMessageStateResponse, AtmError> {
        Ok(boundary::LoadMailMessageStateResponse { state: None })
    }

    fn record_ingest_replay_state(
        &self,
        request: boundary::MailStoreRecordIngestReplayStateRequest,
    ) -> Result<boundary::MailStoreRecordIngestReplayStateResponse, AtmError> {
        Ok(boundary::MailStoreRecordIngestReplayStateResponse {
            state: request.state,
        })
    }

    fn load_ingest_replay_state(
        &self,
        _request: boundary::MailStoreLoadIngestReplayStateRequest,
    ) -> Result<boundary::MailStoreLoadIngestReplayStateResponse, AtmError> {
        Ok(boundary::MailStoreLoadIngestReplayStateResponse { state: None })
    }

    fn health_snapshot(
        &self,
        request: boundary::MailStoreHealthSnapshotRequest,
    ) -> Result<boundary::MailStoreHealthSnapshotResponse, AtmError> {
        Ok(boundary::MailStoreHealthSnapshotResponse {
            snapshot: boundary::MailStoreHealthSnapshot {
                team: request.team,
                agent: request.agent,
                total_messages: 0,
                pending_ack_messages: 0,
                read_messages: 0,
                latest_message_timestamp: None,
            },
        })
    }
}

impl boundary::sealed::Sealed for UnsupportedTaskStoreAdapter {}

impl boundary::TaskStore for UnsupportedTaskStoreAdapter {
    fn create_task(
        &self,
        request: boundary::TaskStoreCreateTaskRequest,
    ) -> Result<boundary::TaskStoreCreateTaskResponse, AtmError> {
        Ok(boundary::TaskStoreCreateTaskResponse {
            record: request.record,
        })
    }

    fn load_task(
        &self,
        _request: boundary::TaskStoreLoadTaskRequest,
    ) -> Result<boundary::TaskStoreLoadTaskResponse, AtmError> {
        Ok(boundary::TaskStoreLoadTaskResponse { record: None })
    }

    fn update_task(
        &self,
        _request: boundary::TaskStoreUpdateTaskRequest,
    ) -> Result<boundary::TaskStoreUpdateTaskResponse, AtmError> {
        Err(unsupported("TaskStore::update_task"))
    }

    fn attach_message_link(
        &self,
        _request: boundary::TaskStoreAttachMessageLinkRequest,
    ) -> Result<boundary::TaskStoreAttachMessageLinkResponse, AtmError> {
        Err(unsupported("TaskStore::attach_message_link"))
    }

    fn detach_message_link(
        &self,
        _request: boundary::TaskStoreDetachMessageLinkRequest,
    ) -> Result<boundary::TaskStoreDetachMessageLinkResponse, AtmError> {
        Err(unsupported("TaskStore::detach_message_link"))
    }

    fn record_ack_transition(
        &self,
        _request: boundary::TaskStoreRecordAckTransitionRequest,
    ) -> Result<boundary::TaskStoreRecordAckTransitionResponse, AtmError> {
        Err(unsupported("TaskStore::record_ack_transition"))
    }

    fn query_task_metadata(
        &self,
        _request: boundary::TaskStoreQueryTaskMetadataRequest,
    ) -> Result<boundary::TaskStoreQueryTaskMetadataResponse, AtmError> {
        Ok(boundary::TaskStoreQueryTaskMetadataResponse {
            records: Vec::new(),
        })
    }
}

impl boundary::sealed::Sealed for UnsupportedRosterStoreAdapter {}

impl boundary::RosterStore for UnsupportedRosterStoreAdapter {
    fn replace_roster(
        &self,
        request: boundary::RosterStoreReplaceRosterRequest,
    ) -> Result<boundary::RosterStoreReplaceRosterResponse, AtmError> {
        Ok(boundary::RosterStoreReplaceRosterResponse {
            team: request.team,
            previous_member_count: 0,
            current_member_count: request.members.len() as u64,
            replaced: true,
        })
    }

    fn load_roster(
        &self,
        request: boundary::RosterStoreLoadRosterRequest,
    ) -> Result<boundary::RosterStoreLoadRosterResponse, AtmError> {
        Ok(boundary::RosterStoreLoadRosterResponse {
            team: request.team,
            members: Vec::new(),
        })
    }

    fn query_membership(
        &self,
        request: boundary::RosterStoreQueryMembershipRequest,
    ) -> Result<boundary::RosterStoreQueryMembershipResponse, AtmError> {
        Ok(boundary::RosterStoreQueryMembershipResponse {
            team: request.team,
            member: None,
            is_member: false,
        })
    }

    fn health_snapshot(
        &self,
        request: boundary::RosterStoreHealthSnapshotRequest,
    ) -> Result<boundary::RosterStoreHealthSnapshotResponse, AtmError> {
        Ok(boundary::RosterStoreHealthSnapshotResponse {
            snapshot: boundary::RosterStoreHealthSnapshot {
                team: request.team,
                member_count: 0,
                stale: false,
                refreshed_at: None,
            },
        })
    }
}
