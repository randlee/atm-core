use atm_core::error::{AtmError, AtmErrorCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqliteObservabilityOutcome {
    Ok,
    Failed,
    Timeout,
}

impl SqliteObservabilityOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Timeout => "timeout",
        }
    }
}

/// Structured SQLite subsystem event emitted through the observability port.
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

/// Bottom-of-stack SQLite observability port.
///
/// This trait may capture SQLite subsystem events, but it must not depend on
/// daemon subsystem types or reconstruct daemon-specific semantics. Callers
/// provide already-shaped SQLite event content and the implementation decides
/// only how to emit or project that event. This trait is intentionally open
/// for extension by downstream observability backends.
pub trait SqliteObservability: Send + Sync {
    fn emit(&self, event: SqliteObservabilityEvent) -> Result<(), AtmError>;

    fn emit_or_warn(&self, event: SqliteObservabilityEvent) {
        if let Err(error) = self.emit(event.clone()) {
            tracing::warn!(
                %error,
                action = event.action,
                outcome = event.outcome.as_str(),
                error_code = ?event.error_code,
                "sqlite subsystem observability emission failed"
            );
        }
    }
}

/// No-op SQLite observability sink used by callers that intentionally do not
/// retain or project SQLite subsystem events.
#[derive(Debug, Default)]
pub struct NullSqliteObservability;

impl SqliteObservability for NullSqliteObservability {
    /// Intentionally succeeds without side effects so callers can reuse the
    /// same SQLite emission sites when observability is disabled.
    fn emit(&self, _event: SqliteObservabilityEvent) -> Result<(), AtmError> {
        Ok(())
    }
}
