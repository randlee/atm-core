use anyhow::{Context, Result};
use atm_core::log::{self, LogFieldFilter, LogLevel, LogQuery};
use atm_core::observability::ObservabilityPort;
use clap::{Args, ValueEnum};

use crate::output;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum LogLevelArg {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<LogLevelArg> for LogLevel {
    fn from(value: LogLevelArg) -> Self {
        match value {
            LogLevelArg::Trace => LogLevel::Trace,
            LogLevelArg::Debug => LogLevel::Debug,
            LogLevelArg::Info => LogLevel::Info,
            LogLevelArg::Warn => LogLevel::Warn,
            LogLevelArg::Error => LogLevel::Error,
        }
    }
}

#[derive(Debug, Args)]
pub struct LogCommand {
    #[arg(long)]
    level: Option<LogLevelArg>,

    #[arg(long = "filter")]
    filters: Vec<String>,

    #[arg(long = "follow")]
    follow: bool,

    #[arg(long)]
    json: bool,
}

impl LogCommand {
    pub fn run(self, observability: &dyn ObservabilityPort) -> Result<()> {
        let query = LogQuery {
            level: self.level.map(Into::into),
            filters: self
                .filters
                .iter()
                .map(|raw| parse_filter(raw))
                .collect::<Result<Vec<_>>>()?,
            follow: self.follow,
        };

        if self.follow {
            let mut session = log::follow_logs(query, observability)?;
            while let Some(record) = session.next_record()? {
                output::print_log_record(&record, self.json)?;
            }
            return Ok(());
        }

        let result = log::query_logs(query, observability)?;
        output::print_log_result(&result, self.json)
    }
}

fn parse_filter(raw: &str) -> Result<LogFieldFilter> {
    let (key, value) = raw
        .split_once('=')
        .with_context(|| format!("invalid filter '{raw}'; expected key=value"))?;
    if key.trim().is_empty() {
        anyhow::bail!("invalid filter '{raw}'; key must not be empty");
    }
    if value.is_empty() {
        anyhow::bail!("invalid filter '{raw}'; value must not be empty");
    }
    Ok(LogFieldFilter {
        key: key.trim().to_string(),
        value: value.to_string(),
    })
}
