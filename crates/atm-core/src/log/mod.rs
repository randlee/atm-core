pub mod filters;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::AtmError;
use crate::observability::{LogFollowSession, ObservabilityPort};
use crate::types::IsoTimestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogFieldFilter {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogQuery {
    pub level: Option<LogLevel>,
    pub filters: Vec<LogFieldFilter>,
    pub follow: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogRecord {
    pub timestamp: IsoTimestamp,
    pub level: LogLevel,
    pub service: String,
    pub event: String,
    pub message: Option<String>,
    pub fields: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogQueryResult {
    pub action: &'static str,
    pub follow: bool,
    pub records: Vec<LogRecord>,
}

pub fn query_logs(
    query: LogQuery,
    observability: &dyn ObservabilityPort,
) -> Result<LogQueryResult, AtmError> {
    observability.query_logs(&query)
}

pub fn follow_logs(
    query: LogQuery,
    observability: &dyn ObservabilityPort,
) -> Result<Box<dyn LogFollowSession>, AtmError> {
    observability.follow_logs(&query)
}
