//! Durable cron transition and delivery intent contract.
//!
//! Implementations commit state plus intents atomically.  The contract creates
//! exactly one logical intent per transition; adapters must retain unknown
//! external handoffs for reconciliation rather than asserting external
//! exactly-once delivery.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::contract::sealed;
use crate::cron_schedule_store::{CronGeneration, CronScheduleId};
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::hook_types::CronName;
use crate::schema::AtmMessageId;
use crate::types::{AgentName, IsoTimestamp, TeamName};

pub const CRON_OUTPUT_BODY_MAX_BYTES: usize = 65_536;
pub const CRON_DIAGNOSTIC_MAX_BYTES: usize = 4_096;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CronOutputBody(String);

impl CronOutputBody {
    pub fn parse(value: String) -> Result<Self, AtmError> {
        if value.is_empty() || value.len() > CRON_OUTPUT_BODY_MAX_BYTES {
            return Err(AtmError::hook_malformed(&format!(
                "cron output body must contain 1..={CRON_OUTPUT_BODY_MAX_BYTES} bytes"
            )));
        }
        Ok(Self(value))
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for CronOutputBody {
    type Error = AtmError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}
impl From<CronOutputBody> for String {
    fn from(value: CronOutputBody) -> Self {
        value.0
    }
}
impl fmt::Display for CronOutputBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Bounded, normalized diagnostic text.  The runner/redactor removes secrets
/// and volatile details before constructing this value; normal module output
/// uses [`CronOutputBody`] instead.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CronDiagnosticDetail(String);

impl CronDiagnosticDetail {
    pub fn parse(value: String) -> Result<Self, AtmError> {
        let value = value.replace(['\r', '\n'], " ");
        if value.trim().is_empty() || value.len() > CRON_DIAGNOSTIC_MAX_BYTES {
            return Err(AtmError::hook_malformed(&format!(
                "cron diagnostic must contain 1..={CRON_DIAGNOSTIC_MAX_BYTES} bytes"
            )));
        }
        Ok(Self(value))
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for CronDiagnosticDetail {
    type Error = AtmError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}
impl From<CronDiagnosticDetail> for String {
    fn from(value: CronDiagnosticDetail) -> Self {
        value.0
    }
}
impl fmt::Display for CronDiagnosticDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CronModuleOutput {
    Message { body: CronOutputBody },
    Error { body: CronOutputBody },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CronFailureKind {
    Timeout,
    MalformedReply,
    RunnerExit,
    Refused,
    ActionFailed,
    StopCheckFailed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CronFailure {
    pub kind: CronFailureKind,
    pub code: Option<AtmErrorCode>,
    pub detail: CronDiagnosticDetail,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CronFailureKey {
    pub kind: CronFailureKind,
    pub stable_code: Option<AtmErrorCode>,
    pub stable_reason: String,
}

impl CronFailure {
    #[must_use]
    pub fn fingerprint(&self) -> CronFailureKey {
        // Callers must create `detail` only after redaction/normalization.
        CronFailureKey {
            kind: self.kind,
            stable_code: self.code,
            stable_reason: self.detail.0.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CronSuccess {
    Ok,
    Actions { count: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CronRunTransition {
    Success { outcome: CronSuccess },
    StopCheckContinues,
    Failure(CronFailure),
    Stopped { reason: CronDiagnosticDetail },
    SkippedInFlight,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct CronNoticeKey {
    pub schedule_id: CronScheduleId,
    pub generation: CronGeneration,
    pub transition_seq: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CronLifecycleEvent {
    Error { failure: CronFailure },
    Stopped { message: CronDiagnosticDetail },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CronNoticeIntent {
    pub key: CronNoticeKey,
    pub team: TeamName,
    pub recipient: AgentName,
    pub name: CronName,
    pub event: CronLifecycleEvent,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CronDeliveryKey {
    Lifecycle(CronNoticeKey),
    ModuleMail {
        schedule_id: CronScheduleId,
        generation: CronGeneration,
        transition_seq: u64,
        output_index: u16,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CronModuleMailIntent {
    pub key: CronDeliveryKey,
    pub message_id: AtmMessageId,
    pub team: TeamName,
    pub recipient: AgentName,
    pub name: CronName,
    pub output: CronModuleOutput,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CronDeliveryIntent {
    Lifecycle(CronNoticeIntent),
    ModuleMail(CronModuleMailIntent),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CronTransitionWrite {
    pub id: CronScheduleId,
    pub generation: CronGeneration,
    pub at: IsoTimestamp,
    pub transition: CronRunTransition,
    pub outputs: Vec<CronModuleOutput>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CronCommitResult {
    Applied { intents: Vec<CronDeliveryIntent> },
    StaleGeneration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryReceipt {
    Accepted { receipt_id: String },
    Suppressed,
    RecipientUnavailable,
}

pub trait CronTransitionStore: sealed::Sealed + Send + Sync {
    fn commit(&self, write: CronTransitionWrite) -> Result<CronCommitResult, AtmError>;
    fn pending(&self, limit: usize) -> Result<Vec<CronDeliveryIntent>, AtmError>;
    fn mark_delivered(
        &self,
        key: CronDeliveryKey,
        receipt: DeliveryReceipt,
    ) -> Result<bool, AtmError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_and_diagnostic_have_different_bounds() {
        assert!(CronOutputBody::parse(String::new()).is_err());
        assert!(CronDiagnosticDetail::parse("\n".into()).is_err());
        assert_eq!(
            CronOutputBody::parse("custom mail".into())
                .unwrap()
                .as_str(),
            "custom mail"
        );
    }
}
