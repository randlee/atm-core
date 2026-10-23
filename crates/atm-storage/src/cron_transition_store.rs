//! Durable cron transition and delivery intent contract.
//!
//! Implementations commit state plus intents atomically.  The contract creates
//! exactly one logical intent per transition; adapters must retain unknown
//! external handoffs for reconciliation rather than asserting external
//! exactly-once delivery.

use std::fmt;

use sc_lint_attributes::sc_lint;
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
    key: CronNoticeKey,
    team: TeamName,
    recipient: AgentName,
    name: CronName,
    event: CronLifecycleEvent,
}

/// A transition row that has already selected its lifecycle recipient.
///
/// This is deliberately private to the storage contract module.  An adapter
/// obtains it only while materializing a row committed by its transition
/// transaction; no scheduler, nudger, or caller can provide a recipient or
/// lifecycle body to the issuer.
#[allow(
    dead_code,
    reason = "BK-9 freezes the issuer seam before BK-10 supplies the SQLite transition adapter"
)]
struct CommittedCronNoticeRecord {
    key: CronNoticeKey,
    team: TeamName,
    recipient: AgentName,
    name: CronName,
    event: CronLifecycleEvent,
}

/// Mints a lifecycle intent from a committed transition record.
///
/// The issuer is intentionally confined to this module.  It is the one
/// construction seam for [`CronNoticeIntent`], and the boundary linter
/// rejects an implementation from any other module.
#[sc_lint(boundary.forbid_external_impls)]
#[allow(
    dead_code,
    reason = "BK-9 freezes the issuer seam before BK-10 supplies the SQLite transition adapter"
)]
trait CronNoticeIntentIssuer: sealed::Sealed {
    fn issue(&self, record: CommittedCronNoticeRecord) -> CronNoticeIntent;
}

/// The storage-owned issuer used by approved transition adapters.
#[allow(
    dead_code,
    reason = "BK-9 freezes the issuer seam before BK-10 supplies the SQLite transition adapter"
)]
struct CommittedCronNoticeIssuer;

impl sealed::Sealed for CommittedCronNoticeIssuer {}

impl CronNoticeIntentIssuer for CommittedCronNoticeIssuer {
    fn issue(&self, record: CommittedCronNoticeRecord) -> CronNoticeIntent {
        CronNoticeIntent {
            key: record.key,
            team: record.team,
            recipient: record.recipient,
            name: record.name,
            event: record.event,
        }
    }
}

impl CronNoticeIntent {
    /// Returns the durable transition key selected when this notice was
    /// committed.
    #[must_use]
    pub fn key(&self) -> CronNoticeKey {
        self.key
    }

    /// Returns the persisted schedule team associated with this notice.
    #[must_use]
    pub fn team(&self) -> &TeamName {
        &self.team
    }

    /// Returns the creator recipient resolved at the committed transition.
    ///
    /// The recipient has no public setter or constructor path: a nudger can
    /// read the persisted destination but cannot redirect the lifecycle mail.
    #[must_use]
    pub fn recipient(&self) -> &AgentName {
        &self.recipient
    }

    /// Returns the persisted schedule name.
    #[must_use]
    pub fn name(&self) -> &CronName {
        &self.name
    }

    /// Returns the committed lifecycle event.
    #[must_use]
    pub fn event(&self) -> &CronLifecycleEvent {
        &self.event
    }
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
    use std::str::FromStr;

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

    #[test]
    fn committed_lifecycle_intent_exposes_read_only_provenance() {
        let key = CronNoticeKey {
            schedule_id: CronScheduleId::new(7),
            generation: CronGeneration::FIRST,
            transition_seq: 3,
        };
        let committed_row = CommittedCronNoticeRecord {
            key,
            team: TeamName::from_str("ops").expect("team"),
            recipient: AgentName::from_str("creator").expect("recipient"),
            name: CronName::parse("hourly").expect("name"),
            event: CronLifecycleEvent::Stopped {
                message: CronDiagnosticDetail::parse("finished".into()).expect("detail"),
            },
        };
        let intent = CommittedCronNoticeIssuer.issue(committed_row);

        assert_eq!(intent.key(), key);
        assert_eq!(intent.team().as_str(), "ops");
        assert_eq!(intent.recipient().as_str(), "creator");
        assert_eq!(intent.name().as_str(), "hourly");
        assert!(matches!(intent.event(), CronLifecycleEvent::Stopped { .. }));
    }

    #[test]
    fn committed_notice_issuer_is_the_only_construction_seam() {
        let source = include_str!("cron_transition_store.rs");
        let public_issue = ["pub", "fn", "issue("].join(" ");
        let public_record = ["pub", "struct", "CommittedCronNoticeRecord"].join(" ");
        assert!(
            source.contains("#[sc_lint(boundary.forbid_external_impls)]"),
            "the issuer must retain executable external-implementation enforcement"
        );
        assert!(
            source.contains("trait CronNoticeIntentIssuer: sealed::Sealed"),
            "the issuer must remain a sealed adapter capability"
        );
        assert!(
            !source.contains(&public_issue) && !source.contains(&public_record),
            "callers must not mint lifecycle intent provenance"
        );
    }
}
