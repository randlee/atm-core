use anyhow::{Context, Result};
use atm_core::types::IsoTimestamp;

pub(crate) fn parse_timestamp(value: &str) -> Result<IsoTimestamp> {
    chrono::DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("invalid ISO 8601 timestamp: {value}"))
        .map(|timestamp| timestamp.with_timezone(&chrono::Utc).into())
}
