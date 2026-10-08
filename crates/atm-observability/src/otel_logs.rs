//! The retained logger's sink boundary mapped directly onto native SDK logs.

use opentelemetry::logs::{LogRecord, Logger, Severity};
use opentelemetry_sdk::logs::SdkLogger;
use sc_observability::v2::LogSink;
use sc_observability_types::v2::LogSinkError;
use sc_observability_types::{Level, LogEvent, SinkHealth, SinkHealthState, SinkName};

/// Receives already-filtered and redacted retained events. This sink owns no
/// provider lifecycle: dropping or flushing it never shuts down or flushes the
/// caller's provider. `emit` is SDK admission, not a collector acknowledgement.
pub struct OtelLogSink(pub SdkLogger);

impl LogSink for OtelLogSink {
    fn write(&self, event: &LogEvent) -> Result<(), LogSinkError> {
        if is_sdk_target(event.target.as_str()) {
            return Ok(());
        }
        let mut record = self.0.create_log_record();
        record.set_timestamp(event.timestamp.into_inner().into());
        record.set_severity_number(match event.level {
            Level::Trace => Severity::Trace,
            Level::Debug => Severity::Debug,
            Level::Info => Severity::Info,
            Level::Warn => Severity::Warn,
            Level::Error => Severity::Error,
        });
        record.set_target(event.target.as_str().to_owned());
        record.set_body(
            event
                .message
                .clone()
                .unwrap_or_else(|| event.action.to_string())
                .into(),
        );
        record.add_attribute("atm.action", event.action.to_string());
        if let Some(outcome) = &event.outcome {
            record.add_attribute("atm.outcome", outcome.to_string());
        }
        // Use the same ATM allowlist for direct logger/macros and tracing input.
        // The logger has already applied its redaction policy before fan-out.
        for (key, value) in crate::sanitize_retained_fields(event.fields.clone()) {
            let value = match value {
                serde_json::Value::String(value) => value,
                value => value.to_string(),
            };
            record.add_attribute(key, value);
        }
        self.0.emit(record);
        Ok(())
    }

    fn health(&self) -> SinkHealth {
        SinkHealth {
            name: SinkName::new("otel").expect("literal sink name"),
            // This reports sink admission only. SDK transport evidence is
            // projected separately by ExportDiagnostics, including unknown loss.
            state: SinkHealthState::Healthy,
            last_error: None,
        }
    }
}

pub(crate) fn is_sdk_target(target: &str) -> bool {
    target.starts_with("opentelemetry") || target.starts_with("tonic")
}
