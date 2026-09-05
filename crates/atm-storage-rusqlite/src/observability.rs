use atm_storage::{AtmError, AtmErrorCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqliteObservabilityOutcome {
    Failed,
    Timeout,
}

impl SqliteObservabilityOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Timeout => "timeout",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SqliteObservabilityEvent {
    pub action: &'static str,
    pub outcome: SqliteObservabilityOutcome,
    pub message: String,
    pub error_code: Option<AtmErrorCode>,
}

impl SqliteObservabilityEvent {
    pub fn new(
        action: &'static str,
        outcome: SqliteObservabilityOutcome,
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

pub trait SqliteObservability: Send + Sync {
    fn emit(&self, event: SqliteObservabilityEvent) -> Result<(), AtmError>;

    fn emit_or_warn(&self, event: SqliteObservabilityEvent) {
        if let Err(error) = self.emit(event.clone()) {
            tracing::warn!(
                %error,
                action = event.action,
                outcome = event.outcome.as_str(),
                event_message = %event.message,
                error_code = ?event.error_code,
                "sqlite subsystem observability emission failed"
            );
        }
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
pub struct NullSqliteObservability;

#[cfg(test)]
impl SqliteObservability for NullSqliteObservability {
    fn emit(&self, _event: SqliteObservabilityEvent) -> Result<(), AtmError> {
        Ok(())
    }
}

/// Deliberately passive adapter for library-only construction paths. The
/// daemon composition root injects its retained tracing adapter instead; this
/// distinct name makes a missing production injection mechanically visible.
#[derive(Debug, Default)]
pub(crate) struct PassiveSqliteObservability;

impl SqliteObservability for PassiveSqliteObservability {
    fn emit(&self, _event: SqliteObservabilityEvent) -> Result<(), AtmError> {
        Ok(())
    }
}
