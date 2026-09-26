//! First-party, best-effort telemetry seam for task-ledger facts.
//!
//! Records contain only typed task metadata already present in durable task
//! events or prompt handoffs. Message bodies, template variables, and free-form
//! event details are deliberately excluded.

use std::future::Future;
use std::pin::Pin;

use atm_storage::{
    AgentName, AtmMessageId, BuiltInNudgeTemplateKind, IsoTimestamp, PromptTrigger,
    ReminderOutcome, TaskActor, TaskCloseOutcome, TaskEventMarker, TaskId, TaskStateTag, TeamName,
};
use serde::{Deserialize, Serialize};

use crate::EnvSource;
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;

const ATM_OTEL_ENDPOINT: &str = "ATM_OTEL_ENDPOINT";
const ATM_OTEL_PROTOCOL: &str = "ATM_OTEL_PROTOCOL";
const ATM_OTEL_AUTH_HEADER: &str = "ATM_OTEL_AUTH_HEADER";
const ATM_OTEL_SERVICE_NAME: &str = "ATM_OTEL_SERVICE_NAME";
const DEFAULT_SERVICE_NAME: &str = "atm-daemon";

/// One redacted task-ledger fact suitable for telemetry export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskTelemetryRecord {
    pub kind: TaskTelemetryKind,
    pub team: TeamName,
    pub task_id: TaskId,
    pub assignee: AgentName,
    pub actor: TaskActor,
    pub seq: Option<u64>,
    pub at: IsoTimestamp,
    pub from_state: Option<TaskStateTag>,
    pub to_state: Option<TaskStateTag>,
    pub close_outcome: Option<TaskCloseOutcome>,
    pub message_id: Option<AtmMessageId>,
    pub reminder_outcome: Option<ReminderOutcome>,
    pub marker: Option<TaskEventMarker>,
    pub handoff: Option<TaskHandoffFacts>,
}

/// Prompt-handoff facts that have no corresponding task-event sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskHandoffFacts {
    pub attempt: u32,
    pub trigger: PromptTrigger,
    pub nudge_kind: BuiltInNudgeTemplateKind,
}

/// Stable task telemetry event classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskTelemetryKind {
    Assigned,
    Acked,
    Started,
    Completed,
    Refused,
    Cancelled,
    Reassigned,
    Reopened,
    Rejected,
    Reminded,
    LeadNotified,
    Moved,
    Migrated,
    RemindersReset,
    PromptHandoff,
}

impl TaskTelemetryKind {
    /// Returns the stable task-event spelling used by exporters.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Assigned => "assigned",
            Self::Acked => "acked",
            Self::Started => "started",
            Self::Completed => "completed",
            Self::Refused => "refused",
            Self::Cancelled => "cancelled",
            Self::Reassigned => "reassigned",
            Self::Reopened => "reopened",
            Self::Rejected => "rejected",
            Self::Reminded => "reminded",
            Self::LeadNotified => "lead_notified",
            Self::Moved => "moved",
            Self::Migrated => "migrated",
            Self::RemindersReset => "reminders_reset",
            Self::PromptHandoff => "prompt_handoff",
        }
    }
}

/// Best-effort task telemetry delivery failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskTelemetryError {
    Unavailable,
    Rejected,
    TimedOut,
}

/// BOUNDARY-TaskTelemetrySink — an object-safe, first-party-only sink.
pub trait TaskTelemetrySink: crate::boundary::sealed::Sealed + Send + Sync {
    /// Emits one task telemetry record without affecting task processing.
    fn emit(
        &self,
        record: TaskTelemetryRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), TaskTelemetryError>> + Send + '_>>;
}

/// Inert built-in default used when task telemetry export is disabled.
#[derive(Debug, Default)]
pub struct NoopTaskTelemetrySink;

impl crate::boundary::sealed::Sealed for NoopTaskTelemetrySink {}

impl TaskTelemetrySink for NoopTaskTelemetrySink {
    fn emit(
        &self,
        _record: TaskTelemetryRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), TaskTelemetryError>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }
}

/// Wire protocol used by the configured OpenTelemetry export endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryExportProtocol {
    HttpJson,
    Grpc,
}

/// Validated environment configuration for the task telemetry exporter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryExportConfig {
    pub endpoint: String,
    pub protocol: TelemetryExportProtocol,
    pub auth_header: Option<String>,
    pub service_name: String,
}

impl TelemetryExportConfig {
    /// Reads and validates the optional exporter configuration.
    ///
    /// # Errors
    ///
    /// Returns a stable configuration error when a configured endpoint is
    /// empty or the protocol value is unsupported.
    pub fn from_env(env: &dyn EnvSource) -> Result<Option<Self>, AtmError> {
        let Some(endpoint) = env.var(ATM_OTEL_ENDPOINT) else {
            return Ok(None);
        };
        if endpoint.trim().is_empty() {
            return Err(config_invalid("ATM_OTEL_ENDPOINT must not be empty"));
        }

        let protocol = match env.var(ATM_OTEL_PROTOCOL).as_deref() {
            None | Some("grpc") => TelemetryExportProtocol::Grpc,
            Some("http/json") => TelemetryExportProtocol::HttpJson,
            Some(value) => {
                return Err(config_invalid(format!(
                    "ATM_OTEL_PROTOCOL must be 'grpc' or 'http/json', got '{value}'"
                )));
            }
        };

        Ok(Some(Self {
            endpoint,
            protocol,
            auth_header: env.var(ATM_OTEL_AUTH_HEADER),
            service_name: env
                .var(ATM_OTEL_SERVICE_NAME)
                .unwrap_or_else(|| DEFAULT_SERVICE_NAME.to_string()),
        }))
    }
}

fn config_invalid(detail: impl Into<String>) -> AtmError {
    AtmError::new(AtmErrorCode::TelemetryExportConfigInvalid, detail)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::test_support::FakeEnvSource;

    #[test]
    fn boundary_is_object_safe() {
        fn accepts_dyn(_: &dyn TaskTelemetrySink) {}
        let sink: Arc<dyn TaskTelemetrySink> = Arc::new(NoopTaskTelemetrySink);
        accepts_dyn(&*sink);
    }

    #[test]
    fn kind_strings_cover_every_task_event_kind() {
        let values = [
            (TaskTelemetryKind::Assigned, "assigned"),
            (TaskTelemetryKind::Acked, "acked"),
            (TaskTelemetryKind::Started, "started"),
            (TaskTelemetryKind::Completed, "completed"),
            (TaskTelemetryKind::Refused, "refused"),
            (TaskTelemetryKind::Cancelled, "cancelled"),
            (TaskTelemetryKind::Reassigned, "reassigned"),
            (TaskTelemetryKind::Reopened, "reopened"),
            (TaskTelemetryKind::Rejected, "rejected"),
            (TaskTelemetryKind::Reminded, "reminded"),
            (TaskTelemetryKind::LeadNotified, "lead_notified"),
            (TaskTelemetryKind::Moved, "moved"),
            (TaskTelemetryKind::Migrated, "migrated"),
            (TaskTelemetryKind::RemindersReset, "reminders_reset"),
            (TaskTelemetryKind::PromptHandoff, "prompt_handoff"),
        ];
        assert_eq!(values.len(), 15);
        for (kind, expected) in values {
            assert_eq!(kind.as_str(), expected);
            assert_eq!(
                serde_json::to_string(&kind).unwrap(),
                format!("\"{expected}\"")
            );
        }
    }

    #[test]
    fn error_variants_match_workflow_telemetry_error() {
        use crate::workflow_telemetry::WorkflowTelemetryError as Workflow;

        let task = [
            TaskTelemetryError::Unavailable,
            TaskTelemetryError::Rejected,
            TaskTelemetryError::TimedOut,
        ];
        let workflow = [
            Workflow::Unavailable,
            Workflow::Rejected,
            Workflow::TimedOut,
        ];
        assert_eq!(task.len(), workflow.len());
        for (task, workflow) in task.into_iter().zip(workflow) {
            assert_eq!(format!("{task:?}"), format!("{workflow:?}"));
        }
    }

    #[test]
    fn record_round_trips_every_typed_field() {
        let record = TaskTelemetryRecord {
            kind: TaskTelemetryKind::PromptHandoff,
            team: "atm-dev".parse().unwrap(),
            task_id: "atm-bd-1".parse().unwrap(),
            assignee: "solar".parse().unwrap(),
            actor: TaskActor::Member("solar".parse().unwrap()),
            seq: None,
            at: "2026-09-26T08:00:00Z".parse().unwrap(),
            from_state: Some(TaskStateTag::Assigned),
            to_state: Some(TaskStateTag::Active),
            close_outcome: Some(TaskCloseOutcome::Completed),
            message_id: Some("01M3EDTWNAAFMF14XDH52PTYMR".parse().unwrap()),
            reminder_outcome: Some(ReminderOutcome::Emitted),
            marker: Some(TaskEventMarker::Resend),
            handoff: Some(TaskHandoffFacts {
                attempt: 2,
                trigger: PromptTrigger::TaskPass,
                nudge_kind: BuiltInNudgeTemplateKind::TaskReminder,
            }),
        };
        let encoded = serde_json::to_string(&record).unwrap();
        assert_eq!(
            serde_json::from_str::<TaskTelemetryRecord>(&encoded).unwrap(),
            record
        );
    }

    #[test]
    fn export_config_from_env_table() {
        assert_eq!(
            TelemetryExportConfig::from_env(&FakeEnvSource::empty()).unwrap(),
            None
        );

        let grpc = TelemetryExportConfig::from_env(&FakeEnvSource::from_pairs([(
            ATM_OTEL_ENDPOINT,
            "http://collector:4317",
        )]))
        .unwrap()
        .unwrap();
        assert_eq!(grpc.protocol, TelemetryExportProtocol::Grpc);
        assert_eq!(grpc.service_name, DEFAULT_SERVICE_NAME);

        let http = TelemetryExportConfig::from_env(&FakeEnvSource::from_pairs([
            (ATM_OTEL_ENDPOINT, "http://collector:4318"),
            (ATM_OTEL_PROTOCOL, "http/json"),
            (ATM_OTEL_AUTH_HEADER, "Bearer redacted"),
            (ATM_OTEL_SERVICE_NAME, "atm-test"),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(http.protocol, TelemetryExportProtocol::HttpJson);
        assert_eq!(http.auth_header.as_deref(), Some("Bearer redacted"));
        assert_eq!(http.service_name, "atm-test");

        for env in [
            FakeEnvSource::from_pairs([(ATM_OTEL_ENDPOINT, "")]),
            FakeEnvSource::from_pairs([
                (ATM_OTEL_ENDPOINT, "http://collector"),
                (ATM_OTEL_PROTOCOL, "xml"),
            ]),
        ] {
            let error = TelemetryExportConfig::from_env(&env).unwrap_err();
            assert_eq!(error.code(), AtmErrorCode::TelemetryExportConfigInvalid);
        }
    }
}
