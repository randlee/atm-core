//! ATM-owned observability boundary and projected log/health types.

use std::path::PathBuf;

use serde::de::Error as DeError;
use serde::ser::{Error as SerError, SerializeMap};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use tracing::warn;

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

/// ATM-owned field-key type for observability query and record projections.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogFieldKey(String);

impl LogFieldKey {
    /// Construct a validated ATM log-field key.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the provided key is empty or whitespace-only.
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(
                AtmError::validation("ATM log field key must not be empty").with_recovery(
                    "Provide a non-empty field key when building ATM log queries or records.",
                ),
            );
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for LogFieldKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LogFieldKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// ATM-owned validated JSON-number representation for the observability
/// boundary.
#[derive(Debug, Clone)]
pub struct AtmJsonNumber {
    raw: String,
    number: serde_json::Number,
    normalized: String,
}

impl AtmJsonNumber {
    /// Construct a validated ATM JSON number from a raw numeric string.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the input is not a valid RFC 8259 JSON
    /// number. Non-JSON values such as `NaN` and `Infinity` are rejected.
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        let parsed: Value = serde_json::from_str(&value).map_err(|source| {
            AtmError::validation(format!("invalid ATM JSON number `{value}`"))
                .with_recovery(
                    "Provide a valid RFC 8259 JSON number such as `1`, `-2.5`, or `6.02e23`.",
                )
                .with_source(source)
        })?;
        match parsed {
            Value::Number(number) => Ok(Self {
                raw: value.clone(),
                number,
                normalized: normalize_json_number(&value),
            }),
            _ => Err(
                AtmError::validation(format!("invalid ATM JSON number `{value}`")).with_recovery(
                    "Provide a valid RFC 8259 JSON number such as `1`, `-2.5`, or `6.02e23`.",
                ),
            ),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    fn to_json_number(&self) -> serde_json::Number {
        self.number.clone()
    }
}

impl PartialEq for AtmJsonNumber {
    fn eq(&self, other: &Self) -> bool {
        self.normalized == other.normalized
    }
}

impl Eq for AtmJsonNumber {}

impl Serialize for AtmJsonNumber {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_json_number().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AtmJsonNumber {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::Number(number) => Self::new(number.to_string()).map_err(D::Error::custom),
            _ => Err(D::Error::custom("expected a JSON number")),
        }
    }
}

/// ATM-owned recursive JSON-value wrapper used by the observability boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogFieldValue {
    Null,
    Bool(bool),
    String(String),
    Number(AtmJsonNumber),
    Array(Vec<LogFieldValue>),
    Object(LogFieldMap),
}

impl LogFieldValue {
    pub fn null() -> Self {
        Self::Null
    }

    pub fn bool(value: bool) -> Self {
        Self::Bool(value)
    }

    pub fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }

    pub fn number(value: AtmJsonNumber) -> Self {
        Self::Number(value)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Convert a serde_json value into the ATM-owned field-value wrapper.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when a nested field key or JSON number fails ATM
    /// validation.
    pub(crate) fn from_json_value(value: Value) -> Result<Self, AtmError> {
        match value {
            Value::Null => Ok(Self::Null),
            Value::Bool(value) => Ok(Self::Bool(value)),
            Value::String(value) => Ok(Self::String(value)),
            Value::Number(value) => Ok(Self::Number(AtmJsonNumber::new(value.to_string())?)),
            Value::Array(values) => values
                .into_iter()
                .map(Self::from_json_value)
                .collect::<Result<Vec<_>, _>>()
                .map(Self::Array),
            Value::Object(values) => LogFieldMap::from_json_map(values).map(Self::Object),
        }
    }

    /// Convert the ATM-owned field-value wrapper into a serde_json value.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when a nested ATM-owned JSON number cannot be
    /// materialized as a JSON value.
    pub(crate) fn to_json_value(&self) -> Result<Value, AtmError> {
        match self {
            Self::Null => Ok(Value::Null),
            Self::Bool(value) => Ok(Value::Bool(*value)),
            Self::String(value) => Ok(Value::String(value.clone())),
            Self::Number(value) => Ok(Value::Number(value.to_json_number())),
            Self::Array(values) => values
                .iter()
                .map(Self::to_json_value)
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            Self::Object(values) => values.to_json_map().map(Value::Object),
        }
    }
}

impl Serialize for LogFieldValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_json_value()
            .map_err(S::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LogFieldValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Self::from_json_value(value).map_err(D::Error::custom)
    }
}

/// ATM-owned map wrapper used by public observability record projections.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogFieldMap {
    entries: Vec<(LogFieldKey, LogFieldValue)>,
}

impl LogFieldMap {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&LogFieldValue> {
        self.entries
            .iter()
            .find_map(|(entry_key, entry_value)| (entry_key.as_str() == key).then_some(entry_value))
    }

    /// Convert a serde_json object into the ATM-owned field-map wrapper.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when a nested key or value fails ATM validation.
    pub(crate) fn from_json_map(values: Map<String, Value>) -> Result<Self, AtmError> {
        let entries = values
            .into_iter()
            .map(|(key, value)| {
                Ok((
                    LogFieldKey::new(key)?,
                    LogFieldValue::from_json_value(value)?,
                ))
            })
            .collect::<Result<Vec<_>, AtmError>>()?;
        Ok(Self { entries })
    }

    fn to_json_map(&self) -> Result<Map<String, Value>, AtmError> {
        // Duplicate keys collapse with a last-wins policy when projected back
        // into JSON. Serialize uses the same helper so the policy is
        // consistent across both outbound paths.
        self.entries
            .iter()
            .try_fold(Map::new(), |mut map, (key, value)| {
                map.insert(key.as_str().to_string(), value.to_json_value()?);
                Ok(map)
            })
    }
}

impl Serialize for LogFieldMap {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let json_map = self.to_json_map().map_err(S::Error::custom)?;
        let mut map = serializer.serialize_map(Some(json_map.len()))?;
        for (key, value) in json_map {
            map.serialize_entry(&key, &value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for LogFieldMap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Map::<String, Value>::deserialize(deserializer)?;
        Self::from_json_map(values).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LogFieldMatch {
    pub key: LogFieldKey,
    pub value: LogFieldValue,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AtmLogQuery {
    pub mode: LogMode,
    pub levels: Vec<LogLevelFilter>,
    pub field_matches: Vec<LogFieldMatch>,
    pub since: Option<IsoTimestamp>,
    pub until: Option<IsoTimestamp>,
    pub limit: Option<usize>,
    pub order: LogOrder,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AtmLogRecord {
    pub timestamp: IsoTimestamp,
    pub severity: LogLevelFilter,
    pub service: String,
    pub target: Option<String>,
    pub action: Option<String>,
    pub message: Option<String>,
    pub fields: LogFieldMap,
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

/// One follow/tail session over retained ATM observability records.
///
/// `LogTailSession` is `Send` but intentionally not `Sync`: callers should move
/// one session onto one polling task and share the owning `ObservabilityPort`
/// behind an `Arc` if multiple async tasks need to create independent sessions.
pub struct LogTailSession {
    inner: Box<dyn LogFollowPort>,
}

impl LogTailSession {
    /// Construct an empty follow session that never yields records.
    pub fn empty() -> Self {
        Self {
            inner: Box::<EmptyFollowPort>::default(),
        }
    }

    /// Construct one follow session from a polling closure.
    pub fn from_poller<F>(poller: F) -> Self
    where
        F: FnMut() -> Result<AtmLogSnapshot, AtmError> + Send + 'static,
    {
        Self {
            inner: Box::new(ClosureFollowPort { poller }),
        }
    }

    /// Poll the next batch of followed log records.
    ///
    /// # Errors
    ///
    /// Returns an [`AtmError`] when the underlying follow session cannot
    /// produce the next batch of retained records.
    pub fn poll(&mut self) -> Result<AtmLogSnapshot, AtmError> {
        self.inner.poll()
    }
}

pub trait ObservabilityPort: sealed::Sealed {
    /// Emit one ATM command event into the configured observability sink.
    ///
    /// # Errors
    ///
    /// Returns an [`AtmError`] when the shared observability backend rejects
    /// or cannot persist the event.
    fn emit(&self, event: CommandEvent) -> Result<(), AtmError>;
    /// Query retained ATM observability records.
    ///
    /// # Errors
    ///
    /// Returns an [`AtmError`] when the shared backend cannot execute the
    /// query or when ATM-specific query projection fails.
    fn query(&self, req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError>;
    /// Start a retained follow/tail session for ATM observability records.
    ///
    /// The returned [`LogTailSession`] is designed for one polling owner at a
    /// time. Async callers that need multiple consumers should share the port
    /// behind an `Arc` and create one independent session per task.
    ///
    /// # Errors
    ///
    /// Returns an [`AtmError`] when the shared backend cannot start the follow
    /// session or ATM-specific query projection fails.
    fn follow(&self, req: AtmLogQuery) -> Result<LogTailSession, AtmError>;
    /// Report the current retained observability health state.
    ///
    /// # Errors
    ///
    /// Returns an [`AtmError`] when the shared backend health surface cannot
    /// be evaluated or projected into ATM-owned health types.
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

/// Normalize a JSON number string into a canonical decimal representation.
///
/// # Panics
///
/// This function does not panic on malformed exponents. If exponent parsing
/// fails unexpectedly, it logs a warning and preserves the original string.
fn normalize_json_number(raw: &str) -> String {
    let (negative, unsigned) = match raw.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let (base, exponent) = match unsigned.find(['e', 'E']) {
        Some(index) => match unsigned[index + 1..].parse::<i64>() {
            Ok(exponent) => (&unsigned[..index], exponent),
            Err(error) => {
                warn!(raw, %error, "failed to normalize JSON number exponent; preserving original value");
                return raw.to_string();
            }
        },
        None => (unsigned, 0),
    };
    let (integer, fraction) = match base.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (base, ""),
    };

    let mut digits = format!("{integer}{fraction}");
    let mut scale = exponent - fraction.len() as i64;

    let trimmed = digits.trim_start_matches('0');
    digits = if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    };
    if digits == "0" {
        return "0".to_string();
    }

    while digits.ends_with('0') {
        digits.pop();
        scale += 1;
    }

    let unsigned = if scale >= 0 {
        format!("{digits}{}", "0".repeat(scale as usize))
    } else {
        let point_index = digits.len() as i64 + scale;
        if point_index > 0 {
            let point_index = point_index as usize;
            format!("{}.{}", &digits[..point_index], &digits[point_index..])
        } else {
            format!("0.{}{}", "0".repeat((-point_index) as usize), digits)
        }
    };

    if negative {
        format!("-{unsigned}")
    } else {
        unsigned
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AtmJsonNumber, AtmLogQuery, AtmObservabilityHealthState, LogFieldKey, LogFieldMap,
        LogFieldValue, LogLevelFilter, LogMode, LogOrder, NullObservability, ObservabilityPort,
        normalize_json_number,
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

    #[test]
    fn atm_json_number_rejects_non_json_numeric_values() {
        assert!(AtmJsonNumber::new("NaN").is_err());
        assert!(AtmJsonNumber::new("Infinity").is_err());
        assert!(AtmJsonNumber::new("-Infinity").is_err());
    }

    #[test]
    fn atm_json_number_accepts_valid_json_numbers() {
        for raw in ["1", "1.5", "-42", "6.02e23", "1e-6"] {
            let number = AtmJsonNumber::new(raw).expect("valid number");
            let encoded = serde_json::to_string(&number).expect("serialize");
            let decoded: AtmJsonNumber = serde_json::from_str(&encoded).expect("deserialize");
            assert_eq!(decoded, number, "number `{raw}` should round-trip");
        }
    }

    #[test]
    fn atm_json_number_equality_is_value_based() {
        assert_eq!(
            AtmJsonNumber::new("1").expect("one"),
            AtmJsonNumber::new("1.0").expect("one point zero")
        );
        assert_eq!(
            AtmJsonNumber::new("1").expect("one"),
            AtmJsonNumber::new("1e0").expect("scientific")
        );
    }

    #[test]
    fn normalize_json_number_preserves_raw_string_for_malformed_exponent() {
        assert_eq!(normalize_json_number("1e-not-a-number"), "1e-not-a-number");
    }

    #[test]
    fn log_field_key_round_trips_through_json() {
        let key = LogFieldKey::new("task_id").expect("key");
        let encoded = serde_json::to_string(&key).expect("encode");
        let decoded: LogFieldKey = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(decoded, key);
    }

    #[test]
    fn log_field_value_variants_round_trip_through_json() {
        let object: LogFieldMap = serde_json::from_value(json!({
            "nested": true,
            "answer": 42
        }))
        .expect("object");
        let cases = vec![
            LogFieldValue::Null,
            LogFieldValue::Bool(true),
            LogFieldValue::String("hello".to_string()),
            LogFieldValue::Number(AtmJsonNumber::new("1.0").expect("number")),
            LogFieldValue::Array(vec![
                LogFieldValue::String("a".to_string()),
                LogFieldValue::Bool(false),
            ]),
            LogFieldValue::Object(object),
        ];

        for case in cases {
            let encoded = serde_json::to_value(&case).expect("encode value");
            let decoded: LogFieldValue = serde_json::from_value(encoded).expect("decode value");
            assert_eq!(decoded, case);
        }
    }

    #[test]
    fn log_field_map_round_trips_with_last_key_wins() {
        let map = LogFieldMap {
            entries: vec![
                (
                    LogFieldKey::new("dup").expect("key"),
                    LogFieldValue::String("first".to_string()),
                ),
                (
                    LogFieldKey::new("stable").expect("key"),
                    LogFieldValue::Bool(true),
                ),
                (
                    LogFieldKey::new("dup").expect("key"),
                    LogFieldValue::String("second".to_string()),
                ),
            ],
        };

        let encoded = serde_json::to_value(&map).expect("encode map");
        assert_eq!(
            encoded,
            json!({
                "dup": "second",
                "stable": true
            })
        );

        let decoded: LogFieldMap = serde_json::from_value(encoded).expect("decode map");
        assert_eq!(
            decoded.get("dup").and_then(LogFieldValue::as_str),
            Some("second")
        );
        assert_eq!(decoded.get("stable"), Some(&LogFieldValue::Bool(true)));
    }
}
