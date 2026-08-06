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
