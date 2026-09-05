use std::thread;

use anyhow::{Context, Result, bail};
use atm_core::observability::{
    AtmJsonNumber, AtmLogQuery, LogFieldKey, LogFieldMatch, LogFieldValue, LogLevelFilter, LogMode,
    LogOrder, ObservabilityPort,
};
use atm_core::types::IsoTimestamp;
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

use crate::commands::caller_context::{CallerContextOverrides, resolve_cli_caller_context};
use crate::observability::CliObservability;
use crate::output;

// Keep retained log snapshot output bounded for interactive use.
const DEFAULT_SNAPSHOT_LIMIT: usize = 50;
// Tail mode polls the shared follow surface at a human-readable cadence.
const DEFAULT_TAIL_POLL_INTERVAL_MS: u64 = 250;

#[derive(Debug, Args)]
/// Query or follow ATM retained observability records.
pub struct LogCommand {
    #[command(subcommand)]
    mode: LogModeCommand,
}

impl LogCommand {
    /// Execute the `atm log` command.
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let _caller_context = resolve_cli_caller_context(CallerContextOverrides::default())?;
        match self.mode {
            LogModeCommand::Snapshot(args) => {
                if args.source != LogSource::Jsonl {
                    return args.run_timeline();
                }
                let snapshot = observability.query(args.build_query(LogMode::Snapshot)?)?;
                output::print_log_snapshot(&snapshot, args.json)
            }
            LogModeCommand::Filter(args) => {
                args.ensure_filter_present()?;
                if args.source != LogSource::Jsonl {
                    return args.run_timeline();
                }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LogSource {
    /// The canonical retained JSONL file (the compatibility default).
    Jsonl,
    /// The daemon's bounded SQLite diagnostic timeline.
    Timeline,
    /// A bounded merged diagnostic view; sources are not lossless under overload.
    Merged,
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
    /// Select the retained-log source. JSONL is the byte-compatible default.
    #[arg(long, value_enum, default_value_t = LogSource::Jsonl)]
    source: LogSource,
    /// Restrict results to one or more severity levels.
    #[arg(long = "level", value_enum)]
    levels: Vec<CliLogLevel>,

    /// Match one structured ATM field exactly, for example command=send.
    #[arg(long = "match", value_name = "KEY=VALUE")]
    matches: Vec<String>,

    /// Inclusive lower time bound as RFC3339 or a relative duration like 15m.
    #[arg(long)]
    since: Option<String>,

    /// Inclusive upper time bound as RFC3339 or a relative duration.
    #[arg(long)]
    until: Option<String>,

    /// Restrict timeline records to a component prefix.
    #[arg(long)]
    component: Option<String>,

    /// Maximum number of returned records.
    #[arg(long)]
    limit: Option<usize>,

    /// Emit machine-readable JSON output.
    #[arg(long)]
    json: bool,
}

impl QueryArgs {
    fn run_timeline(&self) -> Result<()> {
        let runtime = atm_daemon_bootstrap::assemble_default_runtime()?;
        let level = self.levels.first().map(|level| match level {
            CliLogLevel::Trace => "trace",
            CliLogLevel::Debug => "debug",
            CliLogLevel::Info => "info",
            CliLogLevel::Warn => "warn",
            CliLogLevel::Error => "error",
        });
        let records = runtime
            .diagnostic_timeline
            .query(&atm_storage::DiagnosticQuery {
                since: self.since.as_deref().map(parse_since_millis).transpose()?,
                until: self.until.as_deref().map(parse_since_millis).transpose()?,
                level_at_least: level.map(str::to_owned),
                component_prefix: self.component.clone(),
                limit: Some(self.limit.unwrap_or(DEFAULT_SNAPSHOT_LIMIT).min(5_000)),
            })?;
        let note = "sources are independently bounded; merged view is not lossless under overload";
        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&TimelineOutput {
                    note,
                    records: records
                        .into_iter()
                        .map(|event| TimelineRecord::from_event(event, self.source))
                        .collect(),
                })?
            );
        } else {
            println!("Note: {note}");
            for event in records {
                println!(
                    "{} {} {} {}",
                    event.ts_unix_ms, event.level, event.component, event.message
                );
            }
        }
        Ok(())
    }

    fn build_query(&self, mode: LogMode) -> Result<AtmLogQuery> {
        let limit = match mode {
            LogMode::Snapshot => Some(self.limit.unwrap_or(DEFAULT_SNAPSHOT_LIMIT)),
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
            until: self.until.as_deref().map(parse_since).transpose()?,
            limit,
            order: LogOrder::NewestFirst,
        })
    }

    fn ensure_filter_present(&self) -> Result<()> {
        if self.matches.is_empty()
            && self.levels.is_empty()
            && self.since.is_none()
            && self.until.is_none()
        {
            bail!("atm log filter requires at least one of --match, --level, or --since");
        }

        Ok(())
    }
}

#[derive(Serialize)]
struct TimelineOutput {
    note: &'static str,
    records: Vec<TimelineRecord>,
}

#[derive(Serialize)]
struct TimelineRecord {
    source: &'static str,
    ts_unix_ms: i64,
    level: String,
    component: String,
    code: Option<String>,
    correlation_id: Option<String>,
    origin: String,
    message: String,
}

impl TimelineRecord {
    fn from_event(event: atm_storage::DiagnosticEvent, source: LogSource) -> Self {
        Self {
            source: match source {
                LogSource::Jsonl => "jsonl",
                LogSource::Timeline => "timeline",
                LogSource::Merged => "timeline",
            },
            ts_unix_ms: event.ts_unix_ms,
            level: event.level,
            component: event.component,
            code: event.code,
            correlation_id: event.correlation_id,
            origin: event.origin,
            message: event.message,
        }
    }
}

#[derive(Debug, Args)]
struct TailArgs {
    #[command(flatten)]
    query: QueryArgs,

    /// Poll interval in milliseconds between follow polls.
    #[arg(long, default_value_t = DEFAULT_TAIL_POLL_INTERVAL_MS)]
    poll_interval_ms: u64,

    /// Internal test seam to stop tail mode after a fixed number of polls.
    #[cfg(test)]
    #[arg(long, hide = true)]
    max_polls: Option<usize>,
}

impl TailArgs {
    #[cfg(not(test))]
    fn run(self, observability: &CliObservability) -> Result<()> {
        let mut session = observability.follow(self.query.build_query(LogMode::Tail)?)?;

        loop {
            let snapshot = session.poll()?;
            output::print_log_records(snapshot.records, self.query.json)?;
            thread::sleep(std::time::Duration::from_millis(self.poll_interval_ms));
        }
    }

    #[cfg(test)]
    fn run(self, observability: &CliObservability) -> Result<()> {
        let mut session = observability.follow(self.query.build_query(LogMode::Tail)?)?;
        let mut polls = 0usize;

        loop {
            let snapshot = session.poll()?;
            output::print_log_records(snapshot.records, self.query.json)?;
            polls += 1;

            if self.max_polls.is_some_and(|limit| polls >= limit) {
                return Ok(());
            }

            if self.max_polls.is_none() {
                thread::yield_now();
            }
        }
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
        key: LogFieldKey::new(key.to_string())?,
        value: parse_match_value(value),
    })
}

fn parse_match_value(raw: &str) -> LogFieldValue {
    if raw.eq_ignore_ascii_case("true") {
        LogFieldValue::bool(true)
    } else if raw.eq_ignore_ascii_case("false") {
        LogFieldValue::bool(false)
    } else if raw.eq_ignore_ascii_case("null") {
        LogFieldValue::null()
    } else if let Ok(number) = AtmJsonNumber::new(raw.to_string()) {
        LogFieldValue::number(number)
    } else {
        LogFieldValue::string(raw.to_string())
    }
}

fn parse_since(raw: &str) -> Result<IsoTimestamp> {
    parse_rfc3339(raw).or_else(|_| parse_relative_duration(raw))
}

fn parse_since_millis(raw: &str) -> Result<i64> {
    Ok(parse_since(raw)?.into_inner().timestamp_millis())
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

    let (amount, unit) = raw
        .char_indices()
        .next_back()
        .map(|(index, _)| (&raw[..index], &raw[index..]))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "invalid relative duration '{raw}'; expected forms like 30s, 15m, 2h, or 7d"
            )
        })?;
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use atm_core::error::AtmError;
    use atm_core::observability::{
        AtmLogRecord, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
        CommandEvent, LogTailSession, ObservabilityPort,
    };
    use atm_core::observability::{LogFieldValue, LogLevelFilter, LogMode};
    use atm_core::test_support::{EnvGuard, TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
    use serial_test::serial;
    use tempfile::TempDir;

    use super::{
        CliLogLevel, LogCommand, LogModeCommand, QueryArgs, parse_match_expression,
        parse_relative_duration,
    };
    use crate::observability::{CliObservability, CliObservabilityOptions};

    fn caller_context_env() -> EnvGuard {
        EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ])
    }

    #[derive(Debug)]
    struct StubObservability {
        // &self query/follow methods require interior mutability once tests store this behind Arc<dyn ObservabilityPort + Send + Sync>.
        snapshot: Mutex<Option<Result<AtmLogSnapshot, AtmError>>>,
    }

    impl atm_core::boundary::sealed::Sealed for StubObservability {}

    impl ObservabilityPort for StubObservability {
        fn emit(&self, _event: atm_core::observability::CommandEvent) -> Result<(), AtmError> {
            Ok(())
        }

        fn query(
            &self,
            _req: atm_core::observability::AtmLogQuery,
        ) -> Result<AtmLogSnapshot, AtmError> {
            self.snapshot
                .lock()
                .expect("snapshot")
                .take()
                .expect("single query result")
        }

        fn follow(
            &self,
            _req: atm_core::observability::AtmLogQuery,
        ) -> Result<LogTailSession, AtmError> {
            Ok(LogTailSession::from_poller(
                || Ok(AtmLogSnapshot::default()),
            ))
        }

        fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
            Ok(AtmObservabilityHealth {
                active_log_path: None,
                logging_state: AtmObservabilityHealthState::Healthy,
                query_state: Some(AtmObservabilityHealthState::Healthy),
                maintenance: None,
                diagnostic: None,
                detail: None,
            })
        }
    }

    #[test]
    fn parse_relative_duration_rejects_multibyte_suffix_without_panicking() {
        let error = parse_relative_duration("10µ").expect_err("invalid unit");
        assert!(
            error.to_string().contains("supported units are s, m, h, d"),
            "error: {error}"
        );
    }

    #[test]
    fn snapshot_query_defaults_limit_and_orders_newest_first() {
        let args = QueryArgs {
            levels: vec![CliLogLevel::Info],
            matches: vec!["command=send".to_string()],
            since: None,
            limit: None,
            json: true,
        };

        let query = args.build_query(LogMode::Snapshot).expect("query");

        assert_eq!(query.mode, LogMode::Snapshot);
        assert_eq!(query.levels, vec![LogLevelFilter::Info]);
        assert_eq!(query.limit, Some(super::DEFAULT_SNAPSHOT_LIMIT));
        assert_eq!(query.field_matches.len(), 1);
        assert_eq!(query.order, atm_core::observability::LogOrder::NewestFirst);
    }

    #[test]
    fn tail_query_preserves_explicit_limit_without_defaulting() {
        let args = QueryArgs {
            levels: vec![],
            matches: vec!["success=true".to_string()],
            since: Some("15m".to_string()),
            limit: Some(3),
            json: false,
        };

        let query = args.build_query(LogMode::Tail).expect("query");

        assert_eq!(query.mode, LogMode::Tail);
        assert_eq!(query.limit, Some(3));
        assert!(query.since.is_some());
        assert_eq!(query.field_matches[0].value, LogFieldValue::bool(true));
    }

    #[test]
    fn filter_mode_requires_at_least_one_predicate() {
        let args = QueryArgs {
            levels: vec![],
            matches: vec![],
            since: None,
            limit: None,
            json: false,
        };

        let error = args.ensure_filter_present().expect_err("missing filter");

        assert!(error.to_string().contains("requires at least one"));
    }

    #[test]
    fn parse_match_expression_coerces_supported_json_scalars() {
        let boolean = parse_match_expression("success=false").expect("bool");
        let null = parse_match_expression("task_id=null").expect("null");
        let number = parse_match_expression("attempts=7").expect("number");
        let string = parse_match_expression("command=send").expect("string");

        assert_eq!(boolean.value, LogFieldValue::bool(false));
        assert_eq!(null.value, LogFieldValue::null());
        assert_eq!(
            number.value,
            LogFieldValue::number(
                atm_core::observability::AtmJsonNumber::new("7".to_string()).expect("number")
            )
        );
        assert_eq!(string.value, LogFieldValue::string("send".to_string()));
    }

    #[test]
    #[serial(env)]
    fn run_snapshot_succeeds_with_fake_observability_snapshot() {
        let _caller = caller_context_env();
        let command = LogCommand {
            mode: LogModeCommand::Snapshot(QueryArgs {
                levels: vec![],
                matches: vec![],
                since: None,
                limit: Some(1),
                json: true,
            }),
        };
        let observability = CliObservability::from_test_port(StubObservability {
            snapshot: Mutex::new(Some(Ok(AtmLogSnapshot {
                records: vec![AtmLogRecord {
                    timestamp: chrono::Utc::now().into(),
                    level: LogLevelFilter::Info,
                    service: atm_core::observability::service_name("atm")
                        .expect("valid service name"),
                    target: None,
                    action: Some("send".to_string()),
                    message: Some("synthetic".to_string()),
                    fields: atm_core::observability::LogFieldMap::default(),
                }],
                truncated: false,
            }))),
        });

        command.run(&observability).expect("snapshot run");
    }

    #[test]
    #[serial(env)]
    fn run_snapshot_surfaces_observability_query_error() {
        let _caller = caller_context_env();
        let command = LogCommand {
            mode: LogModeCommand::Snapshot(QueryArgs {
                levels: vec![],
                matches: vec![],
                since: None,
                limit: None,
                json: false,
            }),
        };
        let observability = CliObservability::from_test_port(StubObservability {
            snapshot: Mutex::new(Some(Err(AtmError::observability_query(
                "synthetic snapshot failure",
            )))),
        });

        let error = command.run(&observability).expect_err("query error");

        assert_eq!(
            error.downcast_ref::<AtmError>().map(|atm| atm.code()),
            Some(atm_core::error_codes::AtmErrorCode::ObservabilityQueryFailed)
        );
    }

    #[test]
    #[serial(env)]
    fn run_snapshot_reads_real_retained_log_without_daemon() {
        let tempdir = TempDir::new().expect("tempdir");
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
            ("ATM_LOG", Some("info")),
        ]);
        let observability =
            CliObservability::new(tempdir.path(), CliObservabilityOptions::default())
                .expect("observability");
        observability
            .emit(CommandEvent {
                command: "send",
                action: atm_core::observability::action_name("send"),
                outcome: atm_core::observability::outcome_label("sent"),
                team: TEST_TEAM.parse().expect("team"),
                agent: TEST_RECIPIENT.parse().expect("agent"),
                sender: TEST_SENDER.parse().expect("sender"),
                message_id: None,
                requires_ack: false,
                dry_run: false,
                task_id: None,
                error_code: None,
                error_message: None,
            })
            .expect("emit");

        let command = LogCommand {
            mode: LogModeCommand::Snapshot(QueryArgs {
                levels: vec![CliLogLevel::Info],
                matches: vec!["command=send".to_string()],
                since: None,
                limit: Some(5),
                json: true,
            }),
        };

        command.run(&observability).expect("snapshot run");
    }

    #[test]
    #[serial(env)]
    fn run_snapshot_fails_without_caller_context() {
        let tempdir = TempDir::new().expect("tempdir");
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", None),
            ("ATM_TEAM", Some(TEST_TEAM)),
            ("ATM_LOG", Some("info")),
        ]);
        let observability =
            CliObservability::new(tempdir.path(), CliObservabilityOptions::default())
                .expect("observability");
        let command = LogCommand {
            mode: LogModeCommand::Snapshot(QueryArgs {
                levels: vec![CliLogLevel::Info],
                matches: vec!["command=send".to_string()],
                since: None,
                limit: Some(5),
                json: true,
            }),
        };

        let error = command.run(&observability).expect_err("missing identity");

        assert_eq!(
            error.downcast_ref::<AtmError>().map(|atm| atm.code()),
            Some(atm_core::error_codes::AtmErrorCode::IdentityUnavailable)
        );
    }
}
