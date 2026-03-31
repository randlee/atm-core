use serde_json::Value;

use super::{LogFieldFilter, LogLevel, LogRecord};

pub fn matches_query(
    record: &LogRecord,
    level: Option<LogLevel>,
    filters: &[LogFieldFilter],
) -> bool {
    level.map_or(true, |level_filter| record.level == level_filter)
        && filters
            .iter()
            .all(|filter| matches_field_filter(record, filter))
}

fn matches_field_filter(record: &LogRecord, filter: &LogFieldFilter) -> bool {
    match record.fields.get(&filter.key) {
        Some(Value::String(value)) => value == &filter.value,
        Some(value) => render_value(value) == filter.value,
        None => false,
    }
}

fn render_value(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::types::IsoTimestamp;

    use super::{matches_query, LogFieldFilter, LogLevel, LogRecord};

    fn record() -> LogRecord {
        LogRecord {
            timestamp: IsoTimestamp::now(),
            level: LogLevel::Warn,
            service: "atm".into(),
            event: "command.failed".into(),
            message: Some("failed".into()),
            fields: serde_json::from_value(json!({
                "command": "read",
                "team": "atm-dev",
                "count": 2
            }))
            .expect("fields"),
        }
    }

    #[test]
    fn matches_level_filter() {
        assert!(matches_query(&record(), Some(LogLevel::Warn), &[]));
        assert!(!matches_query(&record(), Some(LogLevel::Info), &[]));
    }

    #[test]
    fn matches_field_filters() {
        let record = record();
        assert!(matches_query(
            &record,
            None,
            &[LogFieldFilter {
                key: "command".into(),
                value: "read".into(),
            }]
        ));
        assert!(matches_query(
            &record,
            None,
            &[LogFieldFilter {
                key: "count".into(),
                value: "2".into(),
            }]
        ));
        assert!(!matches_query(
            &record,
            None,
            &[LogFieldFilter {
                key: "command".into(),
                value: "send".into(),
            }]
        ));
    }
}
