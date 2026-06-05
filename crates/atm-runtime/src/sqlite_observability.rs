use std::sync::Arc;

use atm_core::error::{AtmError, AtmErrorCode};
use atm_rusqlite::{SqliteObservability, SqliteObservabilityEvent, SqliteObservabilityOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeSqliteOutcome {
    Ok,
    Failed,
    Timeout,
}

impl RuntimeSqliteOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Timeout => "timeout",
        }
    }
}

impl From<SqliteObservabilityOutcome> for RuntimeSqliteOutcome {
    fn from(value: SqliteObservabilityOutcome) -> Self {
        match value {
            SqliteObservabilityOutcome::Ok => Self::Ok,
            SqliteObservabilityOutcome::Failed => Self::Failed,
            SqliteObservabilityOutcome::Timeout => Self::Timeout,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeSqliteEvent {
    pub action: &'static str,
    pub outcome: RuntimeSqliteOutcome,
    pub message: String,
    pub error_code: Option<AtmErrorCode>,
}

impl RuntimeSqliteEvent {
    pub fn new(
        action: &'static str,
        outcome: RuntimeSqliteOutcome,
        message: impl Into<String>,
        error_code: Option<AtmErrorCode>,
    ) -> Self {
        Self {
            action,
            outcome,
            message: message.into(),
            error_code,
        }
    }
}

pub trait RuntimeSqliteObserver: Send + Sync {
    fn emit_sqlite_event(&self, event: RuntimeSqliteEvent) -> Result<(), AtmError>;
}

#[derive(Clone)]
pub struct RuntimeSqliteObservability {
    observer: Arc<dyn RuntimeSqliteObserver>,
}

impl std::fmt::Debug for RuntimeSqliteObservability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeSqliteObservability")
            .field("observer", &"dyn RuntimeSqliteObserver")
            .finish()
    }
}

impl RuntimeSqliteObservability {
    pub fn new(observer: Arc<dyn RuntimeSqliteObserver>) -> Self {
        Self { observer }
    }
}

impl SqliteObservability for RuntimeSqliteObservability {
    fn emit(&self, event: SqliteObservabilityEvent) -> Result<(), AtmError> {
        self.observer.emit_sqlite_event(RuntimeSqliteEvent::new(
            event.action,
            event.outcome.into(),
            event.message,
            event.error_code,
        ))
    }
}
