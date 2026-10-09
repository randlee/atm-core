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
    pub template_kind: BuiltInNudgeTemplateKind,
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

const ATM_LOG_DESTINATION: &str = "ATM_LOG_DESTINATION";
const MAX_AUTH_HEADER_BYTES: usize = 8192;

/// Wire protocol used by the configured OpenTelemetry export endpoint.
///
/// Only gRPC is supported: the exporter uses the official OpenTelemetry tonic
/// transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryExportProtocol {
    Grpc,
}

/// Validated environment configuration for the task telemetry exporter.
///
/// Fields are private and [`TelemetryExportConfig::from_env`] is the only
/// constructor, so every value reachable through the accessors has been
/// validated. `Debug` redacts the auth header; there is no `Display`.
#[derive(Clone, PartialEq, Eq)]
pub struct TelemetryExportConfig {
    endpoint: String,
    protocol: TelemetryExportProtocol,
    auth_header: Option<String>,
    service_name: String,
}

impl std::fmt::Debug for TelemetryExportConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelemetryExportConfig")
            .field("endpoint", &self.endpoint)
            .field("protocol", &self.protocol)
            .field(
                "auth_header",
                &self.auth_header.as_ref().map(|_| "<redacted>"),
            )
            .field("service_name", &self.service_name)
            .finish()
    }
}

impl TelemetryExportConfig {
    /// Reads and validates the optional exporter configuration.
    ///
    /// # Errors
    ///
    /// Returns [`AtmErrorCode::TelemetryExportConfigInvalid`] when the
    /// endpoint is empty, not `http`/`https`, has no authority or embeds
    /// credentials; when the protocol is not `grpc`; when the auth header is
    /// malformed or longer than 8192 bytes; when an auth header would be sent
    /// over plain `http` to a non-loopback host; or when the service name is
    /// empty.
    pub fn from_env(env: &dyn EnvSource) -> Result<Option<Self>, AtmError> {
        let Some(endpoint) = env.var(ATM_OTEL_ENDPOINT) else {
            return Ok(None);
        };
        let endpoint = endpoint.trim().to_string();
        let target = parse_endpoint(&endpoint)?;

        let protocol = match env.var(ATM_OTEL_PROTOCOL).as_deref().map(str::trim) {
            None | Some("grpc") => TelemetryExportProtocol::Grpc,
            Some(value) => {
                return Err(config_invalid(format!(
                    "ATM_OTEL_PROTOCOL '{value}' is not supported; set ATM_OTEL_PROTOCOL=grpc \
                     (or unset it) and point ATM_OTEL_ENDPOINT at the collector's OTLP gRPC port"
                )));
            }
        };

        let auth_header = env.var(ATM_OTEL_AUTH_HEADER);
        if let Some(header) = auth_header.as_deref() {
            validate_auth_header(header)?;
            if !target.https && !target.loopback {
                return Err(config_invalid(
                    "ATM_OTEL_AUTH_HEADER requires an https ATM_OTEL_ENDPOINT for a non-loopback \
                     collector; use https:// or remove the auth header",
                ));
            }
        }

        let service_name = match env.var(ATM_OTEL_SERVICE_NAME) {
            None => DEFAULT_SERVICE_NAME.to_string(),
            Some(name) if name.trim().is_empty() => {
                return Err(config_invalid(
                    "ATM_OTEL_SERVICE_NAME must not be empty; unset it to use 'atm-daemon'",
                ));
            }
            Some(name) => name.trim().to_string(),
        };

        Ok(Some(Self {
            endpoint,
            protocol,
            auth_header,
            service_name,
        }))
    }

    /// The validated collector endpoint.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// The export wire protocol.
    #[must_use]
    pub const fn protocol(&self) -> TelemetryExportProtocol {
        self.protocol
    }

    /// The validated auth header value, when configured.
    #[must_use]
    pub fn auth_header(&self) -> Option<&str> {
        self.auth_header.as_deref()
    }

    /// The OpenTelemetry `service.name` resource value.
    #[must_use]
    pub fn service_name(&self) -> &str {
        &self.service_name
    }
}

/// Where structured log events are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogDestination {
    File,
    Otel,
    Both,
}

impl LogDestination {
    /// Reads `ATM_LOG_DESTINATION` (`file` default, `otel`, `both`).
    ///
    /// This is the single parser shared by the CLI and the daemon.
    ///
    /// # Errors
    ///
    /// Returns [`AtmErrorCode::TelemetryExportConfigInvalid`] for an unknown
    /// value, when `otel` or `both` is selected without `ATM_OTEL_ENDPOINT`, or
    /// when the export configuration itself is invalid.
    pub fn from_env(env: &dyn EnvSource) -> Result<Self, AtmError> {
        let destination = match env.var(ATM_LOG_DESTINATION).as_deref().map(str::trim) {
            None | Some("file") => return Ok(Self::File),
            Some("otel") => Self::Otel,
            Some("both") => Self::Both,
            Some(value) => {
                return Err(config_invalid(format!(
                    "ATM_LOG_DESTINATION '{value}' is not supported; use file, otel or both"
                )));
            }
        };
        if TelemetryExportConfig::from_env(env)?.is_none() {
            return Err(config_invalid(
                "ATM_LOG_DESTINATION=otel|both requires ATM_OTEL_ENDPOINT; set the collector \
                 endpoint or use ATM_LOG_DESTINATION=file",
            ));
        }
        Ok(destination)
    }
}

struct EndpointTarget {
    https: bool,
    loopback: bool,
}

fn parse_endpoint(endpoint: &str) -> Result<EndpointTarget, AtmError> {
    if endpoint.is_empty() {
        return Err(config_invalid("ATM_OTEL_ENDPOINT must not be empty"));
    }
    if endpoint.contains('?') || endpoint.contains('#') {
        return Err(config_invalid(
            "ATM_OTEL_ENDPOINT must not contain a query or fragment",
        ));
    }
    let Some((scheme, rest)) = endpoint.split_once("://") else {
        return Err(config_invalid(
            "ATM_OTEL_ENDPOINT must be an http:// or https:// URL",
        ));
    };
    let https = match scheme.to_ascii_lowercase().as_str() {
        "https" => true,
        "http" => false,
        _ => {
            return Err(config_invalid(format!(
                "ATM_OTEL_ENDPOINT scheme '{scheme}' is not supported; use http:// or https://"
            )));
        }
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return Err(config_invalid(
            "ATM_OTEL_ENDPOINT must not embed credentials; use ATM_OTEL_AUTH_HEADER",
        ));
    }
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    if host.is_empty() || authority.chars().any(char::is_whitespace) {
        return Err(config_invalid(
            "ATM_OTEL_ENDPOINT must name a host, for example http://localhost:4317",
        ));
    }
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<core::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    Ok(EndpointTarget { https, loopback })
}

fn validate_auth_header(header: &str) -> Result<(), AtmError> {
    if header.len() > MAX_AUTH_HEADER_BYTES {
        return Err(config_invalid(format!(
            "ATM_OTEL_AUTH_HEADER is {} bytes; the maximum is {MAX_AUTH_HEADER_BYTES}",
            header.len()
        )));
    }
    let valid = !header.trim().is_empty()
        && header
            .bytes()
            .all(|byte| byte == b'\t' || (0x20..=0x7e).contains(&byte));
    if !valid {
        return Err(config_invalid(
            "ATM_OTEL_AUTH_HEADER must be a non-empty header value of visible ASCII characters, \
             spaces or tabs",
        ));
    }
    Ok(())
}

fn config_invalid(detail: impl Into<String>) -> AtmError {
    AtmError::new(AtmErrorCode::TelemetryExportConfigInvalid, detail)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::test_support::{FakeEnvSource, TEST_TEAM};

    #[test]
    fn boundary_is_object_safe() {
        struct Sink;
        impl crate::boundary::sealed::Sealed for Sink {}
        impl TaskTelemetrySink for Sink {
            fn emit(
                &self,
                _record: TaskTelemetryRecord,
            ) -> Pin<Box<dyn Future<Output = Result<(), TaskTelemetryError>> + Send + '_>>
            {
                Box::pin(async { Ok(()) })
            }
        }
        fn accepts_dyn(_: &dyn TaskTelemetrySink) {}
        let sink: Arc<dyn TaskTelemetrySink> = Arc::new(Sink);
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

    fn full_record() -> TaskTelemetryRecord {
        TaskTelemetryRecord {
            kind: TaskTelemetryKind::PromptHandoff,
            team: TEST_TEAM.parse().unwrap(),
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
                template_kind: BuiltInNudgeTemplateKind::TaskReminder,
            }),
        }
    }

    #[test]
    fn record_round_trips_every_typed_field() {
        let record = full_record();
        let encoded = serde_json::to_string(&record).unwrap();
        assert_eq!(
            serde_json::from_str::<TaskTelemetryRecord>(&encoded).unwrap(),
            record
        );
    }

    /// Pins the payload-free field set (ADR-064). The exhaustive destructure
    /// fails to compile when a field is added, and the key-set check fails
    /// when a serialized key changes, so a payload, template variable or
    /// free-form detail field cannot be added without editing this test.
    #[test]
    fn record_field_set_is_pinned_and_payload_free() {
        let record = full_record();
        let TaskTelemetryRecord {
            kind: _,
            team: _,
            task_id: _,
            assignee: _,
            actor: _,
            seq: _,
            at: _,
            from_state: _,
            to_state: _,
            close_outcome: _,
            message_id: _,
            reminder_outcome: _,
            marker: _,
            handoff,
        } = record.clone();
        let TaskHandoffFacts {
            attempt: _,
            trigger: _,
            template_kind: _,
        } = handoff.unwrap();

        let encoded = serde_json::to_value(&record).unwrap();
        let keys = |value: &serde_json::Value| -> Vec<String> {
            let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };
        assert_eq!(
            keys(&encoded),
            [
                "actor",
                "assignee",
                "at",
                "close_outcome",
                "from_state",
                "handoff",
                "kind",
                "marker",
                "message_id",
                "reminder_outcome",
                "seq",
                "task_id",
                "team",
                "to_state",
            ]
        );
        assert_eq!(
            keys(&encoded["handoff"]),
            ["attempt", "template_kind", "trigger"]
        );
    }

    fn env(pairs: &[(&'static str, &str)]) -> FakeEnvSource {
        FakeEnvSource::from_pairs(pairs.iter().map(|(key, value)| (*key, value.to_string())))
    }

    fn rejects(pairs: &[(&'static str, &str)]) -> String {
        let error = TelemetryExportConfig::from_env(&env(pairs)).unwrap_err();
        assert_eq!(error.code(), AtmErrorCode::TelemetryExportConfigInvalid);
        error.to_string()
    }

    #[test]
    fn export_config_from_env_table() {
        assert_eq!(
            TelemetryExportConfig::from_env(&FakeEnvSource::empty()).unwrap(),
            None
        );

        let grpc =
            TelemetryExportConfig::from_env(&env(&[(ATM_OTEL_ENDPOINT, "http://collector:4317")]))
                .unwrap()
                .unwrap();
        assert_eq!(grpc.protocol(), TelemetryExportProtocol::Grpc);
        assert_eq!(grpc.endpoint(), "http://collector:4317");
        assert_eq!(grpc.auth_header(), None);
        assert_eq!(grpc.service_name(), DEFAULT_SERVICE_NAME);

        let authed = TelemetryExportConfig::from_env(&env(&[
            (ATM_OTEL_ENDPOINT, "https://collector.example:4317"),
            (ATM_OTEL_PROTOCOL, "grpc"),
            (ATM_OTEL_AUTH_HEADER, "Bearer s3cret-token"),
            (ATM_OTEL_SERVICE_NAME, "atm-test"),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(authed.auth_header(), Some("Bearer s3cret-token"));
        assert_eq!(authed.service_name(), "atm-test");

        for protocol in ["http/json", "http/protobuf", "xml"] {
            let message = rejects(&[
                (ATM_OTEL_ENDPOINT, "http://collector:4317"),
                (ATM_OTEL_PROTOCOL, protocol),
            ]);
            assert!(message.contains("ATM_OTEL_PROTOCOL=grpc"), "{message}");
        }
        rejects(&[(ATM_OTEL_ENDPOINT, "")]);
        rejects(&[(ATM_OTEL_ENDPOINT, "   ")]);
    }

    #[test]
    fn export_config_rejects_bad_endpoints() {
        for endpoint in [
            "collector:4317",
            "ftp://collector:4317",
            "unix:///tmp/otel.sock",
            "http://",
            "http:///v1",
            "http://:4317",
            "https://user:pass@collector:4317",
            "http://token@localhost:4317",
            "https://collector:4317/v1?api_key=secret",
            "https://collector:4317/v1#access_token=secret",
        ] {
            rejects(&[(ATM_OTEL_ENDPOINT, endpoint)]);
        }
        for endpoint in [
            "http://localhost:4317",
            "HTTPS://collector.example",
            "http://[::1]:4317/",
            "http://127.0.0.1:4317",
        ] {
            assert!(
                TelemetryExportConfig::from_env(&env(&[(ATM_OTEL_ENDPOINT, endpoint)]))
                    .unwrap()
                    .is_some(),
                "{endpoint}"
            );
        }
    }

    #[test]
    fn export_config_auth_header_rules() {
        let oversized = format!("Bearer {}", "a".repeat(MAX_AUTH_HEADER_BYTES));
        for header in [
            "",
            "  ",
            "Bearer a\r\nX-Injected: 1",
            "Bearer \u{e9}",
            &oversized,
        ] {
            rejects(&[
                (ATM_OTEL_ENDPOINT, "https://collector.example:4317"),
                (ATM_OTEL_AUTH_HEADER, header),
            ]);
        }
        let at_limit = "a".repeat(MAX_AUTH_HEADER_BYTES);
        assert!(
            TelemetryExportConfig::from_env(&env(&[
                (ATM_OTEL_ENDPOINT, "https://collector.example:4317"),
                (ATM_OTEL_AUTH_HEADER, &at_limit),
            ]))
            .unwrap()
            .is_some()
        );

        let message = rejects(&[
            (ATM_OTEL_ENDPOINT, "http://collector.example:4317"),
            (ATM_OTEL_AUTH_HEADER, "Bearer s3cret-token"),
        ]);
        assert!(message.contains("https"), "{message}");
        assert!(!message.contains("s3cret-token"), "{message}");
        for loopback in [
            "http://localhost:4317",
            "http://127.0.0.1:4317",
            "http://[::1]:4317",
        ] {
            assert!(
                TelemetryExportConfig::from_env(&env(&[
                    (ATM_OTEL_ENDPOINT, loopback),
                    (ATM_OTEL_AUTH_HEADER, "Bearer s3cret-token"),
                ]))
                .unwrap()
                .is_some(),
                "{loopback}"
            );
        }
        rejects(&[
            (ATM_OTEL_ENDPOINT, "http://localhost:4317"),
            (ATM_OTEL_SERVICE_NAME, " "),
        ]);
    }

    #[test]
    fn export_config_debug_redacts_auth_header() {
        let config = TelemetryExportConfig::from_env(&env(&[
            (ATM_OTEL_ENDPOINT, "https://collector.example:4317"),
            (ATM_OTEL_AUTH_HEADER, "Bearer s3cret-token"),
        ]))
        .unwrap()
        .unwrap();
        let debug = format!("{config:?}");
        assert!(!debug.contains("s3cret-token"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
        assert!(debug.contains("https://collector.example:4317"), "{debug}");
    }

    #[test]
    fn log_destination_from_env_table() {
        let endpoint = (ATM_OTEL_ENDPOINT, "http://localhost:4317");
        assert_eq!(
            LogDestination::from_env(&FakeEnvSource::empty()).unwrap(),
            LogDestination::File
        );
        assert_eq!(
            LogDestination::from_env(&env(&[(ATM_LOG_DESTINATION, "file")])).unwrap(),
            LogDestination::File
        );
        assert_eq!(
            LogDestination::from_env(&env(&[(ATM_LOG_DESTINATION, "otel"), endpoint])).unwrap(),
            LogDestination::Otel
        );
        assert_eq!(
            LogDestination::from_env(&env(&[(ATM_LOG_DESTINATION, "both"), endpoint])).unwrap(),
            LogDestination::Both
        );
        for pairs in [
            vec![(ATM_LOG_DESTINATION, "otel")],
            vec![(ATM_LOG_DESTINATION, "both")],
            vec![(ATM_LOG_DESTINATION, "syslog"), endpoint],
            vec![
                (ATM_LOG_DESTINATION, "otel"),
                (ATM_OTEL_ENDPOINT, "ftp://collector"),
            ],
        ] {
            let error = LogDestination::from_env(&env(&pairs)).unwrap_err();
            assert_eq!(error.code(), AtmErrorCode::TelemetryExportConfigInvalid);
        }
    }
}
