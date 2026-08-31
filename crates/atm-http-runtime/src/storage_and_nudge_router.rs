//! Replacement-owned canonical write composition.
//!
//! This module owns the two explicit blocking seams in the replacement path:
//! the injected storage-backed core write and the injected received-message
//! hook. The enclosing HTTP route remains async and awaits both operations.

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::num::NonZeroU16;
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
use atm_core::graft_store_error;
use atm_core::list::ListQuery;
use atm_core::observability::ObservabilityPort;
use atm_core::protocol::{
    CompatibilityVerdict, GraftReceiverRegistration, GraftReceiverUnregistration, ReleaseVersion,
    RequestEnvelope, RequestId, ResponseEnvelope, SendResponseEnvelope,
};
use atm_core::read::{PeekQuery, ReadQuery};
use atm_core::send::{NudgeMode, WarningEntry, WriteOutcome, prepare_write_with_async_runtime};
use atm_runtime::{AsyncMailboxRuntime, DoctorProjection, DoctorProjectionContext};

use crate::CanonicalWriteHandler;
use crate::PeerConnectionPool;
use crate::RuntimeHealth;
use crate::bare_cli_fifo::{BareCliFifo, BareCliQueueFullDrops, drain_bare_cli_messages};

fn retry_deferred_marker<F>(health: &RuntimeHealth, mut mark: F) -> Result<(), AtmError>
where
    F: FnMut() -> Result<(), AtmError>,
{
    match mark() {
        Ok(()) => Ok(()),
        Err(error) => {
            health.record_queue_marker_set_failure();
            tracing::warn!(
                subsystem = "atm_core.queue",
                action = "queue_marker_set",
                outcome = "failed",
                %error,
                "retrying deferred write queue marker"
            );
            match mark() {
                Ok(()) => Ok(()),
                Err(retry_error) => {
                    health.record_queue_marker_set_failure();
                    Err(retry_error)
                }
            }
        }
    }
}

/// Bounded bridge for synchronous core operations that are not storage-writer
/// submissions.
///
/// Durable message admission uses the async storage boundary directly. The
/// deferred queue marker is the one post-admission exception: its capability
/// is intentionally synchronous, so the marker transaction enters this bridge
/// before the request leaves the router.
#[derive(Clone)]
struct BlockingCoreBridge {
    permits: Arc<tokio::sync::Semaphore>,
    runtime_health: RuntimeHealth,
}

impl BlockingCoreBridge {
    fn new(capacity: NonZeroUsize, runtime_health: RuntimeHealth) -> Self {
        Self {
            permits: Arc::new(tokio::sync::Semaphore::new(capacity.get())),
            runtime_health,
        }
    }

    async fn run<T, F>(&self, deadline: RequestDeadline, job: F) -> Result<T, AtmError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, AtmError> + Send + 'static,
    {
        let remaining = deadline.remaining().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "request deadline expired before replacement blocking core operation",
            )
        })?;
        let permit = tokio::time::timeout(remaining, Arc::clone(&self.permits).acquire_owned())
            .await
            .map_err(|_| {
                AtmError::daemon_unavailable(
                    "request deadline expired before replacement blocking core operation",
                )
            })?
            .map_err(|_| {
                AtmError::daemon_unavailable("replacement blocking core bridge is shutting down")
            })?;
        if deadline.expired() {
            return Err(AtmError::daemon_unavailable(
                "request deadline expired before replacement blocking core operation started",
            ));
        }
        // The blocking job itself is intentionally not wrapped in a
        // `tokio::time::timeout`: a durable storage write must run to
        // completion rather than be abandoned mid-transaction. `elapsed` is
        // therefore observability, not enforcement -- it records when a job
        // outlived the budget it was dispatched with, without changing
        // whether or how long the job runs.
        let started_at = std::time::Instant::now();
        let outcome = tokio::task::spawn_blocking(job).await.map_err(|source| {
            AtmError::new(
                atm_core::error::AtmErrorCode::InternalError,
                "replacement storage write task ended unexpectedly",
            )
            .with_cause(source)
        })?;
        let elapsed = started_at.elapsed();
        if elapsed > remaining {
            self.runtime_health.record_blocking_core_bridge_stall();
            tracing::warn!(
                subsystem = "atm_http_runtime.blocking_core_bridge",
                action = "blocking_job",
                outcome = "budget_exceeded",
                elapsed = ?elapsed,
                budget = ?remaining,
                "blocking core bridge job outlived its remaining request budget"
            );
        }
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
    blocking_core_bridge: BlockingCoreBridge,
    async_mailbox_runtime: Option<Arc<dyn AsyncMailboxRuntime>>,
    doctor_projection: Option<Arc<dyn DoctorProjection>>,
    daemon_home: PathBuf,
    runtime_health: RuntimeHealth,
    doctor_ports: Option<atm_core::doctor::RuntimeDoctorPorts>,
    daemon_context: Option<atm_core::doctor::DoctorExecutionContext>,
    direct_peer_port: NonZeroU16,
    peer_connection_pool: Option<PeerConnectionPool>,
    shared_direct_peer_client: Option<reqwest::Client>,
    maintenance: Option<Arc<dyn crate::RuntimeMaintenance>>,
    bare_cli_fifo: BareCliFifo,
    bare_cli_queue_full_drops: BareCliQueueFullDrops,
    member_state_transition_sink: Option<Arc<dyn crate::MemberStateTransitionSink>>,
}

impl StorageAndNudgeRouter {
    #[must_use]
    pub fn new(
        service_runtime: LocalServiceRuntime,
        observability: Arc<dyn ObservabilityPort + Send + Sync>,
        received_hook_selector: Arc<dyn MessageReceivedHookSelector>,
        daemon_home: PathBuf,
    ) -> Self {
        let runtime_health = RuntimeHealth::default();
        Self {
            service_runtime,
            observability,
            received_hook_selector,
            blocking_core_bridge: BlockingCoreBridge::new(
                NonZeroUsize::new(1).expect("one non-storage core bridge operation"),
                runtime_health.clone(),
            ),
            async_mailbox_runtime: None,
            doctor_projection: None,
            daemon_home,
            runtime_health,
            doctor_ports: None,
            daemon_context: None,
            direct_peer_port: crate::direct_peer_port(),
            peer_connection_pool: None,
            shared_direct_peer_client: None,
            maintenance: None,
            bare_cli_fifo: Default::default(),
            bare_cli_queue_full_drops: Default::default(),
            member_state_transition_sink: None,
        }
    }

    /// Installs the composition-owned Tokio mailbox port.  AV.1b routes the
    /// read family exclusively through this capability.
    #[must_use]
    pub fn with_async_mailbox_runtime(
        mut self,
        async_mailbox_runtime: Arc<dyn AsyncMailboxRuntime>,
    ) -> Self {
        self.async_mailbox_runtime = Some(async_mailbox_runtime);
        self
    }

    /// Installs the bounded, composition-owned doctor projection.  Doctor is
    /// a control-plane request and must not share the mailbox read bridge.
    #[must_use]
    pub fn with_doctor_projection(mut self, doctor_projection: Arc<dyn DoctorProjection>) -> Self {
        self.doctor_projection = Some(doctor_projection);
        self
    }

    #[cfg(test)]
    fn with_direct_peer_port(mut self, direct_peer_port: NonZeroU16) -> Self {
        self.direct_peer_port = direct_peer_port;
        self
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
        self.blocking_core_bridge.runtime_health = runtime_health.clone();
        self.runtime_health = runtime_health;
        self.doctor_ports = Some(doctor_ports);
        self
    }

    /// Attaches the process-owned maintenance task to this composition root.
    #[must_use]
    pub fn with_maintenance(mut self, maintenance: Arc<dyn crate::RuntimeMaintenance>) -> Self {
        self.maintenance = Some(maintenance);
        self
    }

    /// Installs the daemon-lifetime bare-CLI FIFO and its overflow counter.
    #[must_use]
    pub fn with_bare_cli_fifo(
        mut self,
        fifo: BareCliFifo,
        queue_full_drops: BareCliQueueFullDrops,
    ) -> Self {
        self.bare_cli_fifo = fifo;
        self.bare_cli_queue_full_drops = queue_full_drops;
        self
    }

    /// Installs the best-effort idle-transition queue drain owned by daemon
    /// composition. The runtime only forwards the sealed notification.
    #[must_use]
    pub fn with_member_state_transition_sink(
        mut self,
        sink: Arc<dyn crate::MemberStateTransitionSink>,
    ) -> Self {
        self.member_state_transition_sink = Some(sink);
        self
    }

    /// Attaches daemon process identity captured at bootstrap to the doctor
    /// response. This is intentionally injected: request-time doctor handling
    /// must not read a mutable process environment to identify its server.
    #[must_use]
    pub fn with_daemon_context(
        mut self,
        daemon_context: atm_core::doctor::DoctorExecutionContext,
    ) -> Self {
        self.daemon_context = Some(daemon_context);
        self
    }

    /// Installs the daemon-owned outbound pool paired with the selected
    /// authenticated stream adapter. The pool is a transport implementation
    /// detail; canonical request routing remains in this router.
    #[must_use]
    pub fn with_peer_connection_pool(mut self, pool: PeerConnectionPool) -> Self {
        self.peer_connection_pool = Some(pool);
        self
    }

    /// Injects the daemon-lifetime reqwest client for direct plaintext peers.
    /// Clones share reqwest's built-in connection pool; this router never
    /// creates a second plaintext pooling mechanism.
    #[must_use]
    pub fn with_shared_direct_peer_client(mut self, client: reqwest::Client) -> Self {
        self.shared_direct_peer_client = Some(client);
        self
    }

    /// Drains retained authenticated peer connections after HTTP request
    /// admission has stopped. Individual request guards remain non-blocking
    /// on drop; only this daemon lifecycle path awaits driver termination.
    pub async fn shutdown_peer_connections(&self, deadline: std::time::Duration) {
        if let Some(pool) = &self.peer_connection_pool {
            pool.shutdown(deadline).await;
        }
    }

    async fn commit_write(
        &self,
        request: atm_core::send::WriteRequest,
        deadline: RequestDeadline,
    ) -> Result<CommittedWrite, AtmError> {
        let mut prepared = prepare_write_with_async_runtime(
            request,
            self.observability.as_ref(),
            &self.service_runtime,
        )
        .await?;
        let newly_persisted = prepared.is_newly_persisted();
        let canonical_request = prepared.outbound_request();
        let message_id = prepared.persisted_message_id();
        let persisted_timestamp = prepared.persisted_timestamp();
        let received_hook_dispatches = if newly_persisted {
            prepared.build_received_hook_dispatches(&self.service_runtime)
        } else {
            Ok(Vec::new())
        };
        let outcome = prepared.finish(&self.service_runtime, self.observability.as_ref())?;
        if newly_persisted && canonical_request.nudge_mode == NudgeMode::Deferred {
            let runtime = self.service_runtime.clone();
            let health = self.runtime_health.clone();
            let marker_result = self
                .blocking_core_bridge
                .run(deadline, move || {
                    Ok(retry_deferred_marker(&health, || {
                        prepared.mark_pending_if_deferred(&runtime)
                    }))
                })
                .await;
            match marker_result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!(
                    subsystem = "atm_core.queue",
                    action = "queue_marker_set",
                    outcome = "failed",
                    %error,
                    "deferred write queue marker failed after one retry"
                ),
                Err(error) => tracing::warn!(
                    subsystem = "atm_core.queue",
                    action = "queue_marker_set",
                    outcome = "failed",
                    %error,
                    "deferred write queue marker task could not be scheduled"
                ),
            }
        }
        Ok(CommittedWrite {
            outcome,
            canonical_request,
            message_id,
            persisted_timestamp,
            newly_persisted,
            received_hook_dispatches,
        })
    }

    /// Delivers one locally admitted host-qualified write using the daemon's
    /// selected peer-wire mode. The record is already durable at this point;
    /// this method creates neither a second application route nor delivery
    /// recovery state. Per ADR-057, connection reuse can redial only before
    /// exchange; it never retries a request after handing it to the sender.
    async fn dispatch_resolved_peer_write(
        &self,
        request: &atm_core::send::WriteRequest,
        message_id: atm_core::schema::AtmMessageId,
        timestamp: atm_core::types::IsoTimestamp,
        deadline: RequestDeadline,
        _request_id: RequestId,
    ) -> Result<(), AtmError> {
        let Some(host) = request.to.as_ref().and_then(|recipient| recipient.host()) else {
            return Ok(());
        };
        let remaining = deadline.remaining().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "request deadline expired before cross-host acknowledgement delivery",
            )
        })?;
        let client = match self.peer_connection_pool.as_ref() {
            Some(pool) => crate::client::pooled_peer_stream_write_client(
                host.clone(),
                self.direct_peer_port,
                remaining,
                pool.clone(),
            )?,
            None => match self.shared_direct_peer_client.as_ref() {
                Some(client) => crate::client::direct_peer_tcp_client_with_shared_client(
                    host.clone(),
                    self.direct_peer_port,
                    remaining,
                    client.clone(),
                )?,
                None => crate::client::direct_peer_tcp_client(
                    host.clone(),
                    self.direct_peer_port,
                    remaining,
                )?,
            },
        };
        let request = request.clone().with_origin_metadata(message_id, timestamp);
        match client
            .execute(ApiRequest::new(RequestEnvelope::Write(Box::new(request))))
            .await?
            .into_inner()
        {
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(_)) => Ok(()),
            response => Err(AtmError::new(
                atm_core::error_codes::AtmErrorCode::InternalError,
                "cross-host daemon-owned delivery returned a non-write response",
            )
            .with_cause(format!("received response: {response:?}"))),
        }
    }

    async fn emit_received_hook(
        &self,
        dispatches: Result<Vec<atm_core::boundary::BuiltInPostSendDispatch>, AtmError>,
        deadline: RequestDeadline,
    ) -> Vec<WarningEntry> {
        if deadline.expired() {
            return vec![hook_warning(AtmError::daemon_unavailable(
                "received-message hook was skipped because the request deadline was exhausted after persistence",
            ))];
        }
        let dispatches = match dispatches {
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
            ApiRequest::Search(request) => self.search(*request, ingress, deadline).await,
            ApiRequest::CompatibilityPreflight(preflight) => Ok(ApiResponse::new(
                ResponseEnvelope::CompatibilityVerdict(compatibility_verdict(preflight)),
            )),
            ApiRequest::Heartbeat(request) => self.heartbeat(request, ingress, deadline).await,
            ApiRequest::QueueGetNext(request) => {
                self.queue_get_next(request, ingress, deadline).await
            }
            ApiRequest::GraftReceiverRegister(request) => {
                self.graft_receiver_register(request, ingress, deadline)
                    .await
            }
            ApiRequest::GraftReceiverRefresh(request) => {
                self.graft_receiver_refresh(request, ingress, deadline)
                    .await
            }
            ApiRequest::GraftReceiverUnregister(request) => {
                self.graft_receiver_unregister(request, ingress, deadline)
                    .await
            }
            ApiRequest::GraftReceiverLookup { team, agent } => {
                self.graft_receiver_lookup(team, agent, ingress, deadline)
                    .await
            }
            ApiRequest::ReloadRuntimeView => self.reload_runtime_view(ingress),
        }
    }

    async fn list_messages(
        &self,
        query: ListQuery,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        let runtime = self.async_mailbox_runtime.as_ref().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "async mailbox runtime was not installed at daemon startup",
            )
        })?;
        let command = atm_core::list::prepare_async_list(&query)?;
        runtime
            .list_command(command, deadline)
            .await
            .map(ResponseEnvelope::List)
            .map(ApiResponse::new)
    }

    async fn peek_messages(
        &self,
        query: PeekQuery,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        let runtime = self.async_mailbox_runtime.as_ref().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "async mailbox runtime was not installed at daemon startup",
            )
        })?;
        let command = atm_core::read::async_projection::prepare_async_peek(&query)?;
        runtime
            .peek_command(command, deadline)
            .await
            .map(Box::new)
            .map(ResponseEnvelope::Peek)
            .map(ApiResponse::new)
    }

    async fn receive_messages(
        &self,
        query: ReadQuery,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        let runtime = self.async_mailbox_runtime.as_ref().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "async mailbox runtime was not installed at daemon startup",
            )
        })?;
        let command = atm_core::read::async_projection::prepare_async_read(&query)?;
        runtime
            .read_command(command, deadline)
            .await
            .map(Box::new)
            .map(ResponseEnvelope::Receive)
            .map(ApiResponse::new)
    }

    async fn clear_messages(
        &self,
        query: ClearQuery,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        let runtime = self.service_runtime.clone();
        let observability = Arc::clone(&self.observability);
        let home = self.daemon_home.clone();
        self.blocking_core_bridge
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
        let doctor_projection = self.doctor_projection.as_ref().ok_or_else(|| {
            AtmError::daemon_unavailable("doctor projection was not installed at daemon startup")
        })?;
        let handoff = self
            .async_mailbox_runtime
            .as_ref()
            .and_then(|runtime| runtime.handoff_diagnostics());
        let runtime_health = self.runtime_health.clone();
        let bare_cli_queue_full_drops = self.bare_cli_queue_full_drops.clone();
        let daemon_context = self.daemon_context.clone();
        let mut runtime_status = runtime_health.snapshot();
        runtime_status.bare_cli_queue_full_drops_total =
            bare_cli_queue_full_drops.load(std::sync::atomic::Ordering::Relaxed);
        let mut report = doctor_projection
            .project(
                query.with_daemon_paths(self.daemon_home.clone()),
                DoctorProjectionContext {
                    runtime_status: Some(runtime_status),
                    daemon_context,
                    handoff,
                },
                deadline,
            )
            .await?;
        let runtime_status = report
            .runtime_status
            .as_ref()
            .expect("projection retains runtime status");
        report.herdr_queue_pump.last_tick_at = runtime_status.herdr_queue_last_tick_at;
        report.herdr_queue_pump.breaker = report.herdr_breaker.clone();
        Ok(ApiResponse::new(ResponseEnvelope::Doctor(Box::new(report))))
    }

    async fn search(
        &self,
        request: atm_core::search::SearchRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        let store = self.service_runtime.async_message_search_store()?;
        atm_core::search::execute_search(ingress, request, store.as_ref(), deadline)
            .await
            .map(|response| atm_core::protocol::ResponseEnvelope::Search(Box::new(response)))
            .map(ApiResponse::new)
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
        let sink = self.member_state_transition_sink.clone();
        self.blocking_core_bridge
            .run(deadline, move || {
                validate_heartbeat_member(&runtime, &request.team, &request.member)?;
                Ok(request)
            })
            .await
            .map(|request| {
                let member = atm_core::boundary::MemberKey::new(
                    request.team.clone(),
                    request.member.clone(),
                );
                let (response, transition) = health.record_heartbeat(&request);
                if let (Some(from), Some(sink)) = (transition, sink.as_ref()) {
                    sink.on_transition(&member, from, atm_core::protocol::RuntimeMemberState::Idle);
                }
                ApiResponse::new(ResponseEnvelope::Heartbeat(response))
            })
    }

    async fn queue_get_next(
        &self,
        request: atm_core::protocol::QueueGetNextRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        if ingress != AuthenticatedIngress::Local {
            return Err(AtmError::validation(
                "bare-CLI queue pulls are available only through authenticated local HTTP adapters",
            ));
        }
        let runtime = self.service_runtime.clone();
        let fifo = self.bare_cli_fifo.clone();
        self.blocking_core_bridge
            .run(deadline, move || {
                validate_heartbeat_member(&runtime, &request.team, &request.member)?;
                let member = atm_core::boundary::MemberKey::new(request.team, request.member);
                drain_bare_cli_messages(&fifo, &member).map(|messages| {
                    ApiResponse::new(ResponseEnvelope::QueueGetNext(
                        atm_core::protocol::QueueGetNextResponse { messages },
                    ))
                })
            })
            .await
    }

    async fn graft_receiver_register(
        &self,
        request: GraftReceiverRegistration,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        require_local_graft_ingress(ingress)?;
        let runtime = self.service_runtime.clone();
        let store = runtime.graft_receiver_endpoint_store()?;
        self.blocking_core_bridge
            .run(deadline, move || {
                validate_graft_receiver_member(&runtime, &request.team, &request.agent)?;
                let local_endpoint = match request.endpoint.ip() {
                    IpAddr::V4(address) => address == Ipv4Addr::LOCALHOST,
                    IpAddr::V6(address) => address == Ipv6Addr::LOCALHOST,
                };
                if !local_endpoint {
                    return Err(AtmError::local_http_endpoint_non_loopback(
                        "graft receiver endpoint must be loopback",
                    ));
                }
                store
                    .register(&request, atm_core::types::IsoTimestamp::now().into_inner())
                    .map_err(graft_store_error)?;
                Ok(ApiResponse::new(ResponseEnvelope::GraftReceiverRegister))
            })
            .await
    }

    /// Owner-checked liveness keepalive per ADR-056: unlike `register`, this
    /// rejects with `NotOwner` (mapped to `AtmErrorCode::GraftReceiverNotOwner`)
    /// when the stored lease no longer matches `request.owner_generation`, so a
    /// receiver-side generation mismatch is observable instead of silently
    /// re-upserting over a lease another generation now legitimately owns.
    async fn graft_receiver_refresh(
        &self,
        request: atm_core::protocol::GraftReceiverRefreshRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        require_local_graft_ingress(ingress)?;
        let runtime = self.service_runtime.clone();
        let store = runtime.graft_receiver_endpoint_store()?;
        self.blocking_core_bridge
            .run(deadline, move || {
                validate_graft_receiver_member(&runtime, &request.team, &request.agent)?;
                store
                    .refresh(
                        &request.team,
                        &request.agent,
                        &request.owner_generation,
                        atm_core::types::IsoTimestamp::now().into_inner(),
                    )
                    .map_err(graft_store_error)?;
                Ok(ApiResponse::new(ResponseEnvelope::GraftReceiverRefresh))
            })
            .await
    }

    async fn graft_receiver_unregister(
        &self,
        request: GraftReceiverUnregistration,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        require_local_graft_ingress(ingress)?;
        let runtime = self.service_runtime.clone();
        let store = runtime.graft_receiver_endpoint_store()?;
        self.blocking_core_bridge
            .run(deadline, move || {
                validate_graft_receiver_member(&runtime, &request.team, &request.agent)?;
                store
                    .unregister(&request.team, &request.agent, &request.owner_generation)
                    .map_err(graft_store_error)?;
                Ok(ApiResponse::new(ResponseEnvelope::GraftReceiverUnregister))
            })
            .await
    }

    async fn graft_receiver_lookup(
        &self,
        team: atm_core::types::TeamName,
        agent: atm_core::types::AgentName,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        require_local_graft_ingress(ingress)?;
        let runtime = self.service_runtime.clone();
        let store = runtime.graft_receiver_endpoint_store()?;
        self.blocking_core_bridge
            .run(deadline, move || {
                validate_graft_receiver_member(&runtime, &team, &agent)?;
                let lease = store.lookup(&team, &agent).map_err(graft_store_error)?;
                Ok(ApiResponse::new(ResponseEnvelope::GraftReceiverLookup(
                    lease,
                )))
            })
            .await
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

impl crate::RuntimeMaintenance for StorageAndNudgeRouter {
    fn start(&self, shutdown: tokio::sync::watch::Receiver<()>) -> tokio::task::JoinHandle<()> {
        self.maintenance.as_ref().map_or_else(
            || tokio::spawn(async {}),
            |maintenance| maintenance.start(shutdown),
        )
    }
}

struct CommittedWrite {
    outcome: WriteOutcome,
    canonical_request: atm_core::send::WriteRequest,
    message_id: atm_core::schema::AtmMessageId,
    persisted_timestamp: atm_core::types::IsoTimestamp,
    newly_persisted: bool,
    received_hook_dispatches: Result<Vec<atm_core::boundary::BuiltInPostSendDispatch>, AtmError>,
}

impl atm_core::boundary::sealed::Sealed for StorageAndNudgeRouter {}

impl CanonicalWriteHandler for StorageAndNudgeRouter {
    fn write(
        &self,
        request: atm_core::send::WriteRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<ApiResponse, AtmError>> + Send + '_>> {
        self.write_with_request_id(
            request,
            ingress,
            deadline,
            atm_core::protocol::next_request_id(),
        )
    }

    fn write_with_request_id(
        &self,
        mut request: atm_core::send::WriteRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
        request_id: RequestId,
    ) -> Pin<Box<dyn Future<Output = Result<ApiResponse, AtmError>> + Send + '_>> {
        Box::pin(async move {
            if deadline.expired() {
                return Err(AtmError::daemon_unavailable(
                    "request deadline expired before replacement storage-writer ingress",
                ));
            }
            // HTTP payload paths are caller metadata, never daemon filesystem
            // authority. The replacement normalizes both roots before the
            // shared writer uses them for its state and file-policy paths.
            request.home_dir = self.daemon_home.clone();
            request.current_dir = self.daemon_home.clone();
            let mut committed = self.commit_write(request, deadline).await?;
            if ingress == AuthenticatedIngress::Local
                && committed.newly_persisted
                && committed
                    .canonical_request
                    .to
                    .as_ref()
                    .and_then(|recipient| recipient.host())
                    .is_some()
            {
                self.dispatch_resolved_peer_write(
                    &committed.canonical_request,
                    committed.message_id,
                    committed.persisted_timestamp,
                    deadline,
                    request_id,
                )
                .await?;
            }
            if committed.newly_persisted {
                let hook = self.clone();
                let warnings = hook
                    .emit_received_hook(committed.received_hook_dispatches, deadline)
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
        self.dispatch_with_request_id(
            request,
            ingress,
            deadline,
            atm_core::protocol::next_request_id(),
        )
    }

    fn dispatch_with_request_id(
        &self,
        request: ApiRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
        request_id: RequestId,
    ) -> Pin<Box<dyn Future<Output = Result<ApiResponse, AtmError>> + Send + '_>> {
        Box::pin(async move {
            match request {
                ApiRequest::Write(request) => {
                    self.write_with_request_id(*request, ingress, deadline, request_id)
                        .await
                }
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
    runtime: &LocalServiceRuntime,
    team: &atm_core::types::TeamName,
    member: &atm_core::types::AgentName,
) -> Result<(), AtmError> {
    if runtime.load_roster_member(team, member)?.is_none() {
        return Err(AtmError::agent_not_found(member.as_str(), team.as_str()));
    }
    Ok(())
}

fn require_local_graft_ingress(ingress: AuthenticatedIngress) -> Result<(), AtmError> {
    if ingress != AuthenticatedIngress::Local {
        return Err(AtmError::validation(
            "graft receiver registration is available only through authenticated local HTTP adapters",
        ));
    }
    Ok(())
}

fn validate_graft_receiver_member(
    runtime: &LocalServiceRuntime,
    team: &atm_core::types::TeamName,
    agent: &atm_core::types::AgentName,
) -> Result<(), AtmError> {
    if runtime.load_roster_member(team, agent)?.is_none() {
        return Err(AtmError::agent_not_found(agent.as_str(), team.as_str()));
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
    use std::num::{NonZeroU16, NonZeroUsize};
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use atm_core::LocalServiceRuntime;
    use atm_core::boundary::{
        AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, GraftNudgeTarget,
        LocalSteerTarget, LocalTmuxNudgeTarget, MemberKey, MessageReceivedHookSelector, NudgeClaim,
        NudgeKind, PendingNudgeStore, PostSendBuiltInTarget, PostSendEmissionPath,
        PostSendHookEvent, RosterEntry, RosterHarness, RosterMemberKind,
    };
    use atm_core::observability::NullObservability;
    use atm_core::protocol::{
        GraftReceiverRegistration, GraftReceiverUnregistration, HeartbeatActivity, OwnerGeneration,
        QueueGetNextRequest, QueuedNudgeMessage, RequestEnvelope, ResponseEnvelope,
        RuntimeReadinessState, SendResponseEnvelope, TeamMemberHeartbeatRequest,
    };
    use atm_core::schema::AtmMessageId;
    use atm_core::send::{
        MessageClassification, NudgeMode, SendMessageSource, TemplateSendSource, WriteRequest,
    };
    use atm_core::types::{AgentName, IsoTimestamp, ModelName, PaneId, TeamName};
    use atm_core::{AuthenticatedIngress, RequestDeadline, api::ApiRequest, error::AtmError};
    use atm_runtime::{
        DoctorProjection, DoctorProjectionConfig, DoctorProjectionContext, HandoffConfig,
        StorageDoctorProjection,
    };
    use atm_runtime_test_support::{
        inspect_template_admission_for_test, install_sqlite_message_write_failure,
        open_graft_receiver_endpoint_store, open_sqlite_boundary,
    };
    use atm_storage::{
        MessageKey, MessageQuery, MessageStore, RosterSnapshot, RosterStore as StorageRosterStore,
        TemplateFrontmatter, TemplateSha,
    };
    use axum::body::{Body, to_bytes};
    use axum::http::header::{CONTENT_TYPE, LOCATION};
    use axum::http::{Request, StatusCode};
    use tempfile::TempDir;
    use tower::ServiceExt;

    use super::{BlockingCoreBridge, StorageAndNudgeRouter, retry_deferred_marker};
    use crate::{
        AcceptedPeerStream, EstablishedPeerStream, HttpRuntimeBuilder, HttpRuntimeConfig,
        LoopbackTcpConfig, PeerConnectionPool, PeerPoolConfig, PeerStreamAdapter, PeerStreamFuture,
        direct_peer_tcp_client, shared_direct_peer_client,
    };
    use crate::{
        AuthenticatedConnector, BareCliFifo, BareCliQueueFullDrops, CanonicalWriteHandler,
        NonZeroDuration, RuntimeHealth, RuntimeLimits, RuntimeTimeouts, append_bare_cli_message,
        canonical_api_router, canonical_message_router,
    };
    #[cfg(unix)]
    use crate::{UnixSocketConfig, UnixSocketMode, UnixSocketOwnerUid};

    struct RecordingReceivedHook {
        message_store: Arc<dyn MessageStore + Send + Sync>,
        emitted_ids: Mutex<Vec<AtmMessageId>>,
        dispatches: Mutex<Vec<BuiltInPostSendDispatch>>,
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
            self.dispatches
                .lock()
                .expect("record received hook dispatch")
                .push(dispatch.clone());
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

    struct FailingMarkPendingStore {
        inner: Arc<dyn PendingNudgeStore + Send + Sync>,
        remaining_failures: AtomicUsize,
    }

    impl atm_storage::contract::sealed::Sealed for FailingMarkPendingStore {}

    impl PendingNudgeStore for FailingMarkPendingStore {
        fn mark_pending(
            &self,
            member: &MemberKey,
            msg: &AtmMessageId,
            at: IsoTimestamp,
        ) -> Result<bool, AtmError> {
            let previous = self
                .remaining_failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .unwrap_or(0);
            if previous > 0 {
                return Err(AtmError::daemon_unavailable(
                    "test pending-marker store failure",
                ));
            }
            self.inner.mark_pending(member, msg, at)
        }

        fn claim_next_pending(&self, member: &MemberKey) -> Result<Option<NudgeClaim>, AtmError> {
            self.inner.claim_next_pending(member)
        }

        fn requeue_pending(&self, member: &MemberKey, claim: &NudgeClaim) -> Result<(), AtmError> {
            self.inner.requeue_pending(member, claim)
        }

        fn release_pending(&self, member: &MemberKey, claim: &NudgeClaim) -> Result<(), AtmError> {
            self.inner.release_pending(member, claim)
        }

        fn clear_pending_on_read(
            &self,
            member: &MemberKey,
            msg: &AtmMessageId,
        ) -> Result<(), AtmError> {
            self.inner.clear_pending_on_read(member, msg)
        }

        fn clear_pending_on_handoff(
            &self,
            member: &MemberKey,
            msg: &AtmMessageId,
        ) -> Result<(), AtmError> {
            self.inner.clear_pending_on_handoff(member, msg)
        }

        fn list_pending_members(&self) -> Result<Vec<MemberKey>, AtmError> {
            self.inner.list_pending_members()
        }
    }

    struct FixtureTemplateComposer {
        source_bytes: Vec<u8>,
        inspection: atm_core::TemplateInspection,
    }

    impl FixtureTemplateComposer {
        fn new(body: &str) -> Self {
            Self::with_inspection(
                body.as_bytes().to_vec(),
                atm_core::TemplateInspection {
                    sha: TemplateSha::new(
                        "814271b7e98145c998a2c1f20270856c592881ba7dac4dfee9307d8093163a03",
                    )
                    .expect("template SHA"),
                    frontmatter: TemplateFrontmatter::default(),
                    include_references: Vec::new(),
                    output_format: atm_storage::TemplateOutputFormat::Text,
                },
            )
        }

        fn with_inspection(
            source_bytes: Vec<u8>,
            inspection: atm_core::TemplateInspection,
        ) -> Self {
            Self {
                source_bytes,
                inspection,
            }
        }
    }

    impl atm_core::boundary::sealed::Sealed for FixtureTemplateComposer {}

    impl atm_core::TemplateComposer for FixtureTemplateComposer {
        fn inspect(
            &self,
            source: &atm_core::TemplateSource,
        ) -> Result<atm_core::TemplateInspection, AtmError> {
            assert_eq!(source.raw_file_bytes, self.source_bytes);
            Ok(self.inspection.clone())
        }

        fn render_within_root(
            &self,
            source: &atm_core::TemplateSource,
            _vars: &serde_json::Map<String, serde_json::Value>,
            _root: &atm_core::TemplateRoot,
        ) -> Result<atm_core::RenderedBody, AtmError> {
            let text = std::str::from_utf8(&source.raw_file_bytes)
                .map_err(|_| AtmError::template_content_not_utf8())?
                .to_owned();
            Ok(atm_core::RenderedBody { text })
        }

        fn render_without_includes(
            &self,
            _source: &atm_core::TemplateSource,
            _vars: &serde_json::Map<String, serde_json::Value>,
        ) -> Result<atm_core::RenderedBody, AtmError> {
            unreachable!("HTTP runtime tests require confinement-aware rendering")
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
                PostSendBuiltInTarget::LocalSteer(LocalSteerTarget::Tmux(_)) => {
                    Some(self.tmux.as_ref())
                }
                PostSendBuiltInTarget::LocalSteer(LocalSteerTarget::Herdr(_)) => None,
                PostSendBuiltInTarget::Graft(_) => Some(self.graft.as_ref()),
                PostSendBuiltInTarget::QueuePull(_) => None,
            }
        }
    }

    struct Fixture {
        _temporary_root: TempDir,
        router: StorageAndNudgeRouter,
        message_store: Arc<dyn MessageStore + Send + Sync>,
        pending_nudge_store: Arc<dyn PendingNudgeStore + Send + Sync>,
        received_hook: Arc<RecordingReceivedHook>,
        runtime_health: RuntimeHealth,
        database_path: PathBuf,
        home_dir: PathBuf,
        current_dir: PathBuf,
    }

    /// Test-only opaque adapter: it preserves the selected authenticated-peer
    /// seam while counting outbound establishments. It is deliberately not a
    /// TLS substitute; physical mTLS remains covered by the live peer gate.
    #[derive(Default)]
    struct CountingPassthroughPeerAdapter {
        outbound_connects: AtomicUsize,
    }

    impl PeerStreamAdapter for CountingPassthroughPeerAdapter {
        fn connect<'a>(
            &'a self,
            stream: tokio::net::TcpStream,
            _peer: &'a atm_core::types::HostName,
        ) -> PeerStreamFuture<'a, EstablishedPeerStream> {
            self.outbound_connects.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(Box::new(stream) as EstablishedPeerStream) })
        }

        fn accept<'a>(
            &'a self,
            stream: tokio::net::TcpStream,
        ) -> PeerStreamFuture<'a, AcceptedPeerStream> {
            Box::pin(async move {
                Ok(AcceptedPeerStream {
                    source_host: "127.0.0.1".parse().expect("loopback source host"),
                    stream: Box::new(stream),
                })
            })
        }
    }

    fn fixture(
        with_recipient: bool,
        hook_failure: Option<AtmError>,
        cancelled_on_drop: Option<Arc<AtomicBool>>,
    ) -> Fixture {
        fixture_with_selector_and_template(
            with_recipient,
            hook_failure,
            cancelled_on_drop,
            None,
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
        fixture_with_selector_and_template(
            with_recipient,
            hook_failure,
            cancelled_on_drop,
            None,
            select,
        )
    }

    fn fixture_with_selector_and_template<F>(
        with_recipient: bool,
        hook_failure: Option<AtmError>,
        cancelled_on_drop: Option<Arc<AtomicBool>>,
        template_composer: Option<Arc<dyn atm_core::TemplateComposer>>,
        select: F,
    ) -> Fixture
    where
        F: FnOnce(Arc<RecordingReceivedHook>) -> Arc<dyn MessageReceivedHookSelector>,
    {
        fixture_with_selector_and_template_and_pending(
            with_recipient,
            hook_failure,
            cancelled_on_drop,
            template_composer,
            select,
            None,
        )
    }

    fn fixture_with_selector_and_template_and_pending<F>(
        with_recipient: bool,
        hook_failure: Option<AtmError>,
        cancelled_on_drop: Option<Arc<AtomicBool>>,
        template_composer: Option<Arc<dyn atm_core::TemplateComposer>>,
        select: F,
        pending_marker_failures: Option<usize>,
    ) -> Fixture
    where
        F: FnOnce(Arc<RecordingReceivedHook>) -> Arc<dyn MessageReceivedHookSelector>,
    {
        let temporary_root = tempfile::tempdir().expect("temporary runtime root");
        let database_path = temporary_root.path().join("mail.sqlite");
        let assembly = open_sqlite_boundary(&database_path).expect("assemble SQLite boundary");
        let team: TeamName = "test-team".parse().expect("team");
        if with_recipient {
            seed_fixture_roster(&assembly.shared_roster_store_arc(), &team);
        }
        let message_store = assembly.message_store_arc();
        let pending_nudge_store = assembly
            .service_runtime
            .pending_nudge_store()
            .expect("sqlite pending-nudge store");
        let pending_nudge_store_for_runtime =
            pending_store_with_failures(&pending_nudge_store, pending_marker_failures);
        let received_hook = Arc::new(RecordingReceivedHook {
            message_store: Arc::clone(&message_store),
            emitted_ids: Mutex::new(Vec::new()),
            dispatches: Mutex::new(Vec::new()),
            saw_durable_record: AtomicBool::new(false),
            failure: hook_failure,
            cancelled_on_drop,
        });
        let home_dir = temporary_root.path().join("home");
        let current_dir = temporary_root.path().join("workspace");
        fs::create_dir_all(&home_dir).expect("create fixture home");
        fs::create_dir_all(&current_dir).expect("create fixture workspace");
        let health = RuntimeHealth::with_owner(99);
        let service_runtime = match template_composer {
            Some(composer) => assembly.service_runtime.with_template_composer(composer),
            None => assembly.service_runtime,
        };
        let service_runtime =
            attach_graft_receiver_store(service_runtime, &database_path, with_recipient);
        let service_runtime =
            service_runtime.with_pending_nudge_store(pending_nudge_store_for_runtime);
        let router = StorageAndNudgeRouter::new(
            service_runtime,
            Arc::new(NullObservability),
            select(received_hook.clone()),
            home_dir.clone(),
        )
        .with_runtime_health(health.clone(), assembly.doctor_ports);
        Fixture {
            _temporary_root: temporary_root,
            router,
            message_store,
            pending_nudge_store,
            received_hook,
            runtime_health: health,
            database_path,
            home_dir,
            current_dir,
        }
    }

    fn pending_store_with_failures(
        store: &Arc<dyn PendingNudgeStore + Send + Sync>,
        failures: Option<usize>,
    ) -> Arc<dyn PendingNudgeStore + Send + Sync> {
        match failures {
            Some(failures) => Arc::new(FailingMarkPendingStore {
                inner: Arc::clone(store),
                remaining_failures: AtomicUsize::new(failures),
            }),
            None => Arc::clone(store),
        }
    }

    fn seed_fixture_roster(
        roster_store: &Arc<dyn StorageRosterStore + Send + Sync>,
        team: &TeamName,
    ) {
        roster_store
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: ["recipient", "sender"]
                    .into_iter()
                    .map(|agent_name| RosterEntry {
                        team_name: team.clone(),
                        agent_name: agent_name.parse().expect("agent"),
                        member_kind: RosterMemberKind::Permanent,
                        harness: RosterHarness::PythonGraft,
                        agent_type: atm_core::schema::AgentType::default(),
                        model: ModelName::default(),
                        recipient_pane_id: None,
                        metadata_json: serde_json::Map::new(),
                    })
                    .collect(),
                refreshed_at: None,
            })
            .expect("seed recipient roster");
    }

    fn attach_graft_receiver_store(
        service_runtime: LocalServiceRuntime,
        database_path: &Path,
        with_recipient: bool,
    ) -> LocalServiceRuntime {
        let store = open_graft_receiver_endpoint_store(database_path)
            .expect("sqlite graft receiver endpoint store");
        if with_recipient {
            store
                .register(
                    &GraftReceiverRegistration {
                        team: "test-team".parse().expect("team"),
                        agent: "recipient".parse().expect("agent"),
                        endpoint: "127.0.0.1:9".parse().expect("endpoint"),
                        capability: atm_core::local_http::LocalCapability::generate()
                            .expect("capability"),
                        owner_generation: OwnerGeneration::new("01J00000000000000000000000")
                            .expect("owner generation"),
                    },
                    atm_core::types::IsoTimestamp::now().into_inner(),
                )
                .expect("register fixture graft receiver");
        }
        service_runtime.with_graft_receiver_endpoint_store(store)
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

    fn template_write_request(fixture: &Fixture, body: &str) -> WriteRequest {
        let template_path = fixture._temporary_root.path().join("notice.j2");
        std::fs::write(&template_path, body).expect("write template fixture");
        let raw_file_bytes = std::fs::read(&template_path).expect("read template fixture");
        let mut request = write_request(fixture.home_dir.clone(), fixture.current_dir.clone());
        request.message_source = SendMessageSource::Template(TemplateSendSource {
            canonical_template_path: std::fs::canonicalize(&template_path)
                .expect("canonical template path"),
            canonical_template_root: std::fs::canonicalize(fixture._temporary_root.path())
                .expect("canonical template root"),
            raw_file_bytes,
            input_defaults: serde_json::Map::new(),
            var_file_values: serde_json::Map::new(),
            explicit_values: serde_json::Map::new(),
            environment_values: serde_json::Map::new(),
        });
        request.classification = MessageClassification {
            category: Some("assignment".to_owned()),
            tags: vec!["phase-an".to_owned()],
            content_format: Some("markdown".to_owned()),
        };
        request
    }

    fn template_composer_for(body: &str) -> Arc<dyn atm_core::TemplateComposer> {
        Arc::new(FixtureTemplateComposer::new(body))
    }

    fn router(fixture: &Fixture, connector: AuthenticatedConnector) -> axum::Router {
        // Ordinary route tests are not deadline tests. Give their SQLite
        // admission and advisory hook enough headroom on slower CI hosts;
        // tests of deadline behavior select their one-second budget below.
        router_with_timeout(fixture, connector, Duration::from_secs(10))
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

    /// Hang detector for two-runtime router tests that dispatch a canonical
    /// write or ACK over a real reqwest client to a real, in-process peer
    /// HTTP runtime. This is not a timing assertion on how fast delivery
    /// should be: it exists only to fail a genuinely hung exchange instead of
    /// blocking `cargo test` forever. A fresh client build (TLS/connector
    /// init), TCP connect, remote accept, remote `commit_write` (including a
    /// SQLite fsync), and the synchronous received-hook path all happen
    /// inside this budget on a shared executor, so it must stay generous on
    /// loaded CI runners. See `DIRECT_PEER_TEST_SERVER_REQUEST_TIMEOUT` for
    /// the server-side counterpart, which must stay strictly larger than
    /// this value per the request-budget contract.
    const TWO_RUNTIME_TEST_REQUEST_BUDGET: Duration = Duration::from_secs(30);

    /// Hang detector for tests that host a real accept loop and a real
    /// client dispatch on the same executor (see `TWO_RUNTIME_TEST_REQUEST_BUDGET`
    /// for the client-side counterpart). This is not a timing assertion: it
    /// only needs to be comfortably larger than the client-side budget so a
    /// slow-but-correct exchange cannot be starved by the server closing the
    /// connection first. Per the request-budget contract, the server budget
    /// must stay strictly greater than the client budget.
    const DIRECT_PEER_TEST_SERVER_REQUEST_TIMEOUT: Duration = Duration::from_secs(35);

    fn direct_peer_runtime_config(
        fixture: &Fixture,
        direct_peer: crate::DirectPeerTcpConfig,
    ) -> HttpRuntimeConfig {
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
                NonZeroDuration::new(DIRECT_PEER_TEST_SERVER_REQUEST_TIMEOUT)
                    .expect("request timeout"),
                NonZeroDuration::new(Duration::from_secs(1)).expect("shutdown timeout"),
            ),
        )
        .with_direct_peer_tcp(direct_peer)
    }

    #[tokio::test]
    async fn blocking_core_bridge_rejects_saturation_without_starting_a_second_job() {
        let admission = BlockingCoreBridge::new(
            NonZeroUsize::new(1).expect("non-zero capacity"),
            RuntimeHealth::default(),
        );
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
    async fn expired_blocking_core_bridge_never_starts_a_blocking_job() {
        let admission = BlockingCoreBridge::new(
            NonZeroUsize::new(1).expect("non-zero capacity"),
            RuntimeHealth::default(),
        );
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

    /// Busy-waits for `duration` on the calling thread.
    ///
    /// Test-only stand-in for a slow blocking job. A thread-sleep primitive
    /// is deliberately avoided: the AL.3 architecture boundary guard forbids
    /// `atm-http-runtime` sources, including tests, from restoring blocking
    /// sleep constructs.
    fn spin_wait(duration: Duration) {
        let deadline = std::time::Instant::now() + duration;
        while std::time::Instant::now() < deadline {
            std::hint::spin_loop();
        }
    }

    #[tokio::test]
    async fn blocking_core_bridge_records_a_stall_when_a_job_outlives_its_budget() {
        let health = RuntimeHealth::default();
        let admission = BlockingCoreBridge::new(
            NonZeroUsize::new(1).expect("non-zero capacity"),
            health.clone(),
        );

        // A fake slow job: it runs longer than the remaining budget it was
        // dispatched with. The bridge must let it run to completion (a
        // durable write is never abandoned mid-flight) and only record the
        // stall as an observability event. This busy-waits deliberately
        // instead of sleeping the blocking thread: the AL.3 boundary guard
        // forbids `atm-http-runtime` from restoring blocking-runtime sleep
        // constructs anywhere in its sources, tests included.
        let result = admission
            .run(
                RequestDeadline::after(Duration::from_millis(20)),
                move || {
                    spin_wait(Duration::from_millis(150));
                    Ok("slow job still finished")
                },
            )
            .await
            .expect("an outrunning job still completes and returns its result");

        assert_eq!(result, "slow job still finished");
        assert_eq!(
            health.snapshot().blocking_core_bridge_stalls_total,
            1,
            "a job outliving its budget must be recorded as one bridge stall"
        );
    }

    #[tokio::test]
    async fn blocking_core_bridge_does_not_record_a_stall_for_jobs_within_budget() {
        let health = RuntimeHealth::default();
        let admission = BlockingCoreBridge::new(
            NonZeroUsize::new(1).expect("non-zero capacity"),
            health.clone(),
        );

        admission
            .run(RequestDeadline::after(Duration::from_secs(1)), || Ok(()))
            .await
            .expect("a fast job completes within its budget");

        assert_eq!(
            health.snapshot().blocking_core_bridge_stalls_total,
            0,
            "a job that stays within its budget must not be recorded as a stall"
        );
    }

    #[test]
    fn aq2_crit_002_marker_failure_retries_once_and_preserves_write_success() {
        let health = RuntimeHealth::default();
        let attempts = AtomicUsize::new(0);
        let result = retry_deferred_marker(&health, || {
            let attempt = attempts.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                Err(AtmError::mailbox_write("marker test failure"))
            } else {
                Ok(())
            }
        });

        assert!(
            result.is_ok(),
            "a successful retry must preserve the write result"
        );
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            2,
            "marker failure retries once"
        );
        assert_eq!(health.snapshot().queue_marker_set_failures_total, 1);
    }

    fn hook_event() -> PostSendHookEvent {
        PostSendHookEvent {
            sender: "sender".parse().expect("sender"),
            sender_chat_id: None,
            sender_team: "test-team".parse().expect("sender team"),
            sender_host: None,
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
            dispatches: Mutex::new(Vec::new()),
            saw_durable_record: AtomicBool::new(false),
            failure: None,
            cancelled_on_drop: None,
        });
        let graft = Arc::new(RecordingReceivedHook {
            message_store: Arc::clone(&fixture.message_store),
            emitted_ids: Mutex::new(Vec::new()),
            dispatches: Mutex::new(Vec::new()),
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
            target: PostSendBuiltInTarget::LocalSteer(LocalSteerTarget::Tmux(
                LocalTmuxNudgeTarget {
                    pane_id: PaneId::from_cli("%1").expect("pane"),
                    rendered_nudge: "tmux nudge".to_owned(),
                },
            )),
            kind: NudgeKind::Steer,
        };
        let graft_dispatch = BuiltInPostSendDispatch {
            event: hook_event(),
            target: PostSendBuiltInTarget::Graft(GraftNudgeTarget {
                recipient: "recipient".parse().expect("recipient"),
                recipient_team: "test-team".parse().expect("team"),
                rendered_nudge: "<atm kind=\"nudge\"/>".to_owned(),
                message_body: "message body".to_owned(),
            }),
            kind: NudgeKind::Steer,
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

    #[tokio::test(start_paused = true)]
    async fn started_write_retains_its_actual_result_after_the_advisory_deadline() {
        let admission = BlockingCoreBridge::new(
            NonZeroUsize::new(1).expect("non-zero capacity"),
            RuntimeHealth::default(),
        );
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
        tokio::time::advance(Duration::from_millis(30)).await;
        release_tx.send(()).expect("release started job");
        assert_eq!(
            task.await.expect("task joins").expect("actual result"),
            "actual durable result",
            "a started transaction is not reclassified as a deadline failure"
        );
    }

    #[tokio::test]
    async fn blocking_core_bridge_returns_the_underlying_storage_error() {
        let admission = BlockingCoreBridge::new(
            NonZeroUsize::new(1).expect("non-zero capacity"),
            RuntimeHealth::default(),
        );
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
            ResponseEnvelope::Heartbeat(response)
                if !response.pid_changed
                    && response.state == atm_core::protocol::RuntimeMemberState::Active
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
            ResponseEnvelope::Heartbeat(response)
                if response.pid_changed
                    && response.state == atm_core::protocol::RuntimeMemberState::Offline
        ));
        // Health remains listener-owned. A member becoming offline does not
        // make a process that is still serving local adapters become NotReady.
        assert_eq!(
            fixture.router.runtime_health.snapshot().readiness,
            RuntimeReadinessState::Unavailable,
            "the fixture has no running listener; heartbeat cannot claim readiness"
        );
    }

    /// AC1: a deterministic (non-wall-clock) `observed_at` proves the
    /// existing Heartbeat route drives `RuntimeHealth`'s member-state
    /// projection end to end. AQ3's own observation sink does not exist yet
    /// (this sprint is upstream of AQ3), so this test covers the AC1 claim
    /// as it is actually implementable today: the router's real dispatch
    /// path into `RuntimeHealth::record_heartbeat` and its snapshot.
    #[tokio::test]
    async fn heartbeat_route_drives_runtime_health_member_state_transitions_with_a_deterministic_clock()
     {
        let fixture = fixture(true, None, None);
        let observed_at: atm_core::types::IsoTimestamp = "2026-01-01T00:00:00Z"
            .parse()
            .expect("deterministic fixed timestamp, not wall-clock `now()`");
        let request = TeamMemberHeartbeatRequest {
            team: "test-team".parse().expect("team"),
            member: "recipient".parse().expect("agent"),
            pid: 7,
            observed_at,
            activity: HeartbeatActivity::ActiveToolUse,
            session_id: None,
        };
        fixture
            .router
            .dispatch(
                ApiRequest::new(RequestEnvelope::Heartbeat(request)),
                atm_core::AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("authorized heartbeat");

        let snapshot = fixture.router.runtime_health.snapshot();
        let member = snapshot
            .members
            .iter()
            .find(|observation| observation.member.as_str() == "recipient")
            .expect("heartbeat route projected the member into RuntimeHealth");
        assert_eq!(
            member.state,
            atm_core::protocol::RuntimeMemberState::Active,
            "an active-tool-use heartbeat drives the member to Active"
        );
        assert_eq!(
            member.last_active_at,
            Some(observed_at),
            "the projection preserves the caller-supplied deterministic timestamp"
        );

        let idle_request = TeamMemberHeartbeatRequest {
            team: "test-team".parse().expect("team"),
            member: "recipient".parse().expect("agent"),
            pid: 7,
            observed_at,
            activity: HeartbeatActivity::Idle,
            session_id: None,
        };
        fixture
            .router
            .dispatch(
                ApiRequest::new(RequestEnvelope::Heartbeat(idle_request)),
                atm_core::AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("second authorized heartbeat");
        let idle_state = fixture
            .router
            .runtime_health
            .snapshot()
            .members
            .into_iter()
            .find(|observation| observation.member.as_str() == "recipient")
            .expect("member remains projected")
            .state;
        assert_eq!(
            idle_state,
            atm_core::protocol::RuntimeMemberState::Idle,
            "an idle heartbeat transitions the projected member state to Idle"
        );
    }

    /// AC5: a caller not on the roster is rejected by the real
    /// `queue_get_next` handler, not merely the wire codec.
    #[tokio::test]
    async fn queue_get_next_router_rejects_a_caller_not_on_the_roster() {
        let fixture = fixture(true, None, None);
        let request = QueueGetNextRequest {
            team: "test-team".parse().expect("team"),
            member: "not-on-the-roster".parse().expect("agent"),
        };
        let error = fixture
            .router
            .dispatch(
                ApiRequest::new(RequestEnvelope::QueueGetNext(request)),
                atm_core::AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect_err("a non-roster member must be rejected");
        assert_eq!(error.code(), atm_core::error::AtmErrorCode::AgentNotFound);
    }

    /// AC7/AC3: the real `queue_get_next` handler (not the FIFO helper in
    /// isolation) drains a pre-seeded bare-CLI FIFO entry for an
    /// authenticated roster member.
    #[tokio::test]
    async fn queue_get_next_router_drains_the_bare_cli_fifo_through_the_real_dispatch_path() {
        let fixture = fixture(true, None, None);
        let fifo: BareCliFifo = Default::default();
        let drops: BareCliQueueFullDrops = Default::default();
        let router = fixture
            .router
            .clone()
            .with_bare_cli_fifo(fifo.clone(), drops);
        let member = MemberKey::new(
            "test-team".parse().expect("team"),
            "recipient".parse().expect("agent"),
        );
        append_bare_cli_message(
            &fifo,
            &Default::default(),
            member,
            QueuedNudgeMessage {
                kind: NudgeKind::Queue,
                msg_id: AtmMessageId::new(),
                body: "queued through the real handler".to_owned(),
            },
        )
        .expect("seed FIFO");

        let request = QueueGetNextRequest {
            team: "test-team".parse().expect("team"),
            member: "recipient".parse().expect("agent"),
        };
        let response = router
            .dispatch(
                ApiRequest::new(RequestEnvelope::QueueGetNext(request)),
                atm_core::AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("authorized queue-get");
        let ResponseEnvelope::QueueGetNext(response) = response.into_inner() else {
            panic!("expected a QueueGetNext response");
        };
        assert_eq!(response.messages.len(), 1);
        assert_eq!(response.messages[0].body, "queued through the real handler");
    }

    /// AC6 migration case: a stale FIFO entry from an earlier bare-CLI
    /// window still drains on `queue_get_next` even after the member's
    /// roster/lease inputs have since changed. `queue_get_next` never
    /// re-runs the classifier (FIFO existence wins, critical review I15);
    /// this proves that invariant against the real handler, not just the
    /// classifier function in isolation.
    #[tokio::test]
    async fn queue_get_next_router_drains_a_stale_fifo_entry_after_the_members_classification_changes()
     {
        let fixture = fixture(true, None, None);
        let fifo: BareCliFifo = Default::default();
        let drops: BareCliQueueFullDrops = Default::default();
        let team: TeamName = "test-team".parse().expect("team");
        let member = MemberKey::new(team.clone(), "recipient".parse().expect("agent"));
        append_bare_cli_message(
            &fifo,
            &Default::default(),
            member.clone(),
            QueuedNudgeMessage {
                kind: NudgeKind::Queue,
                msg_id: AtmMessageId::new(),
                body: "queued before the migration".to_owned(),
            },
        )
        .expect("seed stale FIFO entry");

        // Flip the roster input the classifier reads: this member now
        // resolves to a tmux local backend (would classify TmuxSteer for
        // any *new* dispatch), simulating a migration away from bare-CLI
        // since the FIFO entry was queued.
        let assembly_roster =
            atm_runtime_test_support::open_sqlite_boundary(&fixture.database_path)
                .expect("reopen SQLite boundary for the roster mutation");
        assembly_roster
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: ["recipient", "sender"]
                    .into_iter()
                    .map(|agent_name| RosterEntry {
                        team_name: team.clone(),
                        agent_name: agent_name.parse().expect("agent"),
                        member_kind: RosterMemberKind::Permanent,
                        harness: RosterHarness::PythonGraft,
                        agent_type: atm_core::schema::AgentType::default(),
                        model: ModelName::default(),
                        recipient_pane_id: if agent_name == "recipient" {
                            Some(PaneId::from_cli("%9").expect("pane"))
                        } else {
                            None
                        },
                        metadata_json: serde_json::Map::new(),
                    })
                    .collect(),
                refreshed_at: None,
            })
            .expect("flip recipient onto a tmux local backend");

        let router = fixture.router.clone().with_bare_cli_fifo(fifo, drops);
        let request = QueueGetNextRequest {
            team,
            member: "recipient".parse().expect("agent"),
        };
        let response = router
            .dispatch(
                ApiRequest::new(RequestEnvelope::QueueGetNext(request)),
                atm_core::AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("authorized queue-get");
        let ResponseEnvelope::QueueGetNext(response) = response.into_inner() else {
            panic!("expected a QueueGetNext response");
        };
        assert_eq!(
            response.messages.len(),
            1,
            "the stale FIFO entry still drains after the member's classification inputs changed"
        );
        assert_eq!(response.messages[0].body, "queued before the migration");
    }

    #[tokio::test]
    async fn doctor_reports_bootstrap_injected_server_version() {
        let fixture = fixture(true, None, None);
        let assembly =
            open_sqlite_boundary(&fixture.database_path).expect("reopen doctor boundary");
        let doctor_projection = StorageDoctorProjection::start(
            DoctorProjectionConfig::default(),
            assembly.service_runtime,
            assembly.doctor_ports,
            Arc::new(NullObservability),
        )
        .expect("start doctor projection");
        let daemon_context = atm_core::doctor::DoctorExecutionContext {
            team: Some("daemon-team".parse().expect("team")),
            identity: Some("daemon-agent".parse().expect("agent")),
            version: Some(atm_core::protocol::ReleaseVersion::current()),
            cli_schema_version: Some(atm_core::protocol::CLI_SCHEMA_VERSION),
            http_api_version: Some(atm_core::protocol::HttpApiVersion::current()),
            peer_wire_security: None,
        };
        let response = fixture
            .router
            .clone()
            .with_doctor_projection(Arc::new(doctor_projection))
            .with_daemon_context(daemon_context.clone())
            .dispatch(
                ApiRequest::new(atm_core::protocol::RequestEnvelope::Doctor(
                    atm_core::doctor::DoctorQuery::default(),
                )),
                atm_core::AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("doctor response");
        match response.into_inner() {
            ResponseEnvelope::Doctor(report) => {
                assert_eq!(report.daemon_context, Some(daemon_context));
                assert_eq!(report.herdr_queue_pump.last_tick_at, None);
                assert_eq!(report.herdr_queue_pump.breaker, report.herdr_breaker);
            }
            other => panic!("expected doctor report, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn doctor_projection_serves_parallel_control_requests_without_the_read_bridge() {
        let fixture = fixture(true, None, None);
        let assembly =
            open_sqlite_boundary(&fixture.database_path).expect("reopen doctor boundary");
        let projection = Arc::new(
            StorageDoctorProjection::start(
                DoctorProjectionConfig::default(),
                assembly.service_runtime,
                assembly.doctor_ports,
                Arc::new(NullObservability),
            )
            .expect("start doctor projection"),
        );
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let projection = Arc::clone(&projection);
            tasks.push(tokio::spawn(async move {
                projection
                    .project(
                        atm_core::doctor::DoctorQuery::default(),
                        DoctorProjectionContext::default(),
                        RequestDeadline::after(Duration::from_secs(1)),
                    )
                    .await
            }));
        }
        for task in tasks {
            task.await
                .expect("join doctor request")
                .expect("parallel doctor response");
        }
        assert!(
            !projection.workers_finished(),
            "a healthy projection retains its bounded control workers"
        );
    }

    #[tokio::test]
    async fn doctor_projection_rejects_control_lane_overload_explicitly() {
        let fixture = fixture(true, None, None);
        let assembly =
            open_sqlite_boundary(&fixture.database_path).expect("reopen doctor boundary");
        let projection = Arc::new(
            StorageDoctorProjection::start(
                DoctorProjectionConfig {
                    worker_count: 1,
                    queue_depth: 1,
                },
                assembly.service_runtime,
                assembly.doctor_ports,
                Arc::new(NullObservability),
            )
            .expect("start bounded doctor projection"),
        );
        let barrier = Arc::new(tokio::sync::Barrier::new(65));
        let mut calls = Vec::new();
        for _ in 0..64 {
            let projection = Arc::clone(&projection);
            let barrier = Arc::clone(&barrier);
            calls.push(tokio::spawn(async move {
                barrier.wait().await;
                projection
                    .project(
                        atm_core::doctor::DoctorQuery::default(),
                        DoctorProjectionContext::default(),
                        RequestDeadline::after(Duration::from_secs(1)),
                    )
                    .await
            }));
        }
        barrier.wait().await;

        let mut completed = 0;
        let mut saturated = 0;
        for call in calls {
            match call.await.expect("doctor caller joins") {
                Ok(_) => completed += 1,
                Err(error)
                    if error.code() == atm_storage::AtmErrorCode::DaemonConnectionSaturated =>
                {
                    saturated += 1;
                }
                Err(error) => panic!("unexpected doctor overload result: {error}"),
            }
        }
        assert!(completed > 0, "admitted doctor calls retain healthy parity");
        assert!(
            saturated > 0,
            "beyond the bounded control lane, doctor rejects explicitly instead of serializing"
        );
    }

    #[test]
    fn mailbox_and_doctor_handlers_never_enter_the_blocking_core_bridge() {
        let source = include_str!("storage_and_nudge_router.rs")
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .expect("production source");
        for handler in [
            "list_messages",
            "peek_messages",
            "receive_messages",
            "doctor",
        ] {
            let start = source
                .find(&format!("async fn {handler}"))
                .expect("handler exists");
            let tail = &source[start..];
            let end = tail.find("\n    async fn ").unwrap_or(tail.len());
            assert!(
                !tail[..end].contains("blocking_core_bridge"),
                "{handler} must not acquire the global blocking bridge"
            );
        }
    }

    #[tokio::test]
    async fn mailbox_handlers_use_the_async_runtime_for_list_peek_and_read() {
        let fixture = fixture(true, None, None);
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone());
        fixture
            .router
            .dispatch(
                ApiRequest::new(RequestEnvelope::Write(Box::new(write))),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("seed mailbox through canonical write path");

        let assembly = open_sqlite_boundary(&fixture.database_path).expect("reopen async boundary");
        let async_mailbox_runtime = assembly
            .async_mailbox_runtime
            .with_state_handoff(HandoffConfig::default())
            .expect("start async mailbox handoff");
        let router = fixture
            .router
            .clone()
            .with_async_mailbox_runtime(Arc::new(async_mailbox_runtime));
        let root = fixture.home_dir.clone();
        let list = atm_core::list::ListQuery::new(
            root.clone(),
            root.clone(),
            "recipient".parse().expect("recipient"),
            None,
            "test-team".parse().expect("team"),
            atm_core::types::ReadSelection::All,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("list query");
        let listed = router
            .dispatch(
                ApiRequest::Messages(Box::new(atm_core::api::MessageCollectionRequest::List(
                    list,
                ))),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("list through async mailbox runtime")
            .into_inner();
        assert!(matches!(listed, ResponseEnvelope::List(outcome) if outcome.count == 1));

        let peek = atm_core::read::PeekQuery::new(
            root.clone(),
            root.clone(),
            "recipient".parse().expect("recipient"),
            None,
            "test-team".parse().expect("team"),
            atm_core::types::ReadSelection::All,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("peek query");
        let peeked = router
            .dispatch(
                ApiRequest::Messages(Box::new(atm_core::api::MessageCollectionRequest::Peek(
                    peek,
                ))),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("peek through async mailbox runtime")
            .into_inner();
        assert!(matches!(peeked, ResponseEnvelope::Peek(outcome) if outcome.count == 1));

        let read = atm_core::read::ReadQuery::new(
            root.clone(),
            root,
            "recipient".parse().expect("recipient"),
            None,
            "test-team".parse().expect("team"),
            atm_core::types::ReadSelection::All,
            false,
            true,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read query");
        let read = router
            .dispatch(
                ApiRequest::Messages(Box::new(atm_core::api::MessageCollectionRequest::Receive(
                    read,
                ))),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("read through async mailbox runtime")
            .into_inner();
        assert!(
            matches!(read, ResponseEnvelope::Receive(outcome) if outcome.count == 1 && outcome.mutation_applied)
        );
    }

    #[tokio::test]
    async fn mailbox_and_doctor_fanout_stays_live_while_the_legacy_bridge_is_occupied() {
        let fixture = fixture(true, None, None);
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone());
        fixture
            .router
            .dispatch(
                ApiRequest::new(RequestEnvelope::Write(Box::new(write))),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("seed mailbox");

        let assembly = open_sqlite_boundary(&fixture.database_path).expect("reopen async boundary");
        let async_mailbox_runtime = assembly
            .async_mailbox_runtime
            .with_state_handoff(HandoffConfig::default())
            .expect("start mailbox handoff");
        let doctor_projection = StorageDoctorProjection::start(
            DoctorProjectionConfig::default(),
            assembly.service_runtime,
            assembly.doctor_ports,
            Arc::new(NullObservability),
        )
        .expect("start doctor projection");
        let router = fixture
            .router
            .clone()
            .with_async_mailbox_runtime(Arc::new(async_mailbox_runtime))
            .with_doctor_projection(Arc::new(doctor_projection));
        let bridge = router.blocking_core_bridge.clone();
        let bridge_task = tokio::spawn(async move {
            bridge
                .run(RequestDeadline::after(Duration::from_secs(1)), || {
                    spin_wait(Duration::from_millis(250));
                    Ok(())
                })
                .await
        });
        tokio::task::yield_now().await;

        let root = fixture.home_dir.clone();
        let list = atm_core::list::ListQuery::new(
            root.clone(),
            root.clone(),
            "recipient".parse().expect("recipient"),
            None,
            "test-team".parse().expect("team"),
            atm_core::types::ReadSelection::All,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("list query");
        let peek = atm_core::read::PeekQuery::new(
            root.clone(),
            root.clone(),
            "recipient".parse().expect("recipient"),
            None,
            "test-team".parse().expect("team"),
            atm_core::types::ReadSelection::All,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("peek query");
        let read = atm_core::read::ReadQuery::new(
            root.clone(),
            root,
            "recipient".parse().expect("recipient"),
            None,
            "test-team".parse().expect("team"),
            atm_core::types::ReadSelection::All,
            false,
            true,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read query");
        let mut calls = Vec::new();
        for index in 0..12 {
            let router = router.clone();
            let request = match index % 4 {
                0 => ApiRequest::Messages(Box::new(atm_core::api::MessageCollectionRequest::List(
                    list.clone(),
                ))),
                1 => ApiRequest::Messages(Box::new(atm_core::api::MessageCollectionRequest::Peek(
                    peek.clone(),
                ))),
                2 => ApiRequest::Messages(Box::new(
                    atm_core::api::MessageCollectionRequest::Receive(read.clone()),
                )),
                _ => ApiRequest::Doctor(atm_core::doctor::DoctorQuery::default()),
            };
            calls.push(tokio::spawn(async move {
                router
                    .dispatch(
                        request,
                        AuthenticatedIngress::Local,
                        RequestDeadline::after(Duration::from_secs(1)),
                    )
                    .await
            }));
        }
        for call in calls {
            call.await
                .expect("fanout task joins")
                .expect("fanout request completes while bridge is occupied");
        }
        bridge_task
            .await
            .expect("bridge task joins")
            .expect("bridge task completes");
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
    async fn graft_receiver_handlers_enforce_local_roster_and_loopback_contracts() {
        let fixture = fixture(true, None, None);
        let registration = GraftReceiverRegistration {
            team: "test-team".parse().expect("team"),
            agent: "recipient".parse().expect("agent"),
            endpoint: "127.0.0.1:43101".parse().expect("endpoint"),
            capability: atm_core::local_http::LocalCapability::generate().expect("capability"),
            owner_generation: OwnerGeneration::new("01J00000000000000000000000")
                .expect("owner generation"),
        };
        let response = fixture
            .router
            .clone()
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverRegister(registration.clone())),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("register response");
        assert!(matches!(
            response.into_inner(),
            ResponseEnvelope::GraftReceiverRegister
        ));

        let lookup = fixture
            .router
            .clone()
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverLookup {
                    team: registration.team.clone(),
                    agent: registration.agent.clone(),
                }),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("lookup response")
            .into_inner();
        assert!(matches!(
            lookup,
            ResponseEnvelope::GraftReceiverLookup(Some(lease))
                if lease.endpoint == registration.endpoint
                    && lease.owner_generation == registration.owner_generation
        ));

        let non_local = fixture
            .router
            .clone()
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverRegister(registration.clone())),
                AuthenticatedIngress::Peer,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await;
        assert!(
            non_local.is_err(),
            "peer ingress must not register receivers"
        );

        let unknown = fixture
            .router
            .clone()
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverLookup {
                    team: registration.team.clone(),
                    agent: "unknown".parse().expect("agent"),
                }),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await;
        assert!(unknown.is_err(), "unknown roster members must be rejected");

        let mut non_loopback = registration;
        non_loopback.endpoint = "192.0.2.1:43101".parse().expect("endpoint");
        let rejected_endpoint = fixture
            .router
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverRegister(non_loopback)),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await;
        assert!(
            rejected_endpoint.is_err(),
            "non-loopback endpoint must be rejected"
        );
    }

    // AC7 (PR #1048, QA-2): `GraftReceiverLookup` round-trips through the
    // *actual* `atm-http-runtime` HTTP route `_internal-nudge` calls in
    // production (`lookup_receiver_via` in `atm/src/commands/internal_nudge.rs`
    // POSTs `GraftReceiverLookupRequest` JSON to this exact path and decodes
    // an `Option<GraftReceiverLease>` JSON body). Earlier coverage only drove
    // `lookup_receiver_via` against a hand-rolled `FakeClientTransport`, which
    // could not catch a divergence in the real wire contract (path, method,
    // request/response JSON shape). This test builds the production
    // `canonical_api_router` over the real `StorageAndNudgeRouter` fixture
    // (same helper other route tests in this module use) and drives it with
    // an actual `tower::Service::oneshot` HTTP call for both a present lease
    // (`recipient`, seeded by `attach_graft_receiver_store`) and a roster
    // member with no registered lease (`sender`) — no fake stands in for the
    // router or store at any point.
    #[tokio::test]
    async fn graft_receiver_lookup_round_trips_hit_and_absent_lease_through_the_real_http_route() {
        let fixture = fixture(true, None, None);
        let app = canonical_api_router(
            Arc::new(fixture.router.clone()),
            AuthenticatedConnector::local(),
            RuntimeLimits::new(
                std::num::NonZeroUsize::new(4096).expect("non-zero body limit"),
                std::num::NonZeroUsize::new(4).expect("non-zero request limit"),
            ),
            RuntimeTimeouts::new(
                NonZeroDuration::new(Duration::from_secs(10)).expect("non-zero request timeout"),
                NonZeroDuration::new(Duration::from_secs(1)).expect("non-zero shutdown timeout"),
            ),
        );
        let lookup_path = atm_core::api::http_route_surface()
            .find(|route| {
                route.method == "POST" && route.path_template.contains("/graft/receiver/lookup")
            })
            .expect("graft receiver lookup route")
            .path_template;

        // Hit: a roster member with a registered lease resolves it through
        // the real HTTP decode -> dispatch -> SQLite store -> HTTP encode
        // path.
        let hit_body = serde_json::to_vec(&atm_core::protocol::GraftReceiverLookupRequest {
            team: "test-team".parse().expect("team"),
            agent: "recipient".parse().expect("agent"),
        })
        .expect("serialize lookup request");
        let hit_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(lookup_path)
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(hit_body))
                    .expect("HTTP request"),
            )
            .await
            .expect("infallible Axum service");
        assert_eq!(hit_response.status(), StatusCode::OK);
        let hit_bytes = to_bytes(hit_response.into_body(), usize::MAX)
            .await
            .expect("hit response body");
        let hit_lease: Option<atm_storage::GraftReceiverLease> =
            serde_json::from_slice(&hit_bytes).expect("decode hit lease");
        assert_eq!(
            hit_lease
                .expect("recipient has a registered lease")
                .endpoint,
            "127.0.0.1:9".parse().expect("fixture endpoint"),
            "the real route must resolve the fixture-registered lease"
        );

        // Absent lease: a roster member with no registration is `Ok(None)`
        // over HTTP, exactly what `lookup_receiver_via` maps into the
        // not-registered error client-side.
        let absent_body = serde_json::to_vec(&atm_core::protocol::GraftReceiverLookupRequest {
            team: "test-team".parse().expect("team"),
            agent: "sender".parse().expect("agent"),
        })
        .expect("serialize lookup request");
        let absent_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(lookup_path)
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(absent_body))
                    .expect("HTTP request"),
            )
            .await
            .expect("infallible Axum service");
        assert_eq!(absent_response.status(), StatusCode::OK);
        let absent_bytes = to_bytes(absent_response.into_body(), usize::MAX)
            .await
            .expect("absent response body");
        let absent_lease: Option<atm_storage::GraftReceiverLease> =
            serde_json::from_slice(&absent_bytes).expect("decode absent lease");
        assert_eq!(
            absent_lease, None,
            "a known roster member with no registered lease must be Ok(None), not an error, \
             over the real HTTP route"
        );
    }

    // AC 4: register and unregister must both reject non-local ingress and an
    // unknown roster member, independent of the register/lookup coverage
    // above.
    #[tokio::test]
    async fn graft_receiver_register_and_unregister_reject_non_local_ingress_and_unknown_members() {
        let fixture = fixture(true, None, None);
        let team: TeamName = "test-team".parse().expect("team");
        let known_agent: AgentName = "recipient".parse().expect("agent");
        let unknown_agent: AgentName = "unknown".parse().expect("agent");
        let generation = OwnerGeneration::new("01J00000000000000000000010").expect("generation");
        let registration = GraftReceiverRegistration {
            team: team.clone(),
            agent: known_agent.clone(),
            endpoint: "127.0.0.1:43105".parse().expect("endpoint"),
            capability: atm_core::local_http::LocalCapability::generate().expect("capability"),
            owner_generation: generation.clone(),
        };
        let unregistration = GraftReceiverUnregistration {
            team: team.clone(),
            agent: known_agent.clone(),
            owner_generation: generation.clone(),
        };

        let non_local_register = fixture
            .router
            .clone()
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverRegister(registration.clone())),
                AuthenticatedIngress::Peer,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await;
        assert!(
            non_local_register.is_err(),
            "peer ingress must not register receivers"
        );

        let non_local_unregister = fixture
            .router
            .clone()
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverUnregister(
                    unregistration.clone(),
                )),
                AuthenticatedIngress::Peer,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await;
        assert!(
            non_local_unregister.is_err(),
            "peer ingress must not unregister receivers"
        );

        let mut unknown_registration = registration.clone();
        unknown_registration.agent = unknown_agent.clone();
        let unknown_member_register = fixture
            .router
            .clone()
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverRegister(unknown_registration)),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await;
        assert!(
            unknown_member_register.is_err(),
            "an unknown roster member must not be registered"
        );

        let unknown_unregistration = GraftReceiverUnregistration {
            team: team.clone(),
            agent: unknown_agent,
            owner_generation: generation,
        };
        let unknown_member_unregister = fixture
            .router
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverUnregister(
                    unknown_unregistration,
                )),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await;
        assert!(
            unknown_member_unregister.is_err(),
            "an unknown roster member must not be unregistered"
        );
    }

    // Closes the RBQA-F001 / real-gap finding on AQ1.6 PR #1046: the
    // generation-checked `GraftReceiverEndpointStore::refresh` keepalive must
    // be reachable through the daemon client route (not only `register`'s
    // unconditional upsert), and a foreign-generation refresh must surface as
    // `AtmErrorCode::GraftReceiverNotOwner`, not a generic error.
    #[tokio::test]
    async fn graft_receiver_refresh_round_trips_owner_match_and_rejects_foreign_generation() {
        let fixture = fixture(true, None, None);
        let team: TeamName = "test-team".parse().expect("team");
        let agent: AgentName = "recipient".parse().expect("agent");
        let owner_generation =
            OwnerGeneration::new("01J00000000000000000000020").expect("generation");
        let registration = GraftReceiverRegistration {
            team: team.clone(),
            agent: agent.clone(),
            endpoint: "127.0.0.1:43110".parse().expect("endpoint"),
            capability: atm_core::local_http::LocalCapability::generate().expect("capability"),
            owner_generation: owner_generation.clone(),
        };
        fixture
            .router
            .clone()
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverRegister(registration)),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("register response");

        let refresh = atm_core::protocol::GraftReceiverRefreshRequest {
            team: team.clone(),
            agent: agent.clone(),
            owner_generation: owner_generation.clone(),
        };
        let refreshed = fixture
            .router
            .clone()
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverRefresh(refresh)),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("owner-matched refresh must succeed")
            .into_inner();
        assert!(matches!(refreshed, ResponseEnvelope::GraftReceiverRefresh));

        let foreign_generation =
            OwnerGeneration::new("01J00000000000000000000021").expect("generation");
        let foreign_refresh = atm_core::protocol::GraftReceiverRefreshRequest {
            team: team.clone(),
            agent: agent.clone(),
            owner_generation: foreign_generation,
        };
        let rejected = fixture
            .router
            .clone()
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverRefresh(foreign_refresh)),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect_err("a foreign generation must be rejected, not silently accepted");
        assert_eq!(
            rejected.code(),
            atm_core::error_codes::AtmErrorCode::GraftReceiverNotOwner
        );

        let non_local_refresh = atm_core::protocol::GraftReceiverRefreshRequest {
            team,
            agent,
            owner_generation,
        };
        let non_local = fixture
            .router
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverRefresh(non_local_refresh)),
                AuthenticatedIngress::Peer,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await;
        assert!(
            non_local.is_err(),
            "peer ingress must not refresh receivers"
        );
    }

    // AC 6: `GraftReceiverLookup` round-trips: hit, miss (`Ok(None)`, not an
    // error), and non-local ingress rejected.
    #[tokio::test]
    async fn graft_receiver_lookup_round_trips_hit_miss_and_rejects_non_local_ingress() {
        let fixture = fixture(true, None, None);
        let team: TeamName = "test-team".parse().expect("team");
        let registered_agent: AgentName = "recipient".parse().expect("agent");
        let unleased_agent: AgentName = "sender".parse().expect("agent");
        let registration = GraftReceiverRegistration {
            team: team.clone(),
            agent: registered_agent.clone(),
            endpoint: "127.0.0.1:43106".parse().expect("endpoint"),
            capability: atm_core::local_http::LocalCapability::generate().expect("capability"),
            owner_generation: OwnerGeneration::new("01J00000000000000000000011")
                .expect("generation"),
        };
        fixture
            .router
            .clone()
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverRegister(registration.clone())),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("register response");

        // Hit: a registered member returns its lease.
        let hit = fixture
            .router
            .clone()
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverLookup {
                    team: team.clone(),
                    agent: registered_agent,
                }),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("lookup response")
            .into_inner();
        assert!(matches!(
            hit,
            ResponseEnvelope::GraftReceiverLookup(Some(lease))
                if lease.endpoint == registration.endpoint
        ));

        // Miss: a known roster member with no registered lease is `Ok(None)`,
        // not an error.
        let miss = fixture
            .router
            .clone()
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverLookup {
                    team: team.clone(),
                    agent: unleased_agent,
                }),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("lookup response for a member with no lease")
            .into_inner();
        assert!(
            matches!(miss, ResponseEnvelope::GraftReceiverLookup(None)),
            "a known member with no registered lease must be Ok(None), not an error"
        );

        // Non-local ingress is rejected with the same gating as
        // register/unregister.
        let non_local = fixture
            .router
            .dispatch(
                ApiRequest::new(RequestEnvelope::GraftReceiverLookup {
                    team,
                    agent: "recipient".parse().expect("agent"),
                }),
                AuthenticatedIngress::Peer,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await;
        assert!(non_local.is_err(), "peer ingress must not look up leases");
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

    #[tokio::test]
    async fn deferred_write_through_async_router_marks_once_without_tokio_sqlite_access() {
        let fixture = fixture(true, None, None);
        let message_id = AtmMessageId::new();
        let timestamp = atm_core::types::IsoTimestamp::now();
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone())
            .with_origin_metadata(message_id, timestamp)
            .with_nudge_mode(NudgeMode::Deferred);

        let response = post_write(router(&fixture, AuthenticatedConnector::local()), &write).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        assert!(
            fixture
                .received_hook
                .emitted_ids
                .lock()
                .expect("inspect deferred hook emissions")
                .len()
                == 1,
            "a graft recipient receives its queue-shaped handoff at write time"
        );
        assert_eq!(
            fixture
                .received_hook
                .dispatches
                .lock()
                .expect("inspect deferred dispatch")
                .first()
                .expect("queue dispatch")
                .kind,
            NudgeKind::Queue,
            "the graft handoff is explicitly queue-kind"
        );

        let member = MemberKey::new(
            "test-team".parse().expect("team"),
            "recipient".parse().expect("agent"),
        );
        let first_claim = tokio::task::spawn_blocking({
            let store = fixture.pending_nudge_store.clone();
            let member = member.clone();
            move || store.claim_next_pending(&member)
        })
        .await
        .expect("claim task joins")
        .expect("claim succeeds")
        .expect("deferred marker is claimable");
        assert_eq!(first_claim.msg, message_id);

        let duplicate = post_write(router(&fixture, AuthenticatedConnector::local()), &write).await;
        assert_eq!(duplicate.status(), StatusCode::CREATED);
        let second_claim = tokio::task::spawn_blocking({
            let store = fixture.pending_nudge_store.clone();
            move || store.claim_next_pending(&member)
        })
        .await
        .expect("duplicate claim task joins")
        .expect("duplicate claim succeeds");
        assert!(
            second_claim.is_none(),
            "an idempotent duplicate must not re-mark a claimed deferred write"
        );
    }

    #[tokio::test]
    async fn async_router_retries_one_marker_failure_and_preserves_write() {
        let fixture = fixture_with_selector_and_template_and_pending(
            true,
            None,
            None,
            None,
            |received_hook| {
                Arc::new(FixedReceivedHookSelector {
                    emitter: received_hook,
                })
            },
            Some(1),
        );
        let message_id = AtmMessageId::new();
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone())
            .with_origin_metadata(message_id, atm_core::types::IsoTimestamp::now())
            .with_nudge_mode(NudgeMode::Deferred);

        let response = post_write(router(&fixture, AuthenticatedConnector::local()), &write).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            fixture
                .runtime_health
                .snapshot()
                .queue_marker_set_failures_total,
            1
        );
        assert_eq!(
            fixture
                .pending_nudge_store
                .claim_next_pending(&MemberKey::new(
                    "test-team".parse().expect("team"),
                    "recipient".parse().expect("agent"),
                ))
                .expect("claim after one retry")
                .expect("marker succeeds on retry")
                .msg,
            message_id
        );
    }

    #[tokio::test]
    async fn async_router_counts_two_marker_failures_and_preserves_documented_loss_bound() {
        let fixture = fixture_with_selector_and_template_and_pending(
            true,
            None,
            None,
            None,
            |received_hook| {
                Arc::new(FixedReceivedHookSelector {
                    emitter: received_hook,
                })
            },
            Some(2),
        );
        let message_id = AtmMessageId::new();
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone())
            .with_origin_metadata(message_id, atm_core::types::IsoTimestamp::now())
            .with_nudge_mode(NudgeMode::Deferred);

        let response = post_write(router(&fixture, AuthenticatedConnector::local()), &write).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            fixture
                .runtime_health
                .snapshot()
                .queue_marker_set_failures_total,
            2
        );
        assert!(
            fixture
                .pending_nudge_store
                .claim_next_pending(&MemberKey::new(
                    "test-team".parse().expect("team"),
                    "recipient".parse().expect("agent"),
                ))
                .expect("claim after exhausted retries")
                .is_none(),
            "the documented two-attempt marker loss bound leaves no marker"
        );
    }

    #[tokio::test]
    async fn templated_send_over_loopback_tcp_uses_decomposed_admission_once() {
        let body = "template body";
        let fixture = fixture_with_selector_and_template(
            true,
            None,
            None,
            Some(template_composer_for(body)),
            |received_hook| {
                Arc::new(FixedReceivedHookSelector {
                    emitter: received_hook,
                })
            },
        );
        let write = template_write_request(&fixture, body);
        let response = post_write(router(&fixture, AuthenticatedConnector::local()), &write).await;

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let response: serde_json::Value = serde_json::from_slice(&body).expect("response JSON");
        let message_id = response["message_id"].as_str().expect("message id");
        let snapshot = inspect_template_admission_for_test(
            &fixture.database_path,
            &[format!("atm:{message_id}")],
        )
        .expect("stored decomposition");
        assert_eq!(
            snapshot.template_count, 1,
            "one immutable template registration"
        );
        assert_eq!(snapshot.decomposed_count, 1, "one decomposed message row");
        let stored = snapshot.messages.first().expect("stored mail row");
        assert!(
            stored.template_sha.is_some(),
            "mail row records the template SHA"
        );
        assert_eq!(stored.vars_json.as_deref(), Some("{}"));
        assert_eq!(stored.tags_json, r#"["phase-an"]"#);
        assert_eq!(
            stored.message_text, None,
            "decomposed row never retains rendered plain body"
        );
    }

    #[tokio::test]
    async fn an15_shape_probe_missing_required_input_mutates_no_catalog_or_mailbox_rows() {
        let body = "required input must be captured before durable admission";
        let inspection = atm_core::TemplateInspection {
            sha: TemplateSha::new(
                "814271b7e98145c998a2c1f20270856c592881ba7dac4dfee9307d8093163a03",
            )
            .expect("template SHA"),
            frontmatter: TemplateFrontmatter {
                required_variables: vec![
                    atm_storage::TemplateVariableName::new("ATM_TEAM")
                        .expect("fixture required variable"),
                ],
                ..TemplateFrontmatter::default()
            },
            include_references: Vec::new(),
            output_format: atm_storage::TemplateOutputFormat::Text,
        };
        let composer: Arc<dyn atm_core::TemplateComposer> = Arc::new(
            FixtureTemplateComposer::with_inspection(body.as_bytes().to_vec(), inspection),
        );
        let fixture =
            fixture_with_selector_and_template(true, None, None, Some(composer), |received_hook| {
                Arc::new(FixedReceivedHookSelector {
                    emitter: received_hook,
                })
            });

        let write = template_write_request(&fixture, body);
        let response = post_write(router(&fixture, AuthenticatedConnector::local()), &write).await;
        let status = response.status();
        let error = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("rejection body");
        assert_ne!(
            status,
            StatusCode::CREATED,
            "missing required input must reject the write: {}",
            String::from_utf8_lossy(&error)
        );
        let error: serde_json::Value = serde_json::from_slice(&error).expect("rejection JSON");
        assert_eq!(
            error["code"].as_str(),
            Some("TEMPLATE_REQUIRED_VARIABLE_MISSING"),
            "the stable admission error explains the rejected write"
        );
        eprintln!(
            "AN15_DIAGNOSTIC missing-required-input code={} message={}",
            error["code"].as_str().expect("stable rejection code"),
            error["message"].as_str().expect("stable rejection message"),
        );

        let snapshot = inspect_template_admission_for_test(&fixture.database_path, &[])
            .expect("inspect rejected durable admission");
        assert_eq!(
            snapshot.template_count, 0,
            "rejected input registers no template"
        );
        assert_eq!(
            snapshot.decomposed_count, 0,
            "rejected input creates no decomposition"
        );
        let mailbox = fixture
            .message_store
            .list_messages(&MessageQuery {
                team: "test-team".parse().expect("team"),
                agent: "recipient".parse().expect("agent"),
                sender: None,
                task_id: None,
                limit: None,
            })
            .expect("inspect rejected mailbox");
        assert!(mailbox.is_empty(), "rejected input writes no mailbox row");
    }

    #[tokio::test]
    async fn template_routing_matrix_persists_only_same_team_same_host_as_decomposed() {
        let body = "routing matrix body";
        let fixture = fixture_with_selector_and_template(
            true,
            None,
            None,
            Some(template_composer_for(body)),
            |received_hook| {
                Arc::new(FixedReceivedHookSelector {
                    emitter: received_hook,
                })
            },
        );
        let same_local = template_write_request(&fixture, body);
        let mut same_team_cross_host = template_write_request(&fixture, body);
        same_team_cross_host.to = Some(
            "recipient@test-team.peer.example.test"
                .parse()
                .expect("cross-host recipient"),
        );
        let mut foreign_team_local = template_write_request(&fixture, body);
        foreign_team_local.caller_team = "foreign-team".parse().expect("foreign caller team");
        let mut foreign_team_cross_host = foreign_team_local.clone();
        foreign_team_cross_host.to = Some(
            "recipient@test-team.peer.example.test"
                .parse()
                .expect("foreign cross-host recipient"),
        );

        let mut message_keys = Vec::new();
        for (request, ingress) in [
            (same_local, AuthenticatedIngress::Local),
            // This matrix concerns persisted template routing, not outbound
            // delivery. Model cross-host rows as an already-admitted peer so
            // the daemon does not try to deliver to the fixture hostname.
            (same_team_cross_host, AuthenticatedIngress::Peer),
            (foreign_team_local, AuthenticatedIngress::Local),
            (foreign_team_cross_host, AuthenticatedIngress::Peer),
        ] {
            let response = fixture
                .router
                .dispatch(
                    ApiRequest::new(RequestEnvelope::Write(Box::new(request))),
                    ingress,
                    RequestDeadline::after(Duration::from_secs(1)),
                )
                .await
                .expect("template routing request");
            let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response.into_inner()
            else {
                panic!("template routing request must send")
            };
            message_keys.push(format!("atm:{}", outcome.message_id));
        }

        let snapshot = inspect_template_admission_for_test(&fixture.database_path, &message_keys)
            .expect("inspect routing rows");
        assert_eq!(
            snapshot.template_count, 1,
            "only the same-team local cell registers a template"
        );
        assert_eq!(
            snapshot.decomposed_count, 1,
            "only the same-team local cell decomposes a row"
        );
        assert_eq!(
            snapshot.messages.len(),
            4,
            "each routing cell admits exactly one mailbox row"
        );
        let fallback_rows = snapshot
            .messages
            .iter()
            .filter(|row| row.template_sha.is_none())
            .collect::<Vec<_>>();
        assert_eq!(
            fallback_rows.len(),
            3,
            "the three fallback cells stay ordinary rows"
        );
        assert_eq!(
            fallback_rows
                .iter()
                .filter(|row| row.vars_json.is_none() && row.message_text.as_deref() == Some(body))
                .count(),
            3,
            "every fallback persists the verification render without template metadata"
        );
        assert_eq!(
            snapshot
                .messages
                .iter()
                .filter(|row| row.template_sha.is_some())
                .count(),
            1,
            "none of the plain-text cells may create a catalog admission"
        );
    }

    #[tokio::test]
    async fn include_template_is_verified_but_persisted_as_an_ordinary_rendered_row() {
        let body = "include fallback body";
        let inspection = atm_core::TemplateInspection {
            sha: TemplateSha::new(
                "8eff5904e91d06678d2bd0bd3afd9aef81d4bb3b732a9348b9ba463e00781723",
            )
            .expect("template SHA"),
            frontmatter: TemplateFrontmatter::default(),
            include_references: vec![atm_core::TemplateReference {
                directive: atm_core::TemplateReferenceKind::Include,
                source_span: atm_core::SourceSpan {
                    byte_start: 0,
                    byte_end: body.len(),
                },
            }],
            output_format: atm_storage::TemplateOutputFormat::Text,
        };
        let composer: Arc<dyn atm_core::TemplateComposer> = Arc::new(
            FixtureTemplateComposer::with_inspection(body.as_bytes().to_vec(), inspection),
        );
        let fixture =
            fixture_with_selector_and_template(true, None, None, Some(composer), |received_hook| {
                Arc::new(FixedReceivedHookSelector {
                    emitter: received_hook,
                })
            });
        let response = fixture
            .router
            .dispatch(
                ApiRequest::new(RequestEnvelope::Write(Box::new(template_write_request(
                    &fixture, body,
                )))),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("include fallback send");
        let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response.into_inner()
        else {
            panic!("include fallback send must send")
        };
        let snapshot = inspect_template_admission_for_test(
            &fixture.database_path,
            &[format!("atm:{}", outcome.message_id)],
        )
        .expect("inspect include fallback row");
        assert_eq!(
            snapshot.template_count, 0,
            "include fallback never registers a catalog template"
        );
        assert_eq!(
            snapshot.decomposed_count, 0,
            "include fallback never creates decomposition"
        );
        let row = snapshot
            .messages
            .first()
            .expect("stored include fallback row");
        assert_eq!(row.template_sha, None);
        assert_eq!(row.vars_json, None);
        assert_eq!(row.message_text.as_deref(), Some(body));
    }

    #[test]
    fn canonical_write_path_does_not_reopen_committed_records_for_hook_planning() {
        // This is an architecture regression guard rather than a behavior
        // assertion. The async storage admission already has the recipient,
        // delivery snapshot, and logical messages needed to plan a hook.
        // Reconstructing them by synchronously reading the just-persisted
        // SQLite row turns every public write into a blocking read, including
        // hook-disabled writes.
        let production_source = include_str!("storage_and_nudge_router.rs")
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .expect("production source before test module");
        let legacy_planner = [
            "build_received_",
            "message_hook_",
            "dispatches_after_commit",
        ]
        .concat();
        let storage_reload = ["load_", "message_", "record"].concat();
        assert!(
            !production_source.contains(&legacy_planner),
            "the replacement write path must use PreparedWrite's retained hook plan"
        );
        assert!(
            !production_source.contains(&storage_reload),
            "the replacement write path must not synchronously reload a committed record"
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
        let uid = NonZeroU32::new(
            std::fs::metadata(fixture._temporary_root.path())
                .expect("runtime root metadata")
                .uid(),
        )
        .expect("test process must not use uid zero");
        let runtime = HttpRuntimeBuilder::new(
            HttpRuntimeConfig::new(
                LoopbackTcpConfig::new(
                    // Let Tokio retain the kernel-selected port; reserving a
                    // port synchronously and then dropping it would reopen a
                    // TOCTOU window before the runtime binds.
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
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

    #[cfg(unix)]
    #[tokio::test]
    async fn uds_and_loopback_search_use_the_same_local_storage_port() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};
        use std::num::NonZeroU32;
        use std::os::unix::fs::MetadataExt;

        let fixture = fixture(true, None, None);
        fixture
            .router
            .write(
                write_request(fixture.home_dir.clone(), fixture.current_dir.clone()),
                AuthenticatedIngress::Local,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("seed local mailbox through the canonical storage path");
        let socket_path = fixture._temporary_root.path().join("search-runtime.sock");
        let endpoint_path = fixture
            ._temporary_root
            .path()
            .join("search-local-http.json");
        let instance_id = ulid::Ulid::new();
        std::fs::write(
            fixture._temporary_root.path().join("owner.lock"),
            format!("1:test-owner:{instance_id}\n"),
        )
        .expect("owner record");
        let uid = NonZeroU32::new(
            std::fs::metadata(fixture._temporary_root.path())
                .expect("runtime root metadata")
                .uid(),
        )
        .expect("test process must not use uid zero");
        let runtime = HttpRuntimeBuilder::new(
            HttpRuntimeConfig::new(
                LoopbackTcpConfig::new(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                    endpoint_path.clone(),
                    instance_id,
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
        .expect("valid search runtime configuration");
        let running = runtime.start().await.expect("search runtime starts");
        let request = ApiRequest::new(RequestEnvelope::Search(Box::new(
            atm_core::search::SearchRequest {
                query: atm_core::search::SearchInput::default(),
                lifecycle: None,
            },
        )));
        let uds = crate::unix_socket_client(&socket_path, Duration::from_secs(1))
            .expect("UDS client")
            .execute(request.clone())
            .await
            .expect("UDS search");
        let loopback = crate::loopback_tcp_client(&endpoint_path, Duration::from_secs(1))
            .expect("loopback client")
            .execute(request)
            .await
            .expect("loopback search");
        let ResponseEnvelope::Search(uds) = uds.into_inner() else {
            panic!("UDS response must be search data");
        };
        let ResponseEnvelope::Search(loopback) = loopback.into_inner() else {
            panic!("loopback response must be search data");
        };
        assert_eq!(
            uds.hits.len(),
            1,
            "UDS query returns the seeded mailbox row"
        );
        assert_eq!(
            loopback, uds,
            "UDS and loopback share one local search path"
        );
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("search runtime drains");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn templated_send_over_uds_uses_the_same_decomposed_admission() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};
        use std::num::NonZeroU32;
        use std::os::unix::fs::MetadataExt;

        let body = "template body over UDS";
        let fixture = fixture_with_selector_and_template(
            true,
            None,
            None,
            Some(template_composer_for(body)),
            |received_hook| {
                Arc::new(FixedReceivedHookSelector {
                    emitter: received_hook,
                })
            },
        );
        let socket_path = fixture._temporary_root.path().join("template-runtime.sock");
        let uid = NonZeroU32::new(
            std::fs::metadata(fixture._temporary_root.path())
                .expect("runtime root metadata")
                .uid(),
        )
        .expect("test process must not use uid zero");
        let runtime = HttpRuntimeBuilder::new(
            HttpRuntimeConfig::new(
                LoopbackTcpConfig::new(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                    fixture
                        ._temporary_root
                        .path()
                        .join("template-local-http.json"),
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
        let response = crate::unix_socket_client(&socket_path, Duration::from_secs(1))
            .expect("UDS client")
            .execute(ApiRequest::new(RequestEnvelope::Write(Box::new(
                template_write_request(&fixture, body),
            ))))
            .await
            .expect("canonical UDS response");
        let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response.into_inner()
        else {
            panic!("canonical UDS template write must send")
        };
        let snapshot = inspect_template_admission_for_test(
            &fixture.database_path,
            &[format!("atm:{}", outcome.message_id)],
        )
        .expect("stored template rows");
        assert_eq!(
            (snapshot.template_count, snapshot.decomposed_count),
            (1, 1),
            "UDS reaches the same atomic template admission"
        );
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("UDS runtime drains");
    }

    #[tokio::test]
    async fn direct_peer_runtime_reaches_storage_once_and_skips_duplicate_hook() {
        let fixture = fixture(true, None, None);
        let running = HttpRuntimeBuilder::new(
            direct_peer_runtime_config(&fixture, crate::DirectPeerTcpConfig::ephemeral_for_test()),
            Arc::new(fixture.router.clone()),
        )
        .build()
        .expect("valid direct peer runtime configuration")
        .start()
        .await
        .expect("direct peer runtime starts");
        let peer_port = running
            .direct_peer_address()
            .expect("ephemeral direct peer listener is bound")
            .port();
        let message_id = AtmMessageId::new();
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone())
            .with_origin_metadata(message_id, atm_core::types::IsoTimestamp::now());

        for peer_host in ["localhost", "127.0.0.1"] {
            let client = direct_peer_tcp_client(
                peer_host.parse().expect("direct peer host"),
                std::num::NonZeroU16::new(peer_port).expect("non-zero peer port"),
                Duration::from_secs(5),
            )
            .expect("direct peer client");
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
                "direct peer host {peer_host} receives the canonical send response"
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
        {
            let dispatches = fixture
                .received_hook
                .dispatches
                .lock()
                .expect("inspect direct peer nudge dispatches");
            assert_eq!(dispatches.len(), 1, "one durable write emits one nudge");
            assert_eq!(
                dispatches[0]
                    .event
                    .sender_host
                    .as_ref()
                    .map(|host| host.as_str()),
                Some("127.0.0.1"),
                "direct ingress provenance comes from the accepted peer socket"
            );
            assert_eq!(
                dispatches[0].event.source_address().to_string(),
                "sender@test-team.127.0.0.1",
                "the nudge source preserves the authenticated socket host"
            );
            assert!(
                matches!(&dispatches[0].target, PostSendBuiltInTarget::Graft(_)),
                "the roster harness routes the received nudge through graft"
            );
        }
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("direct peer runtime drains");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn locally_admitted_host_qualified_write_is_forwarded_by_the_selected_daemon() {
        let remote = fixture(true, None, None);
        let remote_runtime = HttpRuntimeBuilder::new(
            direct_peer_runtime_config(&remote, crate::DirectPeerTcpConfig::ephemeral_for_test()),
            Arc::new(remote.router.clone()),
        )
        .build()
        .expect("valid remote direct peer configuration")
        .start()
        .await
        .expect("remote direct peer runtime starts");
        let remote_port = remote_runtime
            .direct_peer_address()
            .expect("ephemeral remote direct peer listener is bound")
            .port();

        let mut local = fixture(true, None, None);
        local.router = local
            .router
            .clone()
            .with_direct_peer_port(NonZeroU16::new(remote_port).expect("non-zero port"))
            .with_shared_direct_peer_client(
                shared_direct_peer_client().expect("shared direct peer client"),
            );
        let mut outbound = write_request(local.home_dir.clone(), local.current_dir.clone());
        outbound.to = Some(
            "recipient@test-team.127.0.0.1"
                .parse()
                .expect("host-qualified remote recipient"),
        );
        let response = local
            .router
            .dispatch(
                ApiRequest::new(RequestEnvelope::Write(Box::new(outbound))),
                AuthenticatedIngress::Local,
                RequestDeadline::after(TWO_RUNTIME_TEST_REQUEST_BUDGET),
            )
            .await
            .expect("local daemon owns host-qualified delivery");
        assert!(matches!(
            response.into_inner(),
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
        ));
        assert_eq!(
            remote
                .message_store
                .list_messages(&MessageQuery {
                    team: "test-team".parse().expect("team"),
                    agent: "recipient".parse().expect("agent"),
                    sender: None,
                    task_id: None,
                    limit: None,
                })
                .expect("read remote recipient mailbox")
                .len(),
            1,
            "the peer listener receives exactly the locally admitted canonical write"
        );
        remote_runtime
            .begin_shutdown()
            .finish()
            .await
            .expect("remote runtime drains");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn selected_router_reuses_one_opaque_peer_connection_across_three_loopback_writes() {
        let adapter = Arc::new(CountingPassthroughPeerAdapter::default());
        let remote = fixture(true, None, None);
        let remote_runtime = HttpRuntimeBuilder::new(
            direct_peer_runtime_config(&remote, crate::DirectPeerTcpConfig::ephemeral_for_test()),
            Arc::new(remote.router.clone()),
        )
        .build()
        .expect("valid authenticated remote direct peer configuration")
        .start()
        .await
        .expect("authenticated remote direct peer runtime starts");
        let remote_port = remote_runtime
            .direct_peer_address()
            .expect("ephemeral remote direct peer listener is bound")
            .port();

        let pool = PeerConnectionPool::new(PeerPoolConfig::default(), adapter.clone());
        let mut local = fixture(true, None, None);
        local.router = local
            .router
            .clone()
            .with_direct_peer_port(NonZeroU16::new(remote_port).expect("non-zero port"))
            .with_peer_connection_pool(pool.clone());

        for _ in 0..3 {
            let mut outbound = write_request(local.home_dir.clone(), local.current_dir.clone());
            outbound.to = Some(
                "recipient@test-team.127.0.0.1"
                    .parse()
                    .expect("host-qualified loopback recipient"),
            );
            let response = local
                .router
                .dispatch(
                    ApiRequest::new(RequestEnvelope::Write(Box::new(outbound))),
                    AuthenticatedIngress::Local,
                    RequestDeadline::after(TWO_RUNTIME_TEST_REQUEST_BUDGET),
                )
                .await
                .expect("selected router forwards the local write");
            assert!(matches!(
                response.into_inner(),
                ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
            ));
        }

        assert_eq!(
            adapter.outbound_connects.load(Ordering::SeqCst),
            1,
            "three sequential router dispatches establish one opaque peer stream"
        );
        assert_eq!(
            remote
                .message_store
                .list_messages(&MessageQuery {
                    team: "test-team".parse().expect("team"),
                    agent: "recipient".parse().expect("agent"),
                    sender: None,
                    task_id: None,
                    limit: None,
                })
                .expect("read remote recipient mailbox")
                .len(),
            3,
            "every dispatched write crosses the canonical remote peer listener"
        );

        pool.shutdown(Duration::from_secs(1)).await;
        remote_runtime
            .begin_shutdown()
            .finish()
            .await
            .expect("remote runtime drains");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_ack_routes_to_the_received_peer_and_peer_receipt_does_not_reacknowledge() {
        let remote = fixture(true, None, None);
        let remote_runtime = HttpRuntimeBuilder::new(
            direct_peer_runtime_config(&remote, crate::DirectPeerTcpConfig::ephemeral_for_test()),
            Arc::new(remote.router.clone()),
        )
        .build()
        .expect("valid remote direct peer configuration")
        .start()
        .await
        .expect("remote direct peer runtime starts");
        let remote_port = remote_runtime
            .direct_peer_address()
            .expect("ephemeral remote direct peer listener is bound")
            .port();

        let mut local = fixture(true, None, None);
        local.router = local
            .router
            .clone()
            .with_direct_peer_port(std::num::NonZeroU16::new(remote_port).expect("non-zero port"))
            .with_shared_direct_peer_client(
                shared_direct_peer_client().expect("shared direct peer client"),
            );
        let local_runtime = HttpRuntimeBuilder::new(
            direct_peer_runtime_config(&local, crate::DirectPeerTcpConfig::ephemeral_for_test()),
            Arc::new(local.router.clone()),
        )
        .build()
        .expect("valid local direct peer configuration")
        .start()
        .await
        .expect("local direct peer runtime starts");
        let local_port = local_runtime
            .direct_peer_address()
            .expect("ephemeral local direct peer listener is bound")
            .port();

        let received_id = AtmMessageId::new();
        let mut incoming = write_request(remote.home_dir.clone(), remote.current_dir.clone())
            .with_origin_metadata(received_id, atm_core::types::IsoTimestamp::now());
        incoming.requires_ack = true;
        let local_peer_client = direct_peer_tcp_client(
            "127.0.0.1".parse().expect("loopback source host"),
            std::num::NonZeroU16::new(local_port).expect("non-zero port"),
            Duration::from_secs(5),
        )
        .expect("local direct peer client");
        assert!(matches!(
            local_peer_client
                .execute(ApiRequest::new(RequestEnvelope::Write(Box::new(incoming))))
                .await
                .expect("incoming required message reaches local daemon")
                .into_inner(),
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
        ));
        {
            let local_dispatches = local
                .received_hook
                .dispatches
                .lock()
                .expect("inspect two-runtime nudge dispatches");
            assert_eq!(local_dispatches.len(), 1, "direct ingress emits one nudge");
            assert_eq!(
                local_dispatches[0]
                    .event
                    .sender_host
                    .as_ref()
                    .map(|host| host.as_str()),
                Some("127.0.0.1"),
                "the receiver records the accepted socket as the sender host"
            );
            assert_eq!(
                local_dispatches[0].event.source_address().to_string(),
                "sender@test-team.127.0.0.1",
                "the cross-runtime nudge retains direct-peer provenance"
            );
            assert!(matches!(
                &local_dispatches[0].target,
                PostSendBuiltInTarget::Graft(_)
            ));
        }

        let acknowledgement = atm_core::ack::AckRequest {
            home_dir: local.home_dir.clone(),
            current_dir: local.current_dir.clone(),
            caller_identity: "recipient".parse().expect("recipient"),
            caller_chat_id: None,
            caller_team: "test-team".parse().expect("team"),
            activity_observation: None,
            message_id: received_id,
            reply_body: "received".to_owned(),
        }
        .into_write_request();
        let response = local
            .router
            .dispatch(
                ApiRequest::new(RequestEnvelope::Write(Box::new(acknowledgement))),
                atm_core::AuthenticatedIngress::Local,
                RequestDeadline::after(TWO_RUNTIME_TEST_REQUEST_BUDGET),
            )
            .await
            .expect("local acknowledgement is delivered to the stored peer host");
        let ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) =
            response.into_inner()
        else {
            panic!("local ACK must retain its acknowledgement response")
        };
        let reply_id = match outcome.reply_disposition {
            atm_core::ack::AckReplyDisposition::Sent {
                reply_message_id, ..
            } => reply_message_id,
        };
        assert!(
            local
                .message_store
                .load_message(&MessageKey::from(received_id))
                .expect("read acknowledged local source")
                .expect("received source remains durable")
                .envelope
                .acknowledged_at
                .is_some(),
            "the local source is transitioned only as part of the acknowledged reply"
        );

        let reply = remote
            .message_store
            .load_message(&MessageKey::from(reply_id))
            .expect("read peer ACK receipt")
            .expect("ACK reply persists at original sender");
        assert_eq!(reply.team.as_str(), "test-team");
        assert_eq!(reply.agent.as_str(), "sender");
        assert_eq!(reply.envelope.from.as_str(), "recipient");
        assert_eq!(reply.envelope.acknowledges_message_id, Some(received_id));
        assert!(
            reply.envelope.pending_ack_at.is_none(),
            "an ACK receipt is not an acknowledgement source and never starts an ACK loop"
        );
        assert_eq!(
            remote
                .message_store
                .list_messages(&MessageQuery {
                    team: "test-team".parse().expect("team"),
                    agent: "sender".parse().expect("sender"),
                    sender: Some("recipient".parse().expect("recipient")),
                    task_id: None,
                    limit: None,
                })
                .expect("read remote messages")
                .iter()
                .filter(|message| message.envelope.acknowledges_message_id == Some(received_id))
                .count(),
            1,
            "the canonical ACK reply crosses the shared direct-peer path exactly once"
        );

        local_runtime
            .begin_shutdown()
            .finish()
            .await
            .expect("local direct peer runtime drains");
        remote_runtime
            .begin_shutdown()
            .finish()
            .await
            .expect("remote direct peer runtime drains");
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
        let running = HttpRuntimeBuilder::new(
            direct_peer_runtime_config(&fixture, crate::DirectPeerTcpConfig::ephemeral_for_test()),
            Arc::new(fixture.router.clone()),
        )
        .build()
        .expect("valid direct peer runtime configuration")
        .start()
        .await
        .expect("direct peer runtime starts");
        let peer_port = running
            .direct_peer_address()
            .expect("ephemeral direct peer listener is bound")
            .port();
        let client = direct_peer_tcp_client(
            "localhost".parse().expect("direct peer host"),
            std::num::NonZeroU16::new(peer_port).expect("non-zero peer port"),
            Duration::from_secs(5),
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
    async fn axum_route_storage_rejection_emits_no_hook_and_persists_no_message() {
        let fixture = fixture(true, None, None);
        install_sqlite_message_write_failure(&fixture.database_path)
            .expect("install deterministic SQLite storage failure");
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone());
        let response = post_write(router(&fixture, AuthenticatedConnector::local()), &write).await;
        // The deterministic SQLite trigger is a constraint rejection, which
        // the shared ADR-032 mapper exposes as a client error.  The contract
        // under test here is that any failed durable admission emits no hook
        // and leaves no mailbox record; availability mapping is covered by
        // the dedicated lock-timeout tests in the SQLite backend.
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(
            fixture
                .received_hook
                .emitted_ids
                .lock()
                .expect("inspect received-hook emissions")
                .is_empty(),
            "storage failure must not emit a receiver hook"
        );
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
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone())
            .with_origin_metadata(message_id, atm_core::types::IsoTimestamp::now());
        // This test isolates idempotency. An unqualified mailbox target
        // avoids exercising AO2.3's separate daemon-owned remote delivery.

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
    async fn axum_route_hook_deadline_keeps_the_durable_write_advisory() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let fixture = fixture(true, None, Some(cancelled));
        let write = write_request(fixture.home_dir.clone(), fixture.current_dir.clone());
        let response = post_write(
            router_with_timeout(
                &fixture,
                AuthenticatedConnector::local(),
                Duration::from_secs(1),
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

    #[tokio::test]
    async fn received_hook_timeout_is_advisory_and_cancels_the_pending_future() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let fixture = fixture(true, None, Some(Arc::clone(&cancelled)));
        let dispatch = BuiltInPostSendDispatch {
            event: hook_event(),
            target: PostSendBuiltInTarget::Graft(GraftNudgeTarget {
                recipient: "recipient".parse().expect("recipient"),
                recipient_team: "test-team".parse().expect("team"),
                rendered_nudge: "<atm kind=\"nudge\"/>".to_owned(),
                message_body: "message body".to_owned(),
            }),
            kind: NudgeKind::Steer,
        };

        let warnings = fixture
            .router
            .emit_received_hook(
                Ok(vec![dispatch]),
                RequestDeadline::after(Duration::from_millis(50)),
            )
            .await;

        assert_eq!(warnings.len(), 1, "one timed-out hook yields one warning");
        assert_eq!(
            warnings[0].code,
            Some(atm_core::error_codes::AtmErrorCode::DaemonUnavailable)
        );
        assert!(
            warnings[0].message.contains("timed out"),
            "the hook timeout retains the stable advisory warning text"
        );
        assert!(
            cancelled.load(Ordering::SeqCst),
            "deadline cancellation drops the hook future instead of leaving detached work"
        );
        assert_eq!(
            fixture
                .received_hook
                .emitted_ids
                .lock()
                .expect("inspect timed-out received-hook emission")
                .len(),
            1,
            "the hook begins before its dedicated timeout expires"
        );
    }
}
