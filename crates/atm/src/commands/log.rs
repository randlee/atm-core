use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use atm_core::observability::{
    AtmLogQuery, LogFieldMatch, LogLevelFilter, LogMode, LogOrder, ObservabilityPort,
};
use atm_core::types::IsoTimestamp;
use clap::{Args, Subcommand, ValueEnum};
use serde_json::Value;

use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
pub struct LogCommand {
    #[command(subcommand)]
    mode: LogModeCommand,
}

impl LogCommand {
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        match self.mode {
            LogModeCommand::Snapshot(args) => {
                let snapshot = observability.query(args.build_query(LogMode::Snapshot)?)?;
                output::print_log_snapshot(&snapshot, args.json)
            }
            LogModeCommand::Filter(args) => {
                args.ensure_filter_present()?;
                let snapshot = observability.query(args.build_query(LogMode::Snapshot)?)?;
                output::print_log_snapshot(&snapshot, args.json)
            }
            LogModeCommand::Tail(args) => args.run(observability),
        }
    }
}

#[derive(Debug, Subcommand)]
enum LogModeCommand {
    /// Query recent ATM log records.
    Snapshot(QueryArgs),
    /// Query ATM log records using explicit field filters.
    Filter(QueryArgs),
    /// Follow new ATM log records as they arrive.
    Tail(TailArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliLogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<CliLogLevel> for LogLevelFilter {
    fn from(value: CliLogLevel) -> Self {
        match value {
            CliLogLevel::Trace => LogLevelFilter::Trace,
            CliLogLevel::Debug => LogLevelFilter::Debug,
            CliLogLevel::Info => LogLevelFilter::Info,
            CliLogLevel::Warn => LogLevelFilter::Warn,
            CliLogLevel::Error => LogLevelFilter::Error,
        }
    }
}

#[derive(Debug, Args)]
struct QueryArgs {
    /// Restrict results to one or more severity levels.
    #[arg(long = "level", value_enum)]
    levels: Vec<CliLogLevel>,

    /// Match one structured ATM field exactly, for example command=send.
    #[arg(long = "match", value_name = "KEY=VALUE")]
    matches: Vec<String>,

    /// Inclusive lower time bound as RFC3339 or a relative duration like 15m.
    #[arg(long)]
    since: Option<String>,

    /// Maximum number of returned records.
    #[arg(long)]
    limit: Option<usize>,

    /// Emit machine-readable JSON output.
    #[arg(long)]
    json: bool,
}

impl QueryArgs {
    fn build_query(&self, mode: LogMode) -> Result<AtmLogQuery> {
        let limit = match mode {
            LogMode::Snapshot => Some(self.limit.unwrap_or(50)),
            LogMode::Tail => self.limit,
        };

        Ok(AtmLogQuery {
            mode,
            levels: self.levels.iter().copied().map(Into::into).collect(),
            field_matches: self
                .matches
                .iter()
                .map(|raw| parse_match_expression(raw))
                .collect::<Result<Vec<_>>>()?,
            since: self.since.as_deref().map(parse_since).transpose()?,
            until: None,
            limit,
            order: LogOrder::NewestFirst,
        })
    }

    fn ensure_filter_present(&self) -> Result<()> {
        if self.matches.is_empty() && self.levels.is_empty() && self.since.is_none() {
            bail!("atm log filter requires at least one of --match, --level, or --since");
        }

        Ok(())
    }
}

#[derive(Debug, Args)]
struct TailArgs {
    #[command(flatten)]
    query: QueryArgs,

    /// Poll interval in milliseconds between follow polls.
    #[arg(long, default_value_t = 250)]
    poll_interval_ms: u64,

    /// Internal test seam to stop tail mode after a fixed number of polls.
    #[arg(long, hide = true)]
    max_polls: Option<usize>,
}

impl TailArgs {
    fn run(self, observability: &CliObservability) -> Result<()> {
        let mut session = observability.follow(self.query.build_query(LogMode::Tail)?)?;
        let mut polls = 0usize;

        loop {
            let snapshot = session.poll()?;
            output::print_log_records(snapshot.records, self.query.json)?;
            polls += 1;

            if self.max_polls.is_some_and(|limit| polls >= limit) {
                break;
            }

            thread::sleep(Duration::from_millis(self.poll_interval_ms));
        }

        Ok(())
    }
}

fn parse_match_expression(raw: &str) -> Result<LogFieldMatch> {
    let (key, value) = raw
        .split_once('=')
        .ok_or_else(|| anyhow::anyhow!("invalid --match expression '{raw}'; expected key=value"))?;

    if key.trim().is_empty() {
        bail!("invalid --match expression '{raw}'; key must not be empty");
    }

    Ok(LogFieldMatch {
        key: key.to_string(),
        value: parse_match_value(value),
    })
}

fn parse_match_value(raw: &str) -> Value {
    if raw.eq_ignore_ascii_case("true") {
        Value::Bool(true)
    } else if raw.eq_ignore_ascii_case("false") {
        Value::Bool(false)
    } else if raw.eq_ignore_ascii_case("null") {
        Value::Null
    } else if let Ok(number) = raw.parse::<i64>() {
        Value::Number(number.into())
    } else if let Ok(number) = raw.parse::<u64>() {
        Value::Number(number.into())
    } else if let Ok(number) = raw.parse::<f64>() {
        serde_json::Number::from_f64(number)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(raw.to_string()))
    } else {
        Value::String(raw.to_string())
    }
}

fn parse_since(raw: &str) -> Result<IsoTimestamp> {
    parse_rfc3339(raw).or_else(|_| parse_relative_duration(raw))
}

fn parse_rfc3339(raw: &str) -> Result<IsoTimestamp> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .with_context(|| format!("invalid RFC3339 timestamp: {raw}"))
        .map(|timestamp| timestamp.with_timezone(&chrono::Utc).into())
}

fn parse_relative_duration(raw: &str) -> Result<IsoTimestamp> {
    if raw.len() < 2 {
        bail!("invalid relative duration '{raw}'; expected forms like 30s, 15m, 2h, or 7d");
    }

    let (amount, unit) = raw.split_at(raw.len() - 1);
    let amount: i64 = amount.parse().with_context(|| {
        format!("invalid relative duration '{raw}'; duration amount must be an integer")
    })?;

    let delta = match unit {
        "s" => chrono::Duration::seconds(amount),
        "m" => chrono::Duration::minutes(amount),
        "h" => chrono::Duration::hours(amount),
        "d" => chrono::Duration::days(amount),
        _ => bail!("invalid relative duration '{raw}'; supported units are s, m, h, d"),
    };

    Ok((chrono::Utc::now() - delta).into())
}
