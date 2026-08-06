//! Replacement-owned canonical write composition.
//!
//! This module owns the two explicit blocking seams in the replacement path:
//! the injected storage-backed core write and the injected received-message
//! hook. The enclosing HTTP route remains async and awaits both operations.

use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::Arc;

use atm_core::LocalServiceRuntime;
use atm_core::api::{ApiResponse, AuthenticatedIngress, RequestDeadline};
use atm_core::boundary::AsyncMessageReceivedHookEmitter;
use atm_core::error::AtmError;
use atm_core::observability::ObservabilityPort;
use atm_core::protocol::{ResponseEnvelope, SendResponseEnvelope};
use atm_core::send::{
    WarningEntry, WriteOutcome, build_received_message_hook_dispatches_after_commit,
    prepare_write_with_runtime,
};

use crate::CanonicalWriteHandler;

/// Replacement-owned admission for synchronous SQLite write jobs.
///
/// A permit is acquired before creating the narrow blocking task. Dropping a
/// caller while it waits cancels only that admission. Once started, the job is
/// awaited to its real durable outcome; the request deadline is not reused to
/// falsely reclassify a committed transaction as a timeout.
#[derive(Clone)]
struct WriteAdmission {
    permits: Arc<tokio::sync::Semaphore>,
}

impl WriteAdmission {
    fn new(capacity: NonZeroUsize) -> Self {
        Self {
            permits: Arc::new(tokio::sync::Semaphore::new(capacity.get())),
        }
    }

    async fn run<T, F>(&self, deadline: RequestDeadline, job: F) -> Result<T, AtmError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, AtmError> + Send + 'static,
    {
        let remaining = deadline.remaining().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "request deadline expired before replacement write admission",
            )
        })?;
        let permit = tokio::time::timeout(remaining, Arc::clone(&self.permits).acquire_owned())
            .await
            .map_err(|_| {
                AtmError::daemon_unavailable(
                    "request deadline expired before replacement write admission",
                )
            })?
            .map_err(|_| {
                AtmError::daemon_unavailable("replacement write admission is shutting down")
            })?;
        if deadline.expired() {
            return Err(AtmError::daemon_unavailable(
                "request deadline expired before replacement write started",
            ));
        }
        let outcome = tokio::task::spawn_blocking(job).await.map_err(|source| {
            AtmError::new(
                atm_core::error::AtmErrorCode::InternalError,
                "replacement storage write task ended unexpectedly",
            )
            .with_cause(source)
        })?;
        drop(permit);
        outcome
    }
}

/// The replacement implementation of the canonical write operation.
///
/// Storage stays behind `LocalServiceRuntime`'s core interfaces and
/// notification stays behind the injected `AsyncMessageReceivedHookEmitter`. This
/// type has no concrete SQLite, tmux, graft, or legacy-daemon dependency.
#[derive(Clone)]
pub struct StorageAndNudgeRouter {
    service_runtime: LocalServiceRuntime,
    observability: Arc<dyn ObservabilityPort + Send + Sync>,
    received_hook: Arc<dyn AsyncMessageReceivedHookEmitter>,
    write_admission: WriteAdmission,
}

impl StorageAndNudgeRouter {
    #[must_use]
    pub fn new(
        service_runtime: LocalServiceRuntime,
        observability: Arc<dyn ObservabilityPort + Send + Sync>,
        received_hook: Arc<dyn AsyncMessageReceivedHookEmitter>,
    ) -> Self {
        Self {
            service_runtime,
            observability,
            received_hook,
            write_admission: WriteAdmission::new(NonZeroUsize::new(1).expect("one SQLite writer")),
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

    async fn emit_received_hook(
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
        let dispatches = match build_received_message_hook_dispatches_after_commit(
            &self.service_runtime,
            &request.home_dir,
            &team,
            &agent,
            message_id,
        ) {
            Ok(dispatches) => dispatches,
            Err(error) => return vec![hook_warning(error)],
        };
        let mut warnings = Vec::new();
        for dispatch in dispatches {
            let Some(remaining) = deadline.remaining() else {
                warnings.push(hook_warning(AtmError::daemon_unavailable(
                    "received-message hook was skipped because the request deadline was exhausted after persistence",
                )));
                break;
            };
            match tokio::time::timeout(
                remaining,
                self.received_hook.emit_received_message(dispatch, deadline),
            )
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => warnings.push(hook_warning(error)),
                Err(_) => warnings.push(hook_warning(AtmError::daemon_unavailable(
                    "received-message hook timed out after durable message persistence",
                ))),
            }
        }
        warnings
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
            let mut committed = self
                .write_admission
                .run(deadline, move || storage.commit_write(request))
                .await?;
            if committed.newly_persisted {
                let hook = self.clone();
                let request = committed.canonical_request.clone();
                let message_id = committed.message_id;
                let warnings = hook
                    .emit_received_hook(&request, message_id, deadline)
                    .await;
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
    use std::future::Future;
    use std::num::NonZeroUsize;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use atm_core::boundary::{
        AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, PostSendEmissionPath,
        RosterEntry, RosterHarness, RosterMemberKind,
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

    use super::{StorageAndNudgeRouter, WriteAdmission};
    use crate::{
        AuthenticatedConnector, NonZeroDuration, RuntimeLimits, RuntimeTimeouts,
        canonical_message_router,
    };

    struct RecordingReceivedHook {
        message_store: Arc<dyn MessageStore + Send + Sync>,
        emitted_ids: Mutex<Vec<AtmMessageId>>,
        saw_durable_record: AtomicBool,
        failure: Option<AtmError>,
        cancelled_on_drop: Option<Arc<AtomicBool>>,
    }

    struct CancellationMarker(Arc<AtomicBool>);

    impl Drop for CancellationMarker {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    impl atm_core::boundary::sealed::Sealed for RecordingReceivedHook {}

    impl AsyncMessageReceivedHookEmitter for RecordingReceivedHook {
        fn emit_received_message(
            &self,
            dispatch: BuiltInPostSendDispatch,
            _deadline: RequestDeadline,
        ) -> Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>>
        {
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
            let failure = self.failure.clone();
            if let Some(cancelled) = self.cancelled_on_drop.clone() {
                return Box::pin(async move {
                    let _cleanup = CancellationMarker(cancelled);
                    std::future::pending::<Result<PostSendEmissionPath, AtmError>>().await
                });
            }
            Box::pin(async move { failure.map_or(Ok(PostSendEmissionPath::GraftPort), Err) })
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

    fn fixture(
        with_recipient: bool,
        hook_failure: Option<AtmError>,
        cancelled_on_drop: Option<Arc<AtomicBool>>,
    ) -> Fixture {
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
            cancelled_on_drop,
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
        router_with_timeout(fixture, connector, Duration::from_secs(1))
    }

    fn router_with_timeout(
        fixture: &Fixture,
        connector: AuthenticatedConnector,
        request_timeout: Duration,
    ) -> axum::Router {
        canonical_message_router(
            Arc::new(fixture.router.clone()),
            connector,
            RuntimeLimits::new(
                std::num::NonZeroUsize::new(4096).expect("non-zero body limit"),
                std::num::NonZeroUsize::new(1).expect("non-zero request limit"),
            ),
            RuntimeTimeouts::new(
                NonZeroDuration::new(request_timeout).expect("non-zero request timeout"),
                NonZeroDuration::new(Duration::from_secs(1)).expect("non-zero shutdown timeout"),
            ),
        )
    }

    #[tokio::test]
    async fn write_admission_rejects_saturation_without_starting_a_second_job() {
        let admission = WriteAdmission::new(NonZeroUsize::new(1).expect("non-zero capacity"));
        let (first_started_tx, first_started_rx) = tokio::sync::oneshot::channel();
        let (release_first_tx, release_first_rx) = tokio::sync::oneshot::channel();
        let first_admission = admission.clone();
        let first = tokio::spawn(async move {
            first_admission
                .run(RequestDeadline::after(Duration::from_secs(1)), move || {
                    first_started_tx.send(()).expect("signal started job");
                    release_first_rx
                        .blocking_recv()
                        .expect("release started job");
                    Ok("first durable result")
                })
                .await
        });
        first_started_rx.await.expect("first job starts");

        let second_started = Arc::new(AtomicBool::new(false));
        let second_job_started = Arc::clone(&second_started);
        let saturated = admission
            .run(
                RequestDeadline::after(Duration::from_millis(20)),
                move || {
                    second_job_started.store(true, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await;
        assert!(
            saturated.is_err(),
            "saturated admission rejects before start"
        );
        assert!(
            !second_started.load(Ordering::SeqCst),
            "a rejected admission never creates its blocking SQLite job"
        );

        release_first_tx.send(()).expect("release first job");
        assert_eq!(
            first
                .await
                .expect("first task joins")
                .expect("first result"),
            "first durable result"
        );
    }

    #[tokio::test]
    async fn cancelled_waiter_never_starts_after_a_permit_is_released() {
        let admission = WriteAdmission::new(NonZeroUsize::new(1).expect("non-zero capacity"));
        let (first_started_tx, first_started_rx) = tokio::sync::oneshot::channel();
        let (release_first_tx, release_first_rx) = tokio::sync::oneshot::channel();
        let first_admission = admission.clone();
        let first = tokio::spawn(async move {
            first_admission
                .run(RequestDeadline::after(Duration::from_secs(1)), move || {
                    first_started_tx.send(()).expect("signal started job");
                    release_first_rx
                        .blocking_recv()
                        .expect("release started job");
                    Ok(())
                })
                .await
        });
        first_started_rx.await.expect("first job starts");

        let cancelled_job_started = Arc::new(AtomicBool::new(false));
        let cancelled_job_flag = Arc::clone(&cancelled_job_started);
        let waiting_admission = admission.clone();
        let waiting = tokio::spawn(async move {
            waiting_admission
                .run(RequestDeadline::after(Duration::from_secs(1)), move || {
                    cancelled_job_flag.store(true, Ordering::SeqCst);
                    Ok(())
                })
                .await
        });
        tokio::task::yield_now().await;
        waiting.abort();
        assert!(
            waiting
                .await
                .expect_err("waiter is cancelled")
                .is_cancelled()
        );

        release_first_tx.send(()).expect("release first job");
        first
            .await
            .expect("first task joins")
            .expect("first durable result");
        assert!(
            !cancelled_job_started.load(Ordering::SeqCst),
            "cancelling while queued removes the job before it can start"
        );
    }

    #[tokio::test]
    async fn started_write_retains_its_actual_result_after_the_advisory_deadline() {
        let admission = WriteAdmission::new(NonZeroUsize::new(1).expect("non-zero capacity"));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            admission
                .run(
                    RequestDeadline::after(Duration::from_millis(20)),
                    move || {
                        started_tx.send(()).expect("signal started job");
                        release_rx.blocking_recv().expect("release started job");
                        Ok("actual durable result")
                    },
                )
                .await
        });
        started_rx.await.expect("job starts");
        tokio::time::sleep(Duration::from_millis(30)).await;
        release_tx.send(()).expect("release started job");
        assert_eq!(
            task.await.expect("task joins").expect("actual result"),
            "actual durable result",
            "a started transaction is not reclassified as a deadline failure"
        );
    }

    #[tokio::test]
    async fn write_admission_returns_the_underlying_storage_error() {
        let admission = WriteAdmission::new(NonZeroUsize::new(1).expect("non-zero capacity"));
        let error = admission
            .run(RequestDeadline::after(Duration::from_secs(1)), || {
                Err::<(), _>(AtmError::validation("intentional storage failure"))
            })
            .await
            .expect_err("storage failure is preserved");
        assert!(
            error.message().contains("intentional storage failure"),
            "the storage error is returned unchanged instead of being replaced by an admission error"
        );
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
        let fixture = fixture(true, None, None);
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
        let fixture = fixture(false, None, None);
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
        let fixture = fixture(true, None, None);
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
        let fixture = fixture(true, None, None);
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
            None,
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
                .is_some_and(|message| message.contains("receiver hook did not run")),
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

    #[tokio::test]
    async fn axum_route_hook_timeout_returns_success_warning_and_cancels_hook_future() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let fixture = fixture(true, None, Some(Arc::clone(&cancelled)));
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone());
        let response = post_write(
            router_with_timeout(
                &fixture,
                AuthenticatedConnector::local(),
                Duration::from_millis(200),
            ),
            &write,
        )
        .await;

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("send outcome JSON");
        assert_eq!(value["warnings"].as_array().map(Vec::len), Some(1));
        assert!(
            value["warnings"][0]["message"]
                .as_str()
                .is_some_and(|message| message.contains("timed out")),
            "timed-out hook is advisory after the durable write"
        );
        assert!(
            cancelled.load(Ordering::SeqCst),
            "deadline cancellation drops the hook future instead of leaving detached work"
        );
    }
}
