//! Replacement-owned canonical write composition.
//!
//! This module owns the two explicit blocking seams in the replacement path:
//! the injected storage-backed core write and the injected received-message
//! hook. The enclosing HTTP route remains async and awaits both operations.

use std::future::Future;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use atm_core::LocalServiceRuntime;
use atm_core::api::{ApiRequest, ApiResponse, AuthenticatedIngress, RequestDeadline};
use atm_core::boundary::MessageReceivedHookSelector;
use atm_core::clear::ClearQuery;
use atm_core::doctor::DoctorQuery;
use atm_core::error::AtmError;
use atm_core::list::ListQuery;
use atm_core::observability::ObservabilityPort;
use atm_core::protocol::{
    CompatibilityVerdict, ReleaseVersion, ResponseEnvelope, SendResponseEnvelope,
};
use atm_core::read::{PeekQuery, ReadQuery};
use atm_core::send::{
    WarningEntry, WriteOutcome, build_received_message_hook_dispatches_after_commit,
    prepare_write_with_runtime,
};

use crate::CanonicalWriteHandler;
use crate::RuntimeHealth;

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
    received_hook_selector: Arc<dyn MessageReceivedHookSelector>,
    write_admission: WriteAdmission,
    daemon_home: PathBuf,
    runtime_health: RuntimeHealth,
    doctor_ports: Option<atm_core::doctor::RuntimeDoctorPorts>,
}

impl StorageAndNudgeRouter {
    #[must_use]
    pub fn new(
        service_runtime: LocalServiceRuntime,
        observability: Arc<dyn ObservabilityPort + Send + Sync>,
        received_hook_selector: Arc<dyn MessageReceivedHookSelector>,
        daemon_home: PathBuf,
    ) -> Self {
        Self {
            service_runtime,
            observability,
            received_hook_selector,
            write_admission: WriteAdmission::new(NonZeroUsize::new(1).expect("one SQLite writer")),
            daemon_home,
            runtime_health: RuntimeHealth::default(),
            doctor_ports: None,
        }
    }

    /// Adds the existing core doctor ports and the process-owned lifecycle
    /// projection. Both stay behind core interfaces; the HTTP runtime never
    /// learns a concrete storage backend.
    #[must_use]
    pub fn with_runtime_health(
        mut self,
        runtime_health: RuntimeHealth,
        doctor_ports: atm_core::doctor::RuntimeDoctorPorts,
    ) -> Self {
        self.runtime_health = runtime_health;
        self.doctor_ports = Some(doctor_ports);
        self
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
            let Some(emitter) = self.received_hook_selector.select_emitter(&dispatch) else {
                continue;
            };
            let Some(remaining) = deadline.remaining() else {
                warnings.push(hook_warning(AtmError::daemon_unavailable(
                    "received-message hook was skipped because the request deadline was exhausted after persistence",
                )));
                break;
            };
            match tokio::time::timeout(remaining, emitter.emit_received_message(dispatch, deadline))
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

    async fn dispatch_non_write(
        &self,
        request: ApiRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        if deadline.expired() {
            return Err(AtmError::daemon_unavailable(
                "request deadline expired before replacement route dispatch",
            ));
        }
        match request {
            ApiRequest::Write(_) => unreachable!("writes use the canonical write path"),
            ApiRequest::Messages(request) => match *request {
                atm_core::api::MessageCollectionRequest::List(query) => {
                    self.list_messages(query, deadline).await
                }
                atm_core::api::MessageCollectionRequest::Peek(query) => {
                    self.peek_messages(query, deadline).await
                }
                atm_core::api::MessageCollectionRequest::Receive(query) => {
                    self.receive_messages(query, deadline).await
                }
            },
            ApiRequest::Clear(query) => self.clear_messages(query, deadline).await,
            ApiRequest::Doctor(query) => self.doctor(query, deadline).await,
            ApiRequest::CompatibilityPreflight(preflight) => Ok(ApiResponse::new(
                ResponseEnvelope::CompatibilityVerdict(compatibility_verdict(preflight)),
            )),
            ApiRequest::Heartbeat(request) => self.heartbeat(request, ingress, deadline).await,
            ApiRequest::ReloadRuntimeView => self.reload_runtime_view(ingress),
        }
    }

    async fn list_messages(
        &self,
        query: ListQuery,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        let runtime = self.service_runtime.clone();
        let observability = Arc::clone(&self.observability);
        let home = self.daemon_home.clone();
        self.write_admission
            .run(deadline, move || {
                atm_core::list::list_mail_with_runtime(
                    query.with_daemon_paths(home),
                    observability.as_ref(),
                    &runtime,
                )
                .map(ResponseEnvelope::List)
                .map(ApiResponse::new)
            })
            .await
    }

    async fn peek_messages(
        &self,
        query: PeekQuery,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        let runtime = self.service_runtime.clone();
        let observability = Arc::clone(&self.observability);
        let home = self.daemon_home.clone();
        self.write_admission
            .run(deadline, move || {
                atm_core::read::peek_mail_with_runtime(
                    query.with_daemon_paths(home),
                    observability.as_ref(),
                    &runtime,
                )
                .map(Box::new)
                .map(ResponseEnvelope::Peek)
                .map(ApiResponse::new)
            })
            .await
    }

    async fn receive_messages(
        &self,
        query: ReadQuery,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        let runtime = self.service_runtime.clone();
        let observability = Arc::clone(&self.observability);
        let home = self.daemon_home.clone();
        self.write_admission
            .run(deadline, move || {
                atm_core::read::read_mail_with_runtime(
                    query.with_daemon_paths(home),
                    observability.as_ref(),
                    &runtime,
                )
                .map(Box::new)
                .map(ResponseEnvelope::Receive)
                .map(ApiResponse::new)
            })
            .await
    }

    async fn clear_messages(
        &self,
        query: ClearQuery,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        let runtime = self.service_runtime.clone();
        let observability = Arc::clone(&self.observability);
        let home = self.daemon_home.clone();
        self.write_admission
            .run(deadline, move || {
                atm_core::clear::clear_mail_with_runtime(
                    query.with_daemon_paths(home),
                    observability.as_ref(),
                    &runtime,
                )
                .map(ResponseEnvelope::Clear)
                .map(ApiResponse::new)
            })
            .await
    }

    async fn doctor(
        &self,
        query: DoctorQuery,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        let runtime = self.service_runtime.clone();
        let observability = Arc::clone(&self.observability);
        let home = self.daemon_home.clone();
        let runtime_health = self.runtime_health.clone();
        let doctor_ports = self.doctor_ports.clone();
        self.write_admission
            .run(deadline, move || {
                let query = query.with_daemon_paths(home);
                let mut report = match doctor_ports {
                    Some(ports) => atm_core::doctor::run_doctor_with_runtime_ports(
                        query,
                        observability.as_ref(),
                        &runtime,
                        &ports,
                        None,
                    ),
                    None => atm_core::doctor::run_doctor_with_runtime(
                        query,
                        observability.as_ref(),
                        &runtime,
                    ),
                }?;
                report.runtime_status = Some(runtime_health.snapshot());
                Ok(ApiResponse::new(ResponseEnvelope::Doctor(Box::new(report))))
            })
            .await
    }

    async fn heartbeat(
        &self,
        request: atm_core::protocol::TeamMemberHeartbeatRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        if ingress != AuthenticatedIngress::Local {
            return Err(AtmError::validation(
                "heartbeats are available only through authenticated local HTTP adapters",
            ));
        }
        let runtime = self.service_runtime.clone();
        let health = self.runtime_health.clone();
        self.write_admission
            .run(deadline, move || {
                validate_heartbeat_member(runtime, &request)?;
                Ok(request)
            })
            .await
            .map(|request| {
                ApiResponse::new(ResponseEnvelope::Heartbeat(
                    health.record_heartbeat(&request),
                ))
            })
    }

    fn reload_runtime_view(&self, ingress: AuthenticatedIngress) -> Result<ApiResponse, AtmError> {
        if ingress != AuthenticatedIngress::Local {
            return Err(AtmError::validation(
                "runtime reload is available only through authenticated local HTTP adapters",
            ));
        }
        self.service_runtime.clear_roster_cache();
        Ok(ApiResponse::new(ResponseEnvelope::RuntimeViewReloaded))
    }
}

struct CommittedWrite {
    outcome: WriteOutcome,
    canonical_request: atm_core::send::WriteRequest,
    message_id: atm_core::schema::AtmMessageId,
    newly_persisted: bool,
}

impl atm_core::boundary::sealed::Sealed for StorageAndNudgeRouter {}

impl CanonicalWriteHandler for StorageAndNudgeRouter {
    fn write(
        &self,
        mut request: atm_core::send::WriteRequest,
        _ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<ApiResponse, AtmError>> + Send + '_>> {
        Box::pin(async move {
            if deadline.expired() {
                return Err(AtmError::daemon_unavailable(
                    "request deadline expired before replacement write admission",
                ));
            }
            // HTTP payload paths are caller metadata, never daemon filesystem
            // authority. The replacement normalizes both roots before the
            // shared writer uses them for its state and file-policy paths.
            request.home_dir = self.daemon_home.clone();
            request.current_dir = self.daemon_home.clone();
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

    fn dispatch(
        &self,
        request: ApiRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<ApiResponse, AtmError>> + Send + '_>> {
        Box::pin(async move {
            match request {
                ApiRequest::Write(request) => self.write(*request, ingress, deadline).await,
                request => self.dispatch_non_write(request, ingress, deadline).await,
            }
        })
    }
}

fn compatibility_verdict(
    preflight: atm_core::protocol::CompatibilityPreflight,
) -> CompatibilityVerdict {
    let daemon_release = ReleaseVersion::current();
    let daemon_schema_version = atm_core::protocol::CLI_SCHEMA_VERSION;
    let daemon_http_api_version = atm_core::protocol::HttpApiVersion::current();
    if preflight.cli_schema_version == daemon_schema_version
        && preflight.http_api_version.major() == daemon_http_api_version.major()
    {
        CompatibilityVerdict::Compatible {
            daemon_release,
            daemon_schema_version,
            daemon_http_api_version,
        }
    } else {
        CompatibilityVerdict::Incompatible {
            client_release: preflight.client_release,
            daemon_release,
            client_schema_version: preflight.cli_schema_version,
            daemon_schema_version,
            client_http_api_version: preflight.http_api_version,
            daemon_http_api_version,
            code: atm_core::error::AtmErrorCode::ClientDaemonVersionIncompatible,
        }
    }
}

fn validate_heartbeat_member(
    runtime: LocalServiceRuntime,
    request: &atm_core::protocol::TeamMemberHeartbeatRequest,
) -> Result<(), AtmError> {
    if runtime
        .load_roster_member(&request.team, &request.member)?
        .is_none()
    {
        return Err(AtmError::agent_not_found(
            request.member.as_str(),
            request.team.as_str(),
        ));
    }
    Ok(())
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
        AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, GraftNudgeTarget,
        LocalTmuxNudgeTarget, MessageReceivedHookSelector, PostSendBuiltInTarget,
        PostSendEmissionPath, PostSendHookEvent, RosterEntry, RosterHarness, RosterMemberKind,
    };
    use atm_core::observability::NullObservability;
    use atm_core::protocol::{
        HeartbeatActivity, ResponseEnvelope, RuntimeReadinessState, SendResponseEnvelope,
        TeamMemberHeartbeatRequest,
    };
    use atm_core::schema::AtmMessageId;
    use atm_core::send::{SendMessageSource, WriteRequest};
    use atm_core::types::{AgentName, ModelName, PaneId, TeamName};
    use atm_core::{RequestDeadline, api::ApiRequest, error::AtmError};
    use atm_runtime_test_support::{hold_sqlite_writer_lock, open_sqlite_boundary};
    use atm_storage::{MessageKey, MessageQuery, MessageStore, RosterSnapshot};
    use axum::body::{Body, to_bytes};
    use axum::http::header::{CONTENT_TYPE, LOCATION};
    use axum::http::{Request, StatusCode};
    use tempfile::TempDir;
    use tower::ServiceExt;

    use super::{StorageAndNudgeRouter, WriteAdmission};
    use crate::{
        AuthenticatedConnector, CanonicalWriteHandler, NonZeroDuration, RuntimeHealth,
        RuntimeLimits, RuntimeTimeouts, canonical_message_router,
    };
    use crate::{
        DirectPeerTcpConfig, HttpRuntimeBuilder, HttpRuntimeConfig, LoopbackTcpConfig,
        direct_peer_tcp_client,
    };
    #[cfg(unix)]
    use crate::{UnixSocketConfig, UnixSocketMode, UnixSocketOwnerUid};

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

    struct FixedReceivedHookSelector {
        emitter: Arc<RecordingReceivedHook>,
    }

    impl atm_core::boundary::sealed::Sealed for FixedReceivedHookSelector {}

    impl MessageReceivedHookSelector for FixedReceivedHookSelector {
        fn select_emitter(
            &self,
            _dispatch: &BuiltInPostSendDispatch,
        ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
            Some(self.emitter.as_ref())
        }
    }

    struct NoReceivedHookSelector;

    impl atm_core::boundary::sealed::Sealed for NoReceivedHookSelector {}

    impl MessageReceivedHookSelector for NoReceivedHookSelector {
        fn select_emitter(
            &self,
            _dispatch: &BuiltInPostSendDispatch,
        ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
            None
        }
    }

    struct HarnessReceivedHookSelector {
        tmux: Arc<RecordingReceivedHook>,
        graft: Arc<RecordingReceivedHook>,
    }

    impl atm_core::boundary::sealed::Sealed for HarnessReceivedHookSelector {}

    impl MessageReceivedHookSelector for HarnessReceivedHookSelector {
        fn select_emitter(
            &self,
            dispatch: &BuiltInPostSendDispatch,
        ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
            match &dispatch.target {
                PostSendBuiltInTarget::LocalTmux(_) => Some(self.tmux.as_ref()),
                PostSendBuiltInTarget::Graft(_) => Some(self.graft.as_ref()),
            }
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
        fixture_with_selector(
            with_recipient,
            hook_failure,
            cancelled_on_drop,
            |received_hook| {
                Arc::new(FixedReceivedHookSelector {
                    emitter: received_hook,
                })
            },
        )
    }

    fn fixture_with_selector<F>(
        with_recipient: bool,
        hook_failure: Option<AtmError>,
        cancelled_on_drop: Option<Arc<AtomicBool>>,
        select: F,
    ) -> Fixture
    where
        F: FnOnce(Arc<RecordingReceivedHook>) -> Arc<dyn MessageReceivedHookSelector>,
    {
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
        let health = RuntimeHealth::with_owner(99);
        let router = StorageAndNudgeRouter::new(
            assembly.service_runtime,
            Arc::new(NullObservability),
            select(received_hook.clone()),
            home_dir.clone(),
        )
        .with_runtime_health(health, assembly.doctor_ports);
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

    fn direct_peer_runtime_config(fixture: &Fixture, peer_port: u16) -> HttpRuntimeConfig {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};

        HttpRuntimeConfig::new(
            LoopbackTcpConfig::new(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                fixture._temporary_root.path().join("local-http.json"),
                ulid::Ulid::new(),
            ),
            None,
            RuntimeLimits::new(
                std::num::NonZeroUsize::new(4096).expect("non-zero body limit"),
                std::num::NonZeroUsize::new(1).expect("non-zero request limit"),
            ),
            RuntimeTimeouts::new(
                NonZeroDuration::new(Duration::from_secs(1)).expect("request timeout"),
                NonZeroDuration::new(Duration::from_secs(1)).expect("shutdown timeout"),
            ),
        )
        .with_direct_peer_tcp(DirectPeerTcpConfig::new(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), peer_port),
            "sender.example.test"
                .parse()
                .expect("configured source host"),
        ))
    }

    fn unused_direct_peer_port() -> u16 {
        let reserve = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .expect("reserve direct peer port");
        reserve.local_addr().expect("reserved peer address").port()
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
    async fn expired_write_admission_never_starts_a_blocking_job() {
        let admission = WriteAdmission::new(NonZeroUsize::new(1).expect("non-zero capacity"));
        let started = Arc::new(AtomicBool::new(false));
        let job_started = Arc::clone(&started);

        let error = admission
            .run(RequestDeadline::after(Duration::ZERO), move || {
                job_started.store(true, Ordering::SeqCst);
                Ok(())
            })
            .await
            .expect_err("an expired request cannot enter write admission");

        assert_eq!(error.code().as_str(), "ATM_DAEMON_UNAVAILABLE");
        assert!(
            !started.load(Ordering::SeqCst),
            "an expired request must not create a blocking SQLite job"
        );
    }

    fn hook_event() -> PostSendHookEvent {
        PostSendHookEvent {
            sender: "sender".parse().expect("sender"),
            sender_chat_id: None,
            sender_team: "test-team".parse().expect("sender team"),
            recipient: "recipient".parse().expect("recipient"),
            recipient_team: "test-team".parse().expect("recipient team"),
            message_id: AtmMessageId::new(),
            description: "selection test".to_owned(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
            recipient_pane_id: None,
        }
    }

    #[test]
    fn hook_selector_uses_the_committed_dispatch_harness() {
        let fixture = fixture(true, None, None);
        let tmux = Arc::new(RecordingReceivedHook {
            message_store: Arc::clone(&fixture.message_store),
            emitted_ids: Mutex::new(Vec::new()),
            saw_durable_record: AtomicBool::new(false),
            failure: None,
            cancelled_on_drop: None,
        });
        let graft = Arc::new(RecordingReceivedHook {
            message_store: Arc::clone(&fixture.message_store),
            emitted_ids: Mutex::new(Vec::new()),
            saw_durable_record: AtomicBool::new(false),
            failure: None,
            cancelled_on_drop: None,
        });
        let selector = HarnessReceivedHookSelector {
            tmux: Arc::clone(&tmux),
            graft: Arc::clone(&graft),
        };
        let tmux_dispatch = BuiltInPostSendDispatch {
            event: hook_event(),
            target: PostSendBuiltInTarget::LocalTmux(LocalTmuxNudgeTarget {
                pane_id: PaneId::from_cli("%1").expect("pane"),
                rendered_nudge: "tmux nudge".to_owned(),
            }),
        };
        let graft_dispatch = BuiltInPostSendDispatch {
            event: hook_event(),
            target: PostSendBuiltInTarget::Graft(GraftNudgeTarget {
                recipient: "recipient".parse().expect("recipient"),
                recipient_team: "test-team".parse().expect("team"),
            }),
        };

        let selected_tmux = selector
            .select_emitter(&tmux_dispatch)
            .expect("tmux receiver implementation");
        let selected_graft = selector
            .select_emitter(&graft_dispatch)
            .expect("graft receiver implementation");
        assert!(std::ptr::eq(
            selected_tmux,
            tmux.as_ref() as &dyn AsyncMessageReceivedHookEmitter
        ));
        assert!(std::ptr::eq(
            selected_graft,
            graft.as_ref() as &dyn AsyncMessageReceivedHookEmitter
        ));
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

    #[tokio::test]
    async fn authenticated_heartbeat_is_retained_without_affecting_runtime_readiness() {
        let fixture = fixture(true, None, None);
        let first = TeamMemberHeartbeatRequest {
            team: "test-team".parse().expect("team"),
            member: "recipient".parse().expect("agent"),
            pid: 41,
            observed_at: atm_core::types::IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
            session_id: None,
        };
        let first_response = fixture
            .router
            .dispatch(
                ApiRequest::new(atm_core::protocol::RequestEnvelope::Heartbeat(
                    first.clone(),
                )),
                atm_core::AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("authorized heartbeat");
        assert!(matches!(
            first_response.into_inner(),
            ResponseEnvelope::Heartbeat(response) if !response.pid_changed
        ));

        let second = TeamMemberHeartbeatRequest {
            pid: 42,
            activity: HeartbeatActivity::SessionEnded,
            ..first
        };
        let second_response = fixture
            .router
            .dispatch(
                ApiRequest::new(atm_core::protocol::RequestEnvelope::Heartbeat(second)),
                atm_core::AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("second authorized heartbeat");
        assert!(matches!(
            second_response.into_inner(),
            ResponseEnvelope::Heartbeat(response) if response.pid_changed
        ));
        // Health remains listener-owned. A member becoming offline does not
        // make a process that is still serving local adapters become NotReady.
        assert_eq!(
            fixture.router.runtime_health.snapshot().readiness,
            RuntimeReadinessState::Unavailable,
            "the fixture has no running listener; heartbeat cannot claim readiness"
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
        let location = response
            .headers()
            .get(LOCATION)
            .expect("successful write location")
            .to_str()
            .expect("UTF-8 write location")
            .to_owned();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let response_json: serde_json::Value =
            serde_json::from_slice(&body).expect("write response JSON");

        let emitted_ids = fixture
            .received_hook
            .emitted_ids
            .lock()
            .expect("inspect received-hook emissions")
            .clone();
        assert_eq!(emitted_ids.len(), 1, "new durable write emits one hook");
        assert_eq!(
            response_json["message_id"].as_str(),
            Some(emitted_ids[0].to_string().as_str()),
            "the HTTP outcome names the same message the hook observed"
        );
        assert_eq!(
            location,
            format!("/v1/atm/message/{}", emitted_ids[0]),
            "the HTTP location identifies the same durable message"
        );
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

    #[cfg(unix)]
    #[tokio::test]
    async fn uds_runtime_reaches_the_canonical_storage_and_received_hook_path() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};
        use std::num::NonZeroU32;
        use std::os::unix::fs::MetadataExt;

        let fixture = fixture(true, None, None);
        let socket_path = fixture._temporary_root.path().join("atm-runtime.sock");
        let tcp_listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("reserve TCP port for additive runtime bind");
        let tcp_port = tcp_listener.local_addr().expect("read TCP port").port();
        drop(tcp_listener);
        let uid = NonZeroU32::new(
            std::fs::metadata(fixture._temporary_root.path())
                .expect("runtime root metadata")
                .uid(),
        )
        .expect("test process must not use uid zero");
        let runtime = HttpRuntimeBuilder::new(
            HttpRuntimeConfig::new(
                LoopbackTcpConfig::new(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), tcp_port),
                    fixture._temporary_root.path().join("local-http.json"),
                    ulid::Ulid::new(),
                ),
                Some(UnixSocketConfig::new(
                    socket_path.clone(),
                    UnixSocketOwnerUid::new(uid),
                    UnixSocketMode::new(NonZeroU32::new(0o600).expect("owner-only mode")),
                )),
                RuntimeLimits::new(
                    std::num::NonZeroUsize::new(4096).expect("non-zero body limit"),
                    std::num::NonZeroUsize::new(1).expect("non-zero request limit"),
                ),
                RuntimeTimeouts::new(
                    NonZeroDuration::new(Duration::from_secs(1)).expect("request timeout"),
                    NonZeroDuration::new(Duration::from_secs(1)).expect("shutdown timeout"),
                ),
            ),
            Arc::new(fixture.router.clone()),
        )
        .build()
        .expect("valid UDS runtime configuration");
        let running = runtime.start().await.expect("UDS runtime starts");
        let client = crate::unix_socket_client(&socket_path, Duration::from_secs(1))
            .expect("shared UDS client");
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone());

        let response = client
            .execute(ApiRequest::new(atm_core::protocol::RequestEnvelope::Write(
                Box::new(write),
            )))
            .await
            .expect("canonical UDS response");
        assert!(matches!(
            response.into_inner(),
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
        ));
        let emitted_ids = fixture
            .received_hook
            .emitted_ids
            .lock()
            .expect("inspect UDS received hook")
            .clone();
        assert_eq!(emitted_ids.len(), 1, "UDS write emits one received hook");
        assert!(
            fixture
                .received_hook
                .saw_durable_record
                .load(Ordering::SeqCst),
            "the UDS hook runs only after canonical durable persistence"
        );
        assert!(
            fixture
                .message_store
                .load_message(&MessageKey::from(emitted_ids[0]))
                .expect("load UDS message")
                .is_some(),
            "UDS write uses the same storage trait boundary"
        );

        running
            .begin_shutdown()
            .finish()
            .await
            .expect("UDS runtime drains");
        assert!(
            !socket_path.exists(),
            "runtime cleanup removes its UDS endpoint"
        );
    }

    #[tokio::test]
    async fn direct_peer_runtime_reaches_storage_once_and_skips_duplicate_hook() {
        let fixture = fixture(true, None, None);
        let peer_port = unused_direct_peer_port();
        let running = HttpRuntimeBuilder::new(
            direct_peer_runtime_config(&fixture, peer_port),
            Arc::new(fixture.router.clone()),
        )
        .build()
        .expect("valid direct peer runtime configuration")
        .start()
        .await
        .expect("direct peer runtime starts");
        let client = direct_peer_tcp_client(
            "localhost".parse().expect("direct peer host"),
            std::num::NonZeroU16::new(peer_port).expect("non-zero peer port"),
            Duration::from_secs(1),
        )
        .expect("direct peer client");
        let message_id = AtmMessageId::new();
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone())
            .with_origin_metadata(message_id, atm_core::types::IsoTimestamp::now());

        for attempt in 0..2 {
            let response = client
                .execute(ApiRequest::new(atm_core::protocol::RequestEnvelope::Write(
                    Box::new(write.clone()),
                )))
                .await
                .expect("direct peer typed response");
            assert!(
                matches!(
                    response.into_inner(),
                    ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
                ),
                "direct peer attempt {attempt} receives the canonical send response"
            );
        }
        assert_eq!(
            fixture
                .received_hook
                .emitted_ids
                .lock()
                .expect("inspect direct peer hooks")
                .as_slice(),
            &[message_id],
            "the idempotent duplicate stays durable but emits no second hook"
        );
        assert!(
            fixture
                .message_store
                .load_message(&MessageKey::from(message_id))
                .expect("read direct peer message")
                .is_some(),
            "the direct peer write reaches the shared storage boundary"
        );
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("direct peer runtime drains");
    }

    #[tokio::test]
    async fn direct_peer_hook_failure_keeps_the_write_successful_with_a_warning() {
        let fixture = fixture(
            true,
            Some(AtmError::daemon_unavailable(
                "intentional direct peer hook failure",
            )),
            None,
        );
        let peer_port = unused_direct_peer_port();
        let running = HttpRuntimeBuilder::new(
            direct_peer_runtime_config(&fixture, peer_port),
            Arc::new(fixture.router.clone()),
        )
        .build()
        .expect("valid direct peer runtime configuration")
        .start()
        .await
        .expect("direct peer runtime starts");
        let client = direct_peer_tcp_client(
            "localhost".parse().expect("direct peer host"),
            std::num::NonZeroU16::new(peer_port).expect("non-zero peer port"),
            Duration::from_secs(1),
        )
        .expect("direct peer client");
        let response = client
            .execute(ApiRequest::new(atm_core::protocol::RequestEnvelope::Write(
                Box::new(write_request(
                    fixture.home_dir.clone(),
                    fixture.current_dir.clone(),
                )),
            )))
            .await
            .expect("hook failure remains a transport success");
        let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response.into_inner()
        else {
            panic!("direct peer write must keep the canonical send outcome");
        };
        assert_eq!(outcome.warnings.len(), 1, "one advisory hook warning");
        assert!(
            fixture
                .message_store
                .load_message(&MessageKey::from(outcome.message_id))
                .expect("read committed direct peer message")
                .is_some(),
            "hook failure does not roll back the direct peer write"
        );
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("direct peer runtime drains");
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
    async fn axum_route_without_a_selected_receiver_hook_keeps_durable_success() {
        let fixture = fixture_with_selector(true, None, None, |_| Arc::new(NoReceivedHookSelector));
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone());
        let response = post_write(router(&fixture, AuthenticatedConnector::local()), &write).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("send outcome JSON");
        assert!(
            value["warnings"].as_array().is_none_or(Vec::is_empty),
            "an unavailable receiver capability is not a post-commit hook failure"
        );
        assert!(
            fixture
                .received_hook
                .emitted_ids
                .lock()
                .expect("inspect received-hook emissions")
                .is_empty(),
            "the selector prevents an unavailable receiver hook from starting"
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
        assert_eq!(
            messages.len(),
            1,
            "write remains durable without a receiver hook"
        );
    }

    #[tokio::test]
    async fn axum_route_uses_daemon_home_not_the_callers_forged_home_for_file_policy() {
        let fixture = fixture(true, None, None);
        let forged_home = fixture._temporary_root.path().join("forged-client-home");
        let forged_workspace = fixture
            ._temporary_root
            .path()
            .join("forged-client-workspace");
        let client_file = fixture._temporary_root.path().join("client-provided.txt");
        fs::create_dir_all(&forged_home).expect("create forged home");
        fs::create_dir_all(&forged_workspace).expect("create forged workspace");
        fs::write(&client_file, "client-owned attachment").expect("write client file");
        let write = WriteRequest::new(
            forged_home.clone(),
            forged_workspace,
            "sender".parse::<AgentName>().expect("sender"),
            "recipient@test-team",
            "test-team".parse().expect("caller team"),
            SendMessageSource::File {
                path: client_file,
                message: Some("inspect attachment".to_owned()),
            },
            None,
            false,
            None,
            false,
        )
        .expect("write request");

        let response = post_write(router(&fixture, AuthenticatedConnector::local()), &write).await;
        assert_eq!(response.status(), StatusCode::CREATED);

        let daemon_copy = fixture
            .home_dir
            .join(".config/atm/share/test-team/client-provided.txt");
        let forged_copy = forged_home.join(".config/atm/share/test-team/client-provided.txt");
        assert_eq!(
            fs::read_to_string(&daemon_copy).expect("read daemon-owned share copy"),
            "client-owned attachment"
        );
        assert!(
            !forged_copy.exists(),
            "the caller-controlled home_dir must not redirect daemon file-policy output"
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
        assert_eq!(
            value["warnings"][0]["code"].as_str(),
            Some("ATM_DAEMON_UNAVAILABLE"),
            "a timed-out hook keeps the stable advisory error code"
        );
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
        let emitted_ids = fixture
            .received_hook
            .emitted_ids
            .lock()
            .expect("inspect timed-out received-hook emission")
            .clone();
        assert_eq!(emitted_ids.len(), 1, "the hook started after the commit");
        assert!(
            fixture
                .message_store
                .load_message(&MessageKey::from(emitted_ids[0]))
                .expect("load durable message after hook timeout")
                .is_some(),
            "a hook timeout cannot replace or roll back the durable write outcome"
        );
    }
}
