use std::fmt;

use crate::error::{AtmError, AtmErrorCode};

/// Explicit resource-management outcomes from a bounded reader lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadLaneError {
    UnauthorizedScope,
    Saturated {
        reason: &'static str,
    },
    DeadlineExpired {
        stage: &'static str,
        budget_ms: u64,
        elapsed_ms: u64,
    },
    Unavailable {
        message: String,
    },
    Storage {
        code: AtmErrorCode,
        message: String,
        cause: Option<String>,
    },
}

impl fmt::Display for ReadLaneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnauthorizedScope => {
                formatter.write_str("mailbox scope does not authorize this read")
            }
            Self::Saturated { reason } => {
                write!(formatter, "mailbox reader lane is saturated: {reason}")
            }
            Self::DeadlineExpired {
                stage,
                budget_ms,
                elapsed_ms,
            } => {
                write!(
                    formatter,
                    "mailbox reader deadline expired while {stage} (budget_ms={budget_ms} elapsed_ms={elapsed_ms})"
                )
            }
            Self::Unavailable { message } => {
                write!(formatter, "mailbox reader lane is unavailable: {message}")
            }
            Self::Storage { message, cause, .. } => {
                write!(formatter, "mailbox reader storage failure: {message}")?;
                if let Some(cause) = cause {
                    write!(formatter, "; cause: {cause}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ReadLaneError {}

/// Translates the storage-owned reader-lane outcomes exactly once into the
/// stable ATM error vocabulary. The original lane outcome remains attached as
/// the machine-visible cause instead of being flattened into unavailability.
impl From<ReadLaneError> for AtmError {
    fn from(error: ReadLaneError) -> Self {
        let code = match &error {
            ReadLaneError::UnauthorizedScope => AtmErrorCode::MailboxReadFailed,
            ReadLaneError::Saturated { .. } => AtmErrorCode::DaemonConnectionSaturated,
            ReadLaneError::DeadlineExpired { .. } => AtmErrorCode::MailboxLockTimeout,
            ReadLaneError::Unavailable { .. } => AtmErrorCode::DaemonUnavailable,
            ReadLaneError::Storage { code, .. } => *code,
        };
        let detail = match &error {
            ReadLaneError::UnauthorizedScope => {
                "bounded mailbox reader request failed: variant=unauthorized_scope stage=scope_check budget=not_started".to_owned()
            }
            ReadLaneError::Saturated { reason } => format!(
                "bounded mailbox reader request failed: variant=saturated stage={reason} budget=not_started"
            ),
            ReadLaneError::DeadlineExpired {
                stage,
                budget_ms,
                elapsed_ms,
            } => format!(
                "bounded mailbox reader request failed: variant=deadline_expired stage={stage} budget_ms={budget_ms} elapsed_ms={elapsed_ms}"
            ),
            ReadLaneError::Unavailable { .. } => {
                "bounded mailbox reader request failed: variant=unavailable stage=reader_lane budget=unavailable".to_owned()
            }
            ReadLaneError::Storage { code, .. } => format!(
                "bounded mailbox reader request failed: variant=storage stage=reader_lane error_code={code}"
            ),
        };
        AtmError::new(code, detail).with_cause(error)
    }
}
