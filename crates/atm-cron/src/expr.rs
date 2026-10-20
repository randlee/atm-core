use std::str::FromStr;

use atm_core::error::AtmError;
use chrono::{DateTime, Utc};

pub(crate) fn parse(expression: &str, now: DateTime<Utc>) -> Result<cron::Schedule, AtmError> {
    let trimmed = expression.trim();
    if !trimmed.starts_with('@') && trimmed.split_whitespace().count() == 5 {
        return Err(AtmError::cron_expr_invalid(
            "five-field expressions are not accepted; use 6 or 7 fields, seconds first, or @hourly..@yearly",
        ));
    }
    let schedule = cron::Schedule::from_str(trimmed)
        .map_err(|error| AtmError::cron_expr_invalid(&error.to_string()))?;
    if schedule.after(&now).next().is_none() {
        return Err(AtmError::cron_expr_invalid(
            "expression has no future occurrence",
        ));
    }
    Ok(schedule)
}
