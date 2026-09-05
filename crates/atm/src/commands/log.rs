use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use atm_core::observability::{
    AtmJsonNumber, AtmLogQuery, LogFieldKey, LogFieldMatch, LogFieldValue, LogLevelFilter, LogMode,
    LogOrder, ObservabilityPort,
};
use atm_core::observability_counters::{DiagnosticTimelineRecord, DiagnosticTimelineResponse};
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
const MERGED_LOSS_NOTE: &str =
    "sources are independently bounded; merged view is not lossless under overload";

#[derive(Debug, Args)]
/// Query or follow ATM retained observability records.
pub struct LogCommand {
    #[command(subcommand)]
    mode: LogModeCommand,
}

impl LogCommand {
    /// Execute the `atm log` command.
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        let _caller_context = resolve_cli_caller_context(CallerContextOverrides::default())?;
        match self.mode {
            LogModeCommand::Snapshot(args) => {
                if args.source != LogSource::Jsonl {
                    return args.run_timeline(observability).await;
                }
                let snapshot = observability.query(args.build_query(LogMode::Snapshot)?)?;
                output::print_log_snapshot(&snapshot, args.json)
            }
            LogModeCommand::Filter(args) => {
                args.ensure_filter_present()?;
                if args.source != LogSource::Jsonl {
                    return args.run_timeline(observability).await;
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
    async fn run_timeline(&self, observability: &CliObservability) -> Result<()> {
        let records = self.records_for_source(observability).await?;
        print!("{}", render_timeline_output(records, self.json)?);
        Ok(())
    }

    async fn records_for_source(
        &self,
        observability: &CliObservability,
    ) -> Result<Vec<TimelineRecord>> {
        let endpoint = atm_core::home::host_runtime_dir()?
            .join(atm_core::local_http::LOCAL_HTTP_RECORD_FILENAME);
        self.records_for_source_from_timeline_endpoint(observability, &endpoint)
            .await
    }

    async fn records_for_source_from_timeline_endpoint(
        &self,
        observability: &CliObservability,
        endpoint: &Path,
    ) -> Result<Vec<TimelineRecord>> {
        if self.source == LogSource::Timeline {
            return self.timeline_records_from_endpoint(endpoint).await;
        }
        let limit = self.limit.unwrap_or(DEFAULT_SNAPSHOT_LIMIT).min(5_000);
        let mut records = self.jsonl_records(observability)?;
        records.extend(self.graft_fallback_records()?);
        records.extend(self.timeline_records_from_endpoint(endpoint).await?);
        records.sort_by_key(|record| (record.ts_unix_ms, record.source_rank(), record.seq));
        records.truncate(limit);
        Ok(records)
    }

    fn jsonl_records(&self, observability: &CliObservability) -> Result<Vec<TimelineRecord>> {
        let snapshot = observability.query(self.build_query(LogMode::Snapshot)?)?;
        Ok(snapshot
            .records
            .into_iter()
            .enumerate()
            .filter(|(_, record)| {
                self.component.as_ref().is_none_or(|component| {
                    record
                        .target
                        .as_ref()
                        .is_some_and(|target| target.starts_with(component))
                })
            })
            .map(|(seq, record)| TimelineRecord::from_jsonl(record, seq))
            .collect())
    }

    fn graft_fallback_records(&self) -> Result<Vec<TimelineRecord>> {
        let since = self.since.as_deref().map(parse_since_millis).transpose()?;
        let until = self.until.as_deref().map(parse_since_millis).transpose()?;
        let path = atm_observability::graft_fallback_log_path(&atm_core::home::host_log_dir()?);
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error).context("read graft fallback retained diagnostics"),
        };
        Ok(contents
            .lines()
            .enumerate()
            .filter_map(|(seq, line)| {
                serde_json::from_str::<serde_json::Value>(line)
                    .ok()
                    .and_then(|value| TimelineRecord::from_graft_value(value, seq))
            })
            .filter(|record| self.record_matches(record, since, until))
            .take(self.limit.unwrap_or(DEFAULT_SNAPSHOT_LIMIT).min(5_000))
            .collect())
    }

    async fn timeline_records_from_endpoint(&self, endpoint: &Path) -> Result<Vec<TimelineRecord>> {
        let mut query = vec![format!(
            "limit={}",
            self.limit.unwrap_or(DEFAULT_SNAPSHOT_LIMIT).min(5_000)
        )];
        if let Some(since) = self.since.as_deref() {
            query.push(format!("since={}", parse_since_millis(since)?));
        }
        if let Some(until) = self.until.as_deref() {
            query.push(format!("until={}", parse_since_millis(until)?));
        }
        if let Some(component) = self.component.as_deref() {
            query.push(format!("component={}", percent_encode(component)));
        }
        let body = atm_http_runtime::loopback_tcp_get_json(
            endpoint,
            format!("/v1/diagnostics?{}", query.join("&")),
            Duration::from_secs(10),
        )
        .await?;
        let response: DiagnosticTimelineResponse = serde_json::from_slice(&body)
            .context("daemon returned an invalid diagnostic timeline response")?;
        Ok(response
            .records
            .into_iter()
            .filter(|record| {
                self.levels.is_empty()
                    || self
                        .levels
                        .iter()
                        .any(|level| level_name(*level) == record.level)
            })
            .map(|record| TimelineRecord::from_record(record, self.source))
            .collect())
    }

    fn record_matches(
        &self,
        record: &TimelineRecord,
        since: Option<i64>,
        until: Option<i64>,
    ) -> bool {
        self.component
            .as_ref()
            .is_none_or(|component| record.component.starts_with(component))
            && (self.levels.is_empty()
                || self
                    .levels
                    .iter()
                    .any(|level| level_name(*level) == record.level))
            && since.is_none_or(|timestamp| record.ts_unix_ms >= timestamp)
            && until.is_none_or(|timestamp| record.ts_unix_ms <= timestamp)
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

fn render_timeline_output(records: Vec<TimelineRecord>, json: bool) -> Result<String> {
    if json {
        return Ok(format!(
            "{}\n",
            serde_json::to_string_pretty(&TimelineOutput {
                note: MERGED_LOSS_NOTE,
                records,
            })?
        ));
    }

    let mut output = format!("Note: {MERGED_LOSS_NOTE}\n");
    for event in records {
        output.push_str(&format!(
            "{} {} {} {} {}\n",
            event.ts_unix_ms, event.source, event.level, event.component, event.message
        ));
    }
    Ok(output)
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
    #[serde(skip)]
    seq: usize,
}

impl TimelineRecord {
    fn from_record(event: DiagnosticTimelineRecord, source: LogSource) -> Self {
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
            seq: 0,
        }
    }

    fn from_jsonl(event: atm_core::observability::AtmLogRecord, seq: usize) -> Self {
        let fields = serde_json::to_value(&event.fields).unwrap_or_default();
        Self {
            source: "jsonl",
            ts_unix_ms: event.timestamp.into_inner().timestamp_millis(),
            level: serde_json::to_value(event.level)
                .ok()
                .and_then(|value| value.as_str().map(ToOwned::to_owned))
                .unwrap_or_else(|| "info".to_owned()),
            component: event.target.unwrap_or_else(|| event.service.to_string()),
            code: fields
                .get("code")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            correlation_id: fields
                .get("correlation_id")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            origin: fields
                .get("origin")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("jsonl")
                .to_owned(),
            message: event.message.unwrap_or_default(),
            seq,
        }
    }

    fn from_graft_value(value: serde_json::Value, seq: usize) -> Option<Self> {
        let object = value.as_object()?;
        Some(Self {
            source: "graft",
            ts_unix_ms: object.get("ts_unix_ms")?.as_i64()?,
            level: object.get("level")?.as_str()?.to_owned(),
            component: object.get("component")?.as_str()?.to_owned(),
            code: object
                .get("code")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            correlation_id: object
                .get("correlation_id")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            origin: object
                .get("origin")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("graft")
                .to_owned(),
            message: object.get("message")?.as_str()?.to_owned(),
            seq,
        })
    }

    fn source_rank(&self) -> u8 {
        match self.source {
            "jsonl" => 0,
            "graft" => 1,
            "timeline" => 2,
            _ => unreachable!("merged source is constrained"),
        }
    }
}

fn level_name(level: CliLogLevel) -> &'static str {
    match level {
        CliLogLevel::Trace => "trace",
        CliLogLevel::Debug => "debug",
        CliLogLevel::Info => "info",
        CliLogLevel::Warn => "warn",
        CliLogLevel::Error => "error",
    }
}

fn percent_encode(raw: &str) -> String {
    raw.bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                vec![char::from(byte)]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
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
    use std::path::PathBuf;
    use std::sync::Mutex;

    use atm_core::error::AtmError;
    use atm_core::local_http::{LocalCapability, LocalHttpEndpointRecord};
    use atm_core::observability::{
        AtmLogRecord, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
        CommandEvent, LogTailSession, ObservabilityPort,
    };
    use atm_core::observability::{LogFieldValue, LogLevelFilter, LogMode};
    use atm_core::observability_counters::{DiagnosticTimelineRecord, DiagnosticTimelineResponse};
    use atm_core::test_support::{EnvGuard, TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
    use serial_test::serial;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{
        CliLogLevel, LogCommand, LogModeCommand, LogSource, QueryArgs, TimelineRecord,
        parse_match_expression, parse_relative_duration,
    };
    use crate::observability::{CliObservability, CliObservabilityOptions};

    fn caller_context_env() -> EnvGuard {
        EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ])
    }

    async fn timeline_fixture_endpoint(
        response: DiagnosticTimelineResponse,
    ) -> (TempDir, PathBuf, tokio::task::JoinHandle<String>) {
        let fixture = TempDir::new().expect("timeline fixture directory");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("timeline fixture listener");
        let capability = LocalCapability::generate().expect("fixture capability");
        let instance_id = ulid::Ulid::new();
        let endpoint = fixture.path().join("local-http.json");
        std::fs::write(
            fixture
                .path()
                .join(atm_core::home::HOST_RUNTIME_OWNER_LOCK_FILE),
            format!("1:fixture:{instance_id}"),
        )
        .expect("fixture owner record");
        std::fs::write(
            &endpoint,
            serde_json::to_vec(&LocalHttpEndpointRecord::active(
                instance_id,
                Some(listener.local_addr().expect("fixture address")),
                None,
                &capability,
            ))
            .expect("fixture endpoint record"),
        )
        .expect("write fixture endpoint record");

        let body = serde_json::to_vec(&response).expect("fixture response");
        let expected_capability = capability.to_base64url();
        let request = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("fixture connection");
            let mut bytes = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).await.expect("read fixture request");
                assert_ne!(count, 0, "fixture client closed before sending headers");
                bytes.extend_from_slice(&chunk[..count]);
            }
            let request = String::from_utf8(bytes).expect("UTF-8 fixture request");
            assert!(request.starts_with("GET /v1/diagnostics?limit=50 HTTP/1.1\r\n"));
            assert!(request.to_ascii_lowercase().contains(
                &format!("x-atm-local-capability: {}", expected_capability).to_ascii_lowercase()
            ));
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .expect("write fixture headers");
            stream.write_all(&body).await.expect("write fixture body");
            request
        });
        (fixture, endpoint, request)
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
                jsonl: Default::default(),
                timeline: Default::default(),
                degraded: Vec::new(),
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
    fn merged_records_sort_by_timestamp_source_then_sequence() {
        let mut records = [
            TimelineRecord {
                source: "timeline",
                ts_unix_ms: 100,
                level: "info".to_owned(),
                component: "daemon".to_owned(),
                code: None,
                correlation_id: None,
                origin: "daemon".to_owned(),
                message: "timeline".to_owned(),
                seq: 0,
            },
            TimelineRecord {
                source: "graft",
                ts_unix_ms: 100,
                level: "info".to_owned(),
                component: "graft".to_owned(),
                code: None,
                correlation_id: None,
                origin: "graft".to_owned(),
                message: "graft".to_owned(),
                seq: 0,
            },
            TimelineRecord {
                source: "jsonl",
                ts_unix_ms: 100,
                level: "info".to_owned(),
                component: "cli".to_owned(),
                code: None,
                correlation_id: None,
                origin: "cli".to_owned(),
                message: "jsonl".to_owned(),
                seq: 0,
            },
            TimelineRecord {
                source: "jsonl",
                ts_unix_ms: 100,
                level: "info".to_owned(),
                component: "cli".to_owned(),
                code: None,
                correlation_id: None,
                origin: "cli".to_owned(),
                message: "later-jsonl".to_owned(),
                seq: 1,
            },
        ];
        records.sort_by_key(|record| (record.ts_unix_ms, record.source_rank(), record.seq));
        assert_eq!(
            records
                .iter()
                .map(|record| record.message.as_str())
                .collect::<Vec<_>>(),
            ["jsonl", "later-jsonl", "graft", "timeline"]
        );
    }

    #[test]
    fn graft_fallback_record_keeps_its_source_label() {
        let record = TimelineRecord::from_graft_value(
            serde_json::json!({
                "ts_unix_ms": 42,
                "level": "warn",
                "component": "atm_graft",
                "origin": "graft",
                "message": "fallback retained",
            }),
            3,
        )
        .expect("valid graft event");
        assert_eq!(record.source, "graft");
        assert_eq!(record.seq, 3);
    }

    #[test]
    fn merged_graft_records_honor_shared_time_bounds() {
        let args = QueryArgs {
            source: LogSource::Merged,
            levels: Vec::new(),
            matches: Vec::new(),
            since: None,
            until: None,
            component: None,
            limit: None,
            json: false,
        };
        let record = TimelineRecord::from_graft_value(
            serde_json::json!({
                "ts_unix_ms": 42,
                "level": "info",
                "component": "atm_graft",
                "message": "bounded fallback",
            }),
            0,
        )
        .expect("valid graft event");

        assert!(args.record_matches(&record, Some(42), Some(42)));
        assert!(!args.record_matches(&record, Some(43), None));
        assert!(!args.record_matches(&record, None, Some(41)));
    }

    #[test]
    fn merged_output_carries_the_not_lossless_note_in_json_and_text_contracts() {
        let json = super::render_timeline_output(Vec::new(), true).expect("JSON merged output");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse JSON output");
        assert_eq!(value["note"], super::MERGED_LOSS_NOTE);

        let text = super::render_timeline_output(Vec::new(), false).expect("text merged output");
        assert_eq!(text, format!("Note: {}\n", super::MERGED_LOSS_NOTE));
    }

    #[tokio::test]
    #[serial(env)]
    async fn merged_source_reads_interleaved_jsonl_graft_and_timeline_fixture_records() {
        let log_dir = TempDir::new().expect("graft log directory");
        let _env = EnvGuard::set_many([(
            "ATM_LOG_DIR",
            Some(log_dir.path().to_str().expect("UTF-8 log directory")),
        )]);
        std::fs::write(
            atm_observability::graft_fallback_log_path(log_dir.path()),
            concat!(
                r#"{"ts_unix_ms":100,"level":"warn","component":"graft.receiver","origin":"graft","message":"graft fixture"}"#,
                "\n"
            ),
        )
        .expect("write graft fixture");
        let observability = CliObservability::from_test_port(StubObservability {
            snapshot: Mutex::new(Some(Ok(AtmLogSnapshot {
                records: vec![AtmLogRecord {
                    timestamp: chrono::DateTime::from_timestamp_millis(100)
                        .expect("fixture timestamp")
                        .into(),
                    level: LogLevelFilter::Info,
                    service: atm_core::observability::service_name("atm").expect("fixture service"),
                    target: Some("cli.command".to_owned()),
                    action: None,
                    message: Some("jsonl fixture".to_owned()),
                    fields: Default::default(),
                }],
                truncated: false,
            }))),
        });
        let (fixture, endpoint, request) = timeline_fixture_endpoint(DiagnosticTimelineResponse {
            records: vec![DiagnosticTimelineRecord {
                ts_unix_ms: 100,
                level: "error".to_owned(),
                component: "runtime.route".to_owned(),
                code: Some("fixture".to_owned()),
                correlation_id: None,
                origin: "timeline".to_owned(),
                message: "timeline fixture".to_owned(),
                detail: None,
            }],
        })
        .await;
        let args = QueryArgs {
            source: LogSource::Merged,
            levels: Vec::new(),
            matches: Vec::new(),
            since: None,
            until: None,
            component: None,
            limit: None,
            json: true,
        };

        let records = args
            .records_for_source_from_timeline_endpoint(&observability, &endpoint)
            .await
            .expect("merged fixture query");

        assert_eq!(
            records
                .iter()
                .map(|record| (record.source, record.message.as_str()))
                .collect::<Vec<_>>(),
            [
                ("jsonl", "jsonl fixture"),
                ("graft", "graft fixture"),
                ("timeline", "timeline fixture"),
            ]
        );
        request.await.expect("fixture request task");
        drop(fixture);
    }

    #[test]
    fn snapshot_query_defaults_limit_and_orders_newest_first() {
        let args = QueryArgs {
            source: LogSource::Jsonl,
            levels: vec![CliLogLevel::Info],
            matches: vec!["command=send".to_string()],
            since: None,
            until: None,
            component: None,
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
            source: LogSource::Jsonl,
            levels: vec![],
            matches: vec!["success=true".to_string()],
            since: Some("15m".to_string()),
            until: None,
            component: None,
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
            source: LogSource::Jsonl,
            levels: vec![],
            matches: vec![],
            since: None,
            until: None,
            component: None,
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

    #[tokio::test]
    #[serial(env)]
    async fn run_snapshot_succeeds_with_fake_observability_snapshot() {
        let _caller = caller_context_env();
        let command = LogCommand {
            mode: LogModeCommand::Snapshot(QueryArgs {
                source: LogSource::Jsonl,
                levels: vec![],
                matches: vec![],
                since: None,
                until: None,
                component: None,
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

        command.run(&observability).await.expect("snapshot run");
    }

    #[tokio::test]
    #[serial(env)]
    async fn run_snapshot_surfaces_observability_query_error() {
        let _caller = caller_context_env();
        let command = LogCommand {
            mode: LogModeCommand::Snapshot(QueryArgs {
                source: LogSource::Jsonl,
                levels: vec![],
                matches: vec![],
                since: None,
                until: None,
                component: None,
                limit: None,
                json: false,
            }),
        };
        let observability = CliObservability::from_test_port(StubObservability {
            snapshot: Mutex::new(Some(Err(AtmError::observability_query(
                "synthetic snapshot failure",
            )))),
        });

        let error = command.run(&observability).await.expect_err("query error");

        assert_eq!(
            error.downcast_ref::<AtmError>().map(|atm| atm.code()),
            Some(atm_core::error_codes::AtmErrorCode::ObservabilityQueryFailed)
        );
    }

    #[tokio::test]
    #[serial(env)]
    async fn run_snapshot_reads_real_retained_log_without_daemon() {
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
                source: LogSource::Jsonl,
                levels: vec![CliLogLevel::Info],
                matches: vec!["command=send".to_string()],
                since: None,
                until: None,
                component: None,
                limit: Some(5),
                json: true,
            }),
        };

        command.run(&observability).await.expect("snapshot run");
    }

    #[test]
    fn default_jsonl_snapshot_matches_the_pre_aw3_golden_bytes() {
        let snapshot = AtmLogSnapshot {
            records: vec![AtmLogRecord {
                timestamp: chrono::DateTime::from_timestamp_millis(100)
                    .expect("fixture timestamp")
                    .into(),
                level: LogLevelFilter::Info,
                service: atm_core::observability::service_name("atm").expect("fixture service"),
                target: Some("cli.command".to_owned()),
                action: Some("send".to_owned()),
                message: Some("jsonl fixture".to_owned()),
                fields: Default::default(),
            }],
            truncated: false,
        };
        let rendered = format!(
            "{}\n",
            serde_json::to_string_pretty(&snapshot).expect("serialize retained snapshot")
        );
        // The retained snapshot contract is byte-exact in its canonical LF
        // form. Git may materialize the checked-in fixture as CRLF on Windows.
        let golden = include_str!("../../tests/fixtures/log-jsonl-default.json").replace('\r', "");
        assert_eq!(rendered.as_bytes(), golden.as_bytes());
    }

    #[tokio::test]
    #[serial(env)]
    async fn run_snapshot_fails_without_caller_context() {
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
                source: LogSource::Jsonl,
                levels: vec![CliLogLevel::Info],
                matches: vec!["command=send".to_string()],
                since: None,
                until: None,
                component: None,
                limit: Some(5),
                json: true,
            }),
        };

        let error = command
            .run(&observability)
            .await
            .expect_err("missing identity");

        assert_eq!(
            error.downcast_ref::<AtmError>().map(|atm| atm.code()),
            Some(atm_core::error_codes::AtmErrorCode::IdentityUnavailable)
        );
    }
}
