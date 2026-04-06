use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{AtmError, AtmErrorCode};
use crate::schema::LegacyMessageId;
use crate::types::IsoTimestamp;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CommandEvent {
    pub command: &'static str,
    pub action: &'static str,
    pub outcome: &'static str,
    pub team: String,
    pub agent: String,
    pub sender: String,
    pub message_id: Option<LegacyMessageId>,
    pub requires_ack: bool,
    pub dry_run: bool,
    pub task_id: Option<String>,
    pub error_code: Option<AtmErrorCode>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogMode {
    Snapshot,
    Tail,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogLevelFilter {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogOrder {
    NewestFirst,
    OldestFirst,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LogFieldMatch {
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AtmLogQuery {
    pub mode: LogMode,
    pub levels: Vec<LogLevelFilter>,
    pub field_matches: Vec<LogFieldMatch>,
    pub since: Option<IsoTimestamp>,
    pub until: Option<IsoTimestamp>,
    pub limit: Option<usize>,
    pub order: LogOrder,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AtmLogRecord {
    pub timestamp: IsoTimestamp,
    pub severity: LogLevelFilter,
    pub service: String,
    pub target: Option<String>,
    pub action: Option<String>,
    pub message: Option<String>,
    pub fields: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct AtmLogSnapshot {
    pub records: Vec<AtmLogRecord>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AtmObservabilityHealthState {
    Healthy,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AtmObservabilityHealth {
    pub active_log_path: Option<PathBuf>,
    pub logging_state: AtmObservabilityHealthState,
    pub query_state: Option<AtmObservabilityHealthState>,
    pub detail: Option<String>,
}

#[doc(hidden)]
pub mod sealed {
    pub trait Sealed {}
}

trait LogFollowPort: Send {
    fn poll(&mut self) -> Result<AtmLogSnapshot, AtmError>;
}

#[derive(Default)]
struct EmptyFollowPort;

impl LogFollowPort for EmptyFollowPort {
    fn poll(&mut self) -> Result<AtmLogSnapshot, AtmError> {
        Ok(AtmLogSnapshot::default())
    }
}

struct ClosureFollowPort<F> {
    poller: F,
}

impl<F> LogFollowPort for ClosureFollowPort<F>
where
    F: FnMut() -> Result<AtmLogSnapshot, AtmError> + Send,
{
    fn poll(&mut self) -> Result<AtmLogSnapshot, AtmError> {
        (self.poller)()
    }
}

pub struct LogTailSession {
    inner: Box<dyn LogFollowPort>,
}

impl LogTailSession {
    pub fn empty() -> Self {
        Self {
            inner: Box::<EmptyFollowPort>::default(),
        }
    }

    pub fn from_poller<F>(poller: F) -> Self
    where
        F: FnMut() -> Result<AtmLogSnapshot, AtmError> + Send + 'static,
    {
        Self {
            inner: Box::new(ClosureFollowPort { poller }),
        }
    }

    pub fn poll(&mut self) -> Result<AtmLogSnapshot, AtmError> {
        self.inner.poll()
    }
}

pub trait ObservabilityPort: sealed::Sealed {
    fn emit(&self, event: CommandEvent) -> Result<(), AtmError>;
    fn query(&self, req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError>;
    fn follow(&self, req: AtmLogQuery) -> Result<LogTailSession, AtmError>;
    fn health(&self) -> Result<AtmObservabilityHealth, AtmError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NullObservability;

impl sealed::Sealed for NullObservability {}

impl ObservabilityPort for NullObservability {
    fn emit(&self, _event: CommandEvent) -> Result<(), AtmError> {
        Ok(())
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        Ok(AtmLogSnapshot::default())
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        Ok(LogTailSession::empty())
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        Ok(AtmObservabilityHealth {
            active_log_path: None,
            logging_state: AtmObservabilityHealthState::Unavailable,
            query_state: Some(AtmObservabilityHealthState::Unavailable),
            detail: Some("observability adapter is not configured".to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AtmLogQuery, AtmObservabilityHealthState, LogLevelFilter, LogMode, LogOrder,
        NullObservability, ObservabilityPort,
    };
    use serde_json::json;

    fn empty_query() -> AtmLogQuery {
        AtmLogQuery {
            mode: LogMode::Snapshot,
            levels: vec![LogLevelFilter::Info],
            field_matches: vec![],
            since: None,
            until: None,
            limit: None,
            order: LogOrder::NewestFirst,
        }
    }

    #[test]
    fn null_observability_returns_empty_snapshot_and_tail() {
        let observability = NullObservability;
        let query = empty_query();

        let snapshot = observability.query(query.clone()).expect("snapshot");
        assert!(snapshot.records.is_empty());
        assert!(!snapshot.truncated);

        let mut tail = observability.follow(query).expect("tail");
        let follow = tail.poll().expect("follow poll");
        assert!(follow.records.is_empty());
    }

    #[test]
    fn null_observability_reports_unavailable_health() {
        let observability = NullObservability;

        let health = observability.health().expect("health");
        assert_eq!(
            health.logging_state,
            AtmObservabilityHealthState::Unavailable
        );
        assert_eq!(
            health.query_state,
            Some(AtmObservabilityHealthState::Unavailable)
        );
    }

    #[test]
    fn log_mode_serde_round_trips_using_snake_case_wire_format() {
        assert_eq!(
            serde_json::to_value(LogMode::Snapshot).unwrap(),
            json!("snapshot")
        );
        assert_eq!(serde_json::to_value(LogMode::Tail).unwrap(), json!("tail"));
        assert_eq!(
            serde_json::from_value::<LogMode>(json!("snapshot")).unwrap(),
            LogMode::Snapshot
        );
        assert_eq!(
            serde_json::from_value::<LogMode>(json!("tail")).unwrap(),
            LogMode::Tail
        );
    }
}
