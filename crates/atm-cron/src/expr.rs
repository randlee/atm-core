use std::str::FromStr;

use atm_core::error::AtmError;
use chrono::{DateTime, Utc};

pub(crate) fn parse(expression: &str, now: DateTime<Utc>) -> Result<cron::Schedule, AtmError> {
    let schedule = parse_syntax(expression)?;
    if schedule.after(&now).next().is_none() {
        return Err(AtmError::cron_expr_invalid(
            "expression has no future occurrence",
        ));
    }
    Ok(schedule)
}

pub(crate) fn parse_syntax(expression: &str) -> Result<cron::Schedule, AtmError> {
    let trimmed = expression.trim();
    if !trimmed.starts_with('@') && trimmed.split_whitespace().count() == 5 {
        return Err(AtmError::cron_expr_invalid(
            "five-field expressions are not accepted; use 6 or 7 fields, seconds first, or @hourly..@yearly",
        ));
    }
    let schedule = cron::Schedule::from_str(trimmed)
        .map_err(|error| AtmError::cron_expr_invalid(&error.to_string()))?;
    Ok(schedule)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::Value;

    use super::parse_syntax;

    #[test]
    fn cron_docs_expression_fixtures_use_the_production_parser() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(repo_root.join("docs/user-documents/examples/cron/manifest.json"))
                .expect("read cron fixture manifest"),
        )
        .expect("parse cron fixture manifest");
        for fixture in manifest["expressions"]
            .as_array()
            .expect("expressions array")
        {
            let id = fixture["id"].as_str().expect("fixture id");
            let expression = fixture["value"].as_str().expect("expression");
            let expected_valid = fixture["valid"].as_bool().expect("valid flag");
            assert_eq!(
                parse_syntax(expression).is_ok(),
                expected_valid,
                "production cron parser disagrees with fixture {id}: {expression:?}"
            );
        }
    }
}
