//! Replacement-owned canonical write composition.
//!
//! This module owns the two explicit blocking seams in the replacement path:
//! the injected storage-backed core write and the injected received-message
//! hook. The enclosing HTTP route remains async and awaits both operations.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use atm_core::LocalServiceRuntime;
use atm_core::api::{ApiResponse, AuthenticatedIngress, RequestDeadline};
use atm_core::boundary::MessageReceivedHookEmitter;
use atm_core::error::AtmError;
use atm_core::observability::ObservabilityPort;
use atm_core::protocol::{ResponseEnvelope, SendResponseEnvelope};
use atm_core::send::{
    WarningEntry, WriteOutcome, emit_received_message_after_commit, prepare_write_with_runtime,
};

use crate::CanonicalWriteHandler;

/// The replacement implementation of the canonical write operation.
///
/// Storage stays behind `LocalServiceRuntime`'s core interfaces and
/// notification stays behind the injected `MessageReceivedHookEmitter`. This
/// type has no concrete SQLite, tmux, graft, or legacy-daemon dependency.
#[derive(Clone)]
pub struct StorageAndNudgeRouter {
    service_runtime: LocalServiceRuntime,
    observability: Arc<dyn ObservabilityPort + Send + Sync>,
    received_hook: Arc<dyn MessageReceivedHookEmitter>,
}

impl StorageAndNudgeRouter {
    #[must_use]
    pub fn new(
        service_runtime: LocalServiceRuntime,
        observability: Arc<dyn ObservabilityPort + Send + Sync>,
        received_hook: Arc<dyn MessageReceivedHookEmitter>,
    ) -> Self {
        Self {
            service_runtime,
            observability,
            received_hook,
        }
    }

    fn commit_write(
        &self,
        request: atm_core::send::WriteRequest,
    ) -> Result<CommittedWrite, AtmError> {
        let mut prepared = prepare_write_with_runtime(
            request,
            self.observability.as_ref(),
            &self.service_runtime,
        )?;
        let newly_persisted = prepared.is_newly_persisted();
        let canonical_request = prepared.outbound_request();
        let message_id = prepared.persisted_message_id();
        let outcome = prepared.finish(&self.service_runtime, self.observability.as_ref())?;
        Ok(CommittedWrite {
            outcome,
            canonical_request,
            message_id,
            newly_persisted,
        })
    }

    fn emit_received_hook(
        &self,
        request: &atm_core::send::WriteRequest,
        message_id: atm_core::schema::AtmMessageId,
        deadline: RequestDeadline,
    ) -> Vec<WarningEntry> {
        if deadline.expired() {
            return vec![hook_warning(AtmError::daemon_unavailable(
                "received-message hook was skipped because the request deadline was exhausted after persistence",
            ))];
        }
        let Some(target) = request.to.as_ref() else {
            return vec![hook_warning(AtmError::validation(
                "durably received message had no canonical destination for receiver hook",
            ))];
        };
        let team = target
            .team()
            .cloned()
            .unwrap_or_else(|| request.caller_team.clone());
        let agent = target.agent().clone();
        match emit_received_message_after_commit(
            &self.service_runtime,
            &request.home_dir,
            &team,
            &agent,
            message_id,
            deadline,
            Some(self.received_hook.as_ref()),
        ) {
            Ok(warnings) => warnings,
            Err(error) => vec![hook_warning(error)],
        }
    }
}

struct CommittedWrite {
    outcome: WriteOutcome,
    canonical_request: atm_core::send::WriteRequest,
    message_id: atm_core::schema::AtmMessageId,
    newly_persisted: bool,
}

impl CanonicalWriteHandler for StorageAndNudgeRouter {
    fn write(
        &self,
        request: atm_core::send::WriteRequest,
        _ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<ApiResponse, AtmError>> + Send + '_>> {
        Box::pin(async move {
            if deadline.expired() {
                return Err(AtmError::daemon_unavailable(
                    "request deadline expired before replacement write admission",
                ));
            }
            let storage = self.clone();
            let mut committed = tokio::task::spawn_blocking(move || storage.commit_write(request))
                .await
                .map_err(|source| {
                    AtmError::new(
                        atm_core::error::AtmErrorCode::InternalError,
                        "replacement storage write task ended unexpectedly",
                    )
                    .with_cause(source)
                })??;
            if committed.newly_persisted {
                let hook = self.clone();
                let request = committed.canonical_request.clone();
                let message_id = committed.message_id;
                let warnings = tokio::task::spawn_blocking(move || {
                    hook.emit_received_hook(&request, message_id, deadline)
                })
                .await
                .map_err(|source| {
                    AtmError::new(
                        atm_core::error::AtmErrorCode::InternalError,
                        "replacement received-message hook task ended unexpectedly",
                    )
                    .with_cause(source)
                })?;
                append_warnings(&mut committed.outcome, warnings);
            }
            Ok(ApiResponse::new(write_response(committed.outcome)))
        })
    }
}

fn append_warnings(outcome: &mut WriteOutcome, warnings: Vec<WarningEntry>) {
    match outcome {
        WriteOutcome::Sent(outcome) => outcome.warnings.extend(warnings),
        WriteOutcome::Acknowledged(outcome) => outcome.warnings.extend(warnings),
    }
}

fn write_response(outcome: WriteOutcome) -> ResponseEnvelope {
    match outcome {
        WriteOutcome::Sent(outcome) => ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)),
        WriteOutcome::Acknowledged(outcome) => {
            ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome))
        }
    }
}

fn hook_warning(error: AtmError) -> WarningEntry {
    WarningEntry::with_code(
        error.code(),
        format!("message received successfully, but its receiver hook did not run: {error}"),
        Some("inspect the receiver hook endpoint or harness, then continue normally"),
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use atm_core::api::AuthenticatedIngress;
    use atm_core::boundary::{
        BuiltInPostSendDispatch, MessageReceivedHookEmitter, PostSendEmissionPath, RosterEntry,
        RosterHarness, RosterMemberKind,
    };
    use atm_core::observability::NullObservability;
    use atm_core::schema::AtmMessageId;
    use atm_core::send::{SendMessageSource, WriteRequest};
    use atm_core::types::{AgentName, ModelName, TeamName};
    use atm_core::{RequestDeadline, error::AtmError};
    use atm_runtime_test_support::open_sqlite_boundary;
    use atm_storage::{MessageKey, MessageQuery, MessageStore, RosterSnapshot};
    use tempfile::TempDir;

    use super::{CanonicalWriteHandler, StorageAndNudgeRouter};

    struct RecordingReceivedHook {
        message_store: Arc<dyn MessageStore + Send + Sync>,
        emitted_ids: Mutex<Vec<AtmMessageId>>,
        saw_durable_record: AtomicBool,
    }

    impl atm_core::boundary::sealed::Sealed for RecordingReceivedHook {}

    impl MessageReceivedHookEmitter for RecordingReceivedHook {
        fn emit_received_message(
            &self,
            dispatch: &BuiltInPostSendDispatch,
            _deadline: RequestDeadline,
        ) -> Result<PostSendEmissionPath, AtmError> {
            let key = MessageKey::from(dispatch.event.message_id);
            self.saw_durable_record.store(
                self.message_store
                    .load_message(&key)
                    .expect("load durable message while emitting hook")
                    .is_some(),
                Ordering::SeqCst,
            );
            self.emitted_ids
                .lock()
                .expect("record received hook emission")
                .push(dispatch.event.message_id);
            Ok(PostSendEmissionPath::GraftPort)
        }
    }

    struct Fixture {
        _temporary_root: TempDir,
        router: StorageAndNudgeRouter,
        message_store: Arc<dyn MessageStore + Send + Sync>,
        received_hook: Arc<RecordingReceivedHook>,
        home_dir: PathBuf,
        current_dir: PathBuf,
    }

    fn fixture(with_recipient: bool) -> Fixture {
        let temporary_root = tempfile::tempdir().expect("temporary runtime root");
        let database_path = temporary_root.path().join("mail.sqlite");
        let assembly = open_sqlite_boundary(&database_path).expect("assemble SQLite boundary");
        let team: TeamName = "test-team".parse().expect("team");
        if with_recipient {
            assembly
                .shared_roster_store_arc()
                .save_roster(&RosterSnapshot {
                    team_name: team.clone(),
                    members: vec![RosterEntry {
                        team_name: team.clone(),
                        agent_name: "recipient".parse().expect("agent"),
                        member_kind: RosterMemberKind::Permanent,
                        harness: RosterHarness::PythonGraft,
                        agent_type: atm_core::schema::AgentType::default(),
                        model: ModelName::default(),
                        recipient_pane_id: None,
                        metadata_json: serde_json::Map::new(),
                    }],
                    refreshed_at: None,
                })
                .expect("seed recipient roster");
        }
        let message_store = assembly.message_store_arc();
        let received_hook = Arc::new(RecordingReceivedHook {
            message_store: Arc::clone(&message_store),
            emitted_ids: Mutex::new(Vec::new()),
            saw_durable_record: AtomicBool::new(false),
        });
        let home_dir = temporary_root.path().join("home");
        let current_dir = temporary_root.path().join("workspace");
        fs::create_dir_all(&home_dir).expect("create fixture home");
        fs::create_dir_all(&current_dir).expect("create fixture workspace");
        let router = StorageAndNudgeRouter::new(
            assembly.service_runtime,
            Arc::new(NullObservability),
            received_hook.clone(),
        );
        Fixture {
            _temporary_root: temporary_root,
            router,
            message_store,
            received_hook,
            home_dir,
            current_dir,
        }
    }

    fn write_request(home_dir: PathBuf, current_dir: PathBuf) -> WriteRequest {
        WriteRequest::new(
            home_dir,
            current_dir,
            "sender".parse::<AgentName>().expect("sender"),
            "recipient@test-team",
            "test-team".parse().expect("caller team"),
            SendMessageSource::Inline("router direct path fixture".to_owned()),
            None,
            false,
            None,
            false,
        )
        .expect("write request")
    }

    #[tokio::test]
    async fn newly_persisted_write_is_durable_before_one_received_hook_emission() {
        let fixture = fixture(true);
        fixture
            .router
            .write(
                write_request(fixture.home_dir.clone(), fixture.current_dir.clone()),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("new write succeeds");

        let emitted_ids = fixture
            .received_hook
            .emitted_ids
            .lock()
            .expect("inspect received-hook emissions")
            .clone();
        assert_eq!(emitted_ids.len(), 1, "new durable write emits one hook");
        assert!(
            fixture
                .received_hook
                .saw_durable_record
                .load(Ordering::SeqCst),
            "the hook observes the message only after durable persistence"
        );
        assert!(
            fixture
                .message_store
                .load_message(&MessageKey::from(emitted_ids[0]))
                .expect("load emitted message")
                .is_some(),
            "the emitted message remains durable after the write response"
        );
    }

    #[tokio::test]
    async fn rejected_write_emits_no_hook_and_persists_no_message() {
        let fixture = fixture(false);
        let result = fixture
            .router
            .write(
                write_request(fixture.home_dir.clone(), fixture.current_dir.clone()),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await;

        assert!(
            result.is_err(),
            "unknown recipient is rejected before persistence"
        );
        assert!(
            fixture
                .received_hook
                .emitted_ids
                .lock()
                .expect("inspect received-hook emissions")
                .is_empty(),
            "rejected write does not emit a receiver hook"
        );
        let messages = fixture
            .message_store
            .list_messages(&MessageQuery {
                team: "test-team".parse().expect("team"),
                agent: "recipient".parse().expect("agent"),
                sender: None,
                task_id: None,
                limit: None,
            })
            .expect("list recipient mailbox");
        assert!(
            messages.is_empty(),
            "rejected write persists no mailbox record"
        );
    }
}
