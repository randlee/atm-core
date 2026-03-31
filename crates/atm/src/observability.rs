use std::env;
use std::fs;

use anyhow::{Context, Result};
use atm_core::error::AtmError;
use atm_core::log::{filters, LogQuery, LogQueryResult, LogRecord};
use atm_core::observability::{CommandEvent, LogFollowSession, ObservabilityPort};
use serde::Deserialize;
use tracing::info;

#[derive(Debug, Default, Clone)]
pub struct CliObservability {
    backend: ObservabilityBackend,
}

#[derive(Debug, Default, Clone)]
enum ObservabilityBackend {
    #[default]
    Live,
    Test(TestLogFixture),
}

#[derive(Debug, Clone, Deserialize)]
struct TestLogFixture {
    #[serde(default)]
    records: Vec<LogRecord>,
    #[serde(default)]
    follow_records: Vec<LogRecord>,
    #[serde(default)]
    emit_fails: bool,
}

#[derive(Debug)]
struct TestFollowSession {
    records: std::vec::IntoIter<LogRecord>,
}

pub fn init() -> Result<CliObservability> {
    if let Ok(path) = env::var("ATM_TEST_LOG_FIXTURE") {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read ATM_TEST_LOG_FIXTURE at {path}"))?;
        let fixture: TestLogFixture =
            serde_json::from_str(&raw).context("failed to decode ATM test log fixture")?;
        return Ok(CliObservability {
            backend: ObservabilityBackend::Test(fixture),
        });
    }

    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .try_init();
    Ok(CliObservability {
        backend: ObservabilityBackend::Live,
    })
}

impl ObservabilityPort for CliObservability {
    fn emit_command_event(&self, event: CommandEvent) -> Result<(), AtmError> {
        match &self.backend {
            ObservabilityBackend::Test(fixture) if fixture.emit_fails => Err(
                AtmError::observability_emit("test fixture forced emit failure"),
            ),
            ObservabilityBackend::Test(_) | ObservabilityBackend::Live => {
                let message_id = event.message_id.map(|value| value.to_string());
                info!(
                    command = event.command,
                    action = event.action,
                    outcome = event.outcome,
                    team = event.team,
                    agent = event.agent,
                    sender = event.sender,
                    message_id = message_id.as_deref().unwrap_or(""),
                    requires_ack = event.requires_ack,
                    dry_run = event.dry_run,
                    task_id = event.task_id.as_deref().unwrap_or(""),
                    "atm command event"
                );
                Ok(())
            }
        }
    }

    fn query_logs(&self, query: &LogQuery) -> Result<LogQueryResult, AtmError> {
        match &self.backend {
            ObservabilityBackend::Test(fixture) => {
                let mut records = fixture
                    .records
                    .iter()
                    .filter(|record| filters::matches_query(record, query.level, &query.filters))
                    .cloned()
                    .collect::<Vec<_>>();
                records.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
                Ok(LogQueryResult {
                    action: "log",
                    follow: false,
                    records,
                })
            }
            ObservabilityBackend::Live => Err(
                AtmError::observability_query(
                    "shared sc-observability query API is not yet integrated for atm log",
                )
                .with_recovery(
                    "Use the ATM test double for Phase G validation or wait for arch-obs to land the shared query API.",
                ),
            ),
        }
    }

    fn follow_logs(&self, query: &LogQuery) -> Result<Box<dyn LogFollowSession>, AtmError> {
        match &self.backend {
            ObservabilityBackend::Test(fixture) => {
                let source = if fixture.follow_records.is_empty() {
                    &fixture.records
                } else {
                    &fixture.follow_records
                };
                let records = source
                    .iter()
                    .filter(|record| filters::matches_query(record, query.level, &query.filters))
                    .cloned()
                    .collect::<Vec<_>>();
                Ok(Box::new(TestFollowSession {
                    records: records.into_iter(),
                }))
            }
            ObservabilityBackend::Live => Err(
                AtmError::observability_query(
                    "shared sc-observability follow API is not yet integrated for atm log --follow",
                )
                .with_recovery(
                    "Use the ATM test double for Phase G validation or wait for arch-obs to land the shared follow API.",
                ),
            ),
        }
    }
}

impl LogFollowSession for TestFollowSession {
    fn next_record(&mut self) -> Result<Option<LogRecord>, AtmError> {
        Ok(self.records.next())
    }
}
