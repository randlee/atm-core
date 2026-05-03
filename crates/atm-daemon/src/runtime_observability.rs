use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use atm_core::doctor::{DoctorReport, DoctorSeverity, DoctorStatus};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState, CommandEvent,
    LogTailSession, ObservabilityPort,
};

#[derive(Debug)]
pub(crate) struct DaemonObservability {
    active_log_path: PathBuf,
    fault_mode: Option<String>,
}

impl DaemonObservability {
    pub(crate) fn new(home_dir: &Path) -> Self {
        Self {
            // TODO(phase-q): replace this direct JSONL sink with the shared sc-observability retained sink once the daemon runtime adapter lands.
            active_log_path: home_dir
                .join(".local")
                .join("share")
                .join("logs")
                .join("atm.log.jsonl"),
            fault_mode: std::env::var("ATM_OBSERVABILITY_RETAINED_SINK_FAULT").ok(),
        }
    }
}

impl atm_core::observability::sealed::Sealed for DaemonObservability {}

impl ObservabilityPort for DaemonObservability {
    fn emit(&self, event: CommandEvent) -> Result<(), AtmError> {
        if let Some(parent) = self.active_log_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                AtmError::observability_emit(format!(
                    "failed to create daemon observability directory {}: {error}",
                    parent.display()
                ))
                .with_source(error)
            })?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.active_log_path)
            .map_err(|error| {
                AtmError::observability_emit(format!(
                    "failed to open daemon observability log {}: {error}",
                    self.active_log_path.display()
                ))
                .with_source(error)
            })?;
        let payload = serde_json::json!({
            "command": event.command,
            "action": event.action,
            "outcome": event.outcome,
            "team": event.team.as_str(),
            "agent": event.agent.as_str(),
            "sender": event.sender,
            "message_id": event.message_id.map(|value| value.to_string()),
            "requires_ack": event.requires_ack,
            "dry_run": event.dry_run,
            "task_id": event.task_id.map(|value| value.to_string()),
            "error_code": event.error_code.map(|value| value.to_string()),
            "error_message": event.error_message,
        });
        serde_json::to_writer(&mut file, &payload).map_err(|error| {
            AtmError::observability_emit("failed to serialize daemon observability event")
                .with_source(error)
        })?;
        file.write_all(b"\n").map_err(|error| {
            AtmError::observability_emit("failed to append daemon observability newline")
                .with_source(error)
        })?;
        file.flush().map_err(|error| {
            AtmError::observability_emit("failed to flush daemon observability event")
                .with_source(error)
        })
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        Ok(AtmLogSnapshot::default())
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        Ok(LogTailSession::empty())
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        let (logging_state, detail) = match self.fault_mode.as_deref() {
            Some("degraded") => (AtmObservabilityHealthState::Degraded, None),
            Some("unavailable") => (AtmObservabilityHealthState::Unavailable, None),
            _ => (AtmObservabilityHealthState::Healthy, None),
        };
        Ok(AtmObservabilityHealth {
            active_log_path: Some(self.active_log_path.clone()),
            logging_state,
            query_state: Some(AtmObservabilityHealthState::Healthy),
            detail,
        })
    }
}

pub(crate) fn normalize_doctor_report_observability(
    mut report: DoctorReport,
    observability: &dyn ObservabilityPort,
) -> DoctorReport {
    let (health, finding) = match observability.health() {
        Ok(health) => {
            let finding = atm_core::doctor::health::observability_finding(&health);
            (health, finding)
        }
        Err(error) => {
            let health = atm_core::doctor::health::unavailable_snapshot(error.to_string());
            let finding = atm_core::doctor::health::observability_finding_from_error(&error);
            (health, finding)
        }
    };

    report.findings.retain(|finding| {
        !matches!(
            finding.code,
            AtmErrorCode::ObservabilityHealthOk
                | AtmErrorCode::WarningObservabilityHealthDegraded
                | AtmErrorCode::ObservabilityHealthFailed
        )
    });
    report.findings.push(finding);
    report.recommendations = report
        .findings
        .iter()
        .filter_map(|finding| finding.remediation.clone())
        .collect();
    report.observability = health;
    refresh_doctor_summary(&mut report);
    report
}

fn refresh_doctor_summary(report: &mut DoctorReport) {
    let (info_count, warning_count, error_count) = report.findings.iter().fold(
        (0usize, 0usize, 0usize),
        |(info, warning, error), finding| match finding.severity {
            DoctorSeverity::Info => (info + 1, warning, error),
            DoctorSeverity::Warning => (info, warning + 1, error),
            DoctorSeverity::Error => (info, warning, error + 1),
        },
    );
    let status = if error_count > 0 {
        DoctorStatus::Error
    } else if warning_count > 0 {
        DoctorStatus::Warning
    } else {
        DoctorStatus::Healthy
    };
    let message = match status {
        DoctorStatus::Healthy => "ATM doctor completed with healthy findings only",
        DoctorStatus::Warning => "ATM doctor completed with warnings",
        DoctorStatus::Error => "ATM doctor found critical issues",
    };
    report.summary.status = status;
    report.summary.message = message.to_string();
    report.summary.info_count = info_count;
    report.summary.warning_count = warning_count;
    report.summary.error_count = error_count;
}
