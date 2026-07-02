use std::sync::Arc;

use atm_core::error::{AtmError, AtmErrorCode};
use atm_storage_rusqlite::{
    NullSqliteObservability, SqliteObservability, SqliteObservabilityEvent,
    SqliteObservabilityOutcome,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeSqliteOutcome {
    Failed,
    Timeout,
}

impl RuntimeSqliteOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Timeout => "timeout",
        }
    }
}

impl From<SqliteObservabilityOutcome> for RuntimeSqliteOutcome {
    fn from(value: SqliteObservabilityOutcome) -> Self {
        match value {
            SqliteObservabilityOutcome::Failed => Self::Failed,
            SqliteObservabilityOutcome::Timeout => Self::Timeout,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeSqliteEvent {
    pub(crate) action: &'static str,
    pub(crate) outcome: RuntimeSqliteOutcome,
    pub(crate) message: String,
    pub(crate) error_code: Option<AtmErrorCode>,
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

    pub fn action(&self) -> &'static str {
        self.action
    }

    pub fn outcome(&self) -> RuntimeSqliteOutcome {
        self.outcome
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn error_code(&self) -> Option<AtmErrorCode> {
        self.error_code
    }
}

pub trait RuntimeSqliteObserver: Send + Sync {
    fn emit_sqlite_event(&self, event: RuntimeSqliteEvent) -> Result<(), AtmError>;
}

#[derive(Clone)]
pub(crate) struct RuntimeSqliteObservability {
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
    pub(crate) fn new(observer: Arc<dyn RuntimeSqliteObserver>) -> Self {
        Self { observer }
    }

    pub(crate) fn disabled() -> Arc<dyn SqliteObservability> {
        Arc::new(NullSqliteObservability)
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
