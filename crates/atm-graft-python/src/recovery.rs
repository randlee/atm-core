//! Daemon reconnect policy and fallback diagnostics for native tool calls.

use std::time::Instant;

use pyo3::prelude::*;

use super::{AtmToolError, DaemonRecovery, DaemonRecoveryPolicy, PyGraftSession};

struct RecoveryContext {
    strategy: &'static str,
    endpoint_kind: String,
    correlation_id: String,
    started: Instant,
}

impl DaemonRecoveryPolicy {
    pub(super) fn strategy(self) -> &'static str {
        match self {
            Self::RefreshOnly => "refresh_only",
            Self::RetryOnce => "retry_once",
        }
    }
}

impl PyGraftSession {
    pub(super) fn with_daemon_recovery<T: Send>(
        &self,
        py: Python<'_>,
        policy: DaemonRecoveryPolicy,
        action: &'static str,
        mut operation: impl FnMut() -> PyResult<T> + Send,
    ) -> DaemonRecovery<T> {
        let error = match py.detach(&mut operation) {
            Ok(value) => return DaemonRecovery::Completed(value, Default::default()),
            Err(error) => error,
        };
        if !AtmToolError::from_native_error(py, &error).is_daemon_unavailable() {
            return DaemonRecovery::Failed {
                error,
                refreshed: false,
                refresh_error: None,
                observability: Default::default(),
            };
        }
        self.recover_daemon_unavailable(py, policy, action, operation, error)
    }

    fn recover_daemon_unavailable<T: Send>(
        &self,
        py: Python<'_>,
        policy: DaemonRecoveryPolicy,
        action: &'static str,
        mut operation: impl FnMut() -> PyResult<T> + Send,
        error: PyErr,
    ) -> DaemonRecovery<T> {
        let context = RecoveryContext {
            strategy: policy.strategy(),
            endpoint_kind: self.recovery_endpoint_kind(),
            correlation_id: format!("graft-{}", super::observability::correlation_id()),
            started: Instant::now(),
        };
        let mut observability = self.recovery_attempt(action, &context);
        match py.detach(|| self.reconnect_client()) {
            Err(refresh_error) => {
                let refresh_error_code = AtmToolError::from_native_error(py, &refresh_error).code;
                observability.merge(self.recovery_outcome(
                    action,
                    &context,
                    "endpoint_unavailable",
                    "failed",
                    Some(refresh_error_code),
                ));
                DaemonRecovery::Failed {
                    error,
                    refreshed: false,
                    refresh_error: Some(refresh_error),
                    observability,
                }
            }
            Ok(()) if matches!(policy, DaemonRecoveryPolicy::RefreshOnly) => {
                observability.merge(self.recovery_outcome(
                    action,
                    &context,
                    "stale_client",
                    "recovered",
                    None,
                ));
                DaemonRecovery::Failed {
                    error,
                    refreshed: true,
                    refresh_error: None,
                    observability,
                }
            }
            Ok(()) => match py.detach(&mut operation) {
                Ok(value) => {
                    observability.merge(self.recovery_outcome(
                        action,
                        &context,
                        "stale_client",
                        "recovered",
                        None,
                    ));
                    DaemonRecovery::Completed(value, observability)
                }
                Err(error) => DaemonRecovery::Failed {
                    observability: {
                        observability.merge(self.recovery_outcome(
                            action,
                            &context,
                            "stale_client",
                            "failed",
                            None,
                        ));
                        observability
                    },
                    error,
                    refreshed: true,
                    refresh_error: None,
                },
            },
        }
    }

    fn recovery_endpoint_kind(&self) -> String {
        self.client()
            .ok()
            .and_then(|client| client.local_transport_label().ok())
            .unwrap_or("tcp_loopback")
            .to_owned()
    }

    fn recovery_attempt(
        &self,
        action: &'static str,
        context: &RecoveryContext,
    ) -> super::observability::ObservabilityStatus {
        self.emit_graft_event(
            "ATM_GRAFT_RECOVERY_ATTEMPT",
            [
                ("action", action.to_owned()),
                ("attempt", "1".to_owned()),
                ("strategy", context.strategy.to_owned()),
                ("correlation_id", context.correlation_id.clone()),
            ],
        )
    }

    fn recovery_outcome(
        &self,
        action: &'static str,
        context: &RecoveryContext,
        failure_class: &'static str,
        outcome: &'static str,
        refresh_error_code: Option<String>,
    ) -> super::observability::ObservabilityStatus {
        let mut unavailable = vec![
            ("action", action.to_owned()),
            ("endpoint_kind", context.endpoint_kind.clone()),
            ("failure_class", failure_class.to_owned()),
            ("strategy", context.strategy.to_owned()),
            ("correlation_id", context.correlation_id.clone()),
        ];
        if let Some(code) = refresh_error_code {
            unavailable.push(("refresh_error_code", code));
        }
        let mut status = self.emit_graft_event("ATM_GRAFT_DAEMON_UNAVAILABLE", unavailable);
        status.merge(self.emit_graft_event(
            "ATM_GRAFT_RECOVERY_RESULT",
            [
                ("action", action.to_owned()),
                ("outcome", outcome.to_owned()),
                (
                    "elapsed_ms",
                    context.started.elapsed().as_millis().to_string(),
                ),
                ("strategy", context.strategy.to_owned()),
                ("correlation_id", context.correlation_id.clone()),
            ],
        ));
        status
    }
}
