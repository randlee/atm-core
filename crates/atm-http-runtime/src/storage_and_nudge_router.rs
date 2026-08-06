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

    use atm_core::boundary::{
        BuiltInPostSendDispatch, MessageReceivedHookEmitter, PostSendEmissionPath, RosterEntry,
        RosterHarness, RosterMemberKind,
    };
    use atm_core::observability::NullObservability;
    use atm_core::schema::AtmMessageId;
    use atm_core::send::{SendMessageSource, WriteRequest};
    use atm_core::types::{AgentName, ModelName, TeamName};
    use atm_core::{RequestDeadline, error::AtmError};
    use atm_runtime_test_support::{hold_sqlite_writer_lock, open_sqlite_boundary};
    use atm_storage::{MessageKey, MessageQuery, MessageStore, RosterSnapshot};
    use axum::body::{Body, to_bytes};
    use axum::http::header::CONTENT_TYPE;
    use axum::http::{Request, StatusCode};
    use tempfile::TempDir;
    use tower::ServiceExt;

    use super::StorageAndNudgeRouter;
    use crate::{
        AuthenticatedConnector, NonZeroDuration, RuntimeLimits, RuntimeTimeouts,
        canonical_message_router,
    };

    struct RecordingReceivedHook {
        message_store: Arc<dyn MessageStore + Send + Sync>,
        emitted_ids: Mutex<Vec<AtmMessageId>>,
        saw_durable_record: AtomicBool,
        failure: Option<AtmError>,
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
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            Ok(PostSendEmissionPath::GraftPort)
        }
    }

    struct Fixture {
        _temporary_root: TempDir,
        router: StorageAndNudgeRouter,
        message_store: Arc<dyn MessageStore + Send + Sync>,
        received_hook: Arc<RecordingReceivedHook>,
        database_path: PathBuf,
        home_dir: PathBuf,
        current_dir: PathBuf,
    }

    fn fixture(with_recipient: bool, hook_failure: Option<AtmError>) -> Fixture {
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
            failure: hook_failure,
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
            database_path,
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

    fn router(fixture: &Fixture, connector: AuthenticatedConnector) -> axum::Router {
        canonical_message_router(
            Arc::new(fixture.router.clone()),
            connector,
            RuntimeLimits::new(
                std::num::NonZeroUsize::new(4096).expect("non-zero body limit"),
                std::num::NonZeroUsize::new(1).expect("non-zero request limit"),
            ),
            RuntimeTimeouts::new(
                NonZeroDuration::new(Duration::from_secs(1)).expect("non-zero request timeout"),
                NonZeroDuration::new(Duration::from_secs(1)).expect("non-zero shutdown timeout"),
            ),
        )
    }

    async fn post_write(app: axum::Router, write: &WriteRequest) -> axum::response::Response {
        let path = atm_core::api::http_route_surface()
            .find(|route| route.method == "POST" && route.path_template.ends_with("/messages"))
            .expect("core write route")
            .path_template;
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(write).expect("serialize write request"),
                ))
                .expect("HTTP request"),
        )
        .await
        .expect("infallible Axum service")
    }

    #[tokio::test]
    async fn axum_route_persists_before_emitting_one_received_hook() {
        let fixture = fixture(true, None);
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone());
        let response = post_write(router(&fixture, AuthenticatedConnector::local()), &write).await;
        assert_eq!(response.status(), StatusCode::CREATED);

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
    async fn axum_route_rejected_write_emits_no_hook_and_persists_no_message() {
        let fixture = fixture(false, None);
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone());
        let response = post_write(router(&fixture, AuthenticatedConnector::local()), &write).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
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

    #[tokio::test]
    async fn axum_route_storage_failure_emits_no_hook_and_persists_no_message() {
        let fixture = fixture(true, None);
        let writer_lock =
            hold_sqlite_writer_lock(&fixture.database_path).expect("hold SQLite writer lock");
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone());
        let response = post_write(router(&fixture, AuthenticatedConnector::local()), &write).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            fixture
                .received_hook
                .emitted_ids
                .lock()
                .expect("inspect received-hook emissions")
                .is_empty(),
            "storage failure must not emit a receiver hook"
        );
        drop(writer_lock);
        assert!(
            fixture
                .message_store
                .list_messages(&MessageQuery {
                    team: "test-team".parse().expect("team"),
                    agent: "recipient".parse().expect("agent"),
                    sender: None,
                    task_id: None,
                    limit: None,
                })
                .expect("list recipient mailbox")
                .is_empty(),
            "storage failure must not persist a mailbox record"
        );
    }

    #[tokio::test]
    async fn axum_route_idempotent_duplicate_skips_the_second_received_hook() {
        let fixture = fixture(true, None);
        let message_id = AtmMessageId::new();
        let mut write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone())
            .with_origin_metadata(message_id, atm_core::types::IsoTimestamp::now());
        write.to = Some(
            "recipient@test-team.localhost"
                .parse()
                .expect("same-host target"),
        );

        let origin = post_write(router(&fixture, AuthenticatedConnector::local()), &write).await;
        assert_eq!(origin.status(), StatusCode::CREATED);
        let receipt = post_write(
            router(
                &fixture,
                AuthenticatedConnector::peer("localhost".parse().expect("source host")),
            ),
            &write,
        )
        .await;
        assert_eq!(receipt.status(), StatusCode::CREATED);
        assert_eq!(
            fixture
                .received_hook
                .emitted_ids
                .lock()
                .expect("inspect received-hook emissions")
                .as_slice(),
            &[message_id],
            "idempotent peer receipt must not emit a second receiver hook"
        );
    }

    #[tokio::test]
    async fn axum_route_hook_failure_returns_durable_success_with_warning() {
        let fixture = fixture(
            true,
            Some(AtmError::daemon_unavailable(
                "intentional received-hook failure",
            )),
        );
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone());
        let response = post_write(router(&fixture, AuthenticatedConnector::local()), &write).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("send outcome JSON");
        assert_eq!(
            value["warnings"].as_array().map(Vec::len),
            Some(1),
            "hook failure is represented as one existing-schema warning"
        );
        assert!(
            value["warnings"][0]["message"]
                .as_str()
                .is_some_and(|message| message.contains("post-send emission failed")),
            "warning identifies the advisory receiver-hook failure"
        );
        let emitted_ids = fixture
            .received_hook
            .emitted_ids
            .lock()
            .expect("inspect received-hook emissions")
            .clone();
        assert_eq!(emitted_ids.len(), 1);
        assert!(
            fixture
                .message_store
                .load_message(&MessageKey::from(emitted_ids[0]))
                .expect("load committed message")
                .is_some(),
            "hook failure cannot roll back a durable receive"
        );
    }
}
