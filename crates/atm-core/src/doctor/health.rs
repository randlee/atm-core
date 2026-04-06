use std::path::PathBuf;

use crate::doctor::report::{
    DoctorEnvironmentVisibility, DoctorFinding, DoctorSeverity, DoctorStatus,
};
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::observability::{AtmObservabilityHealth, AtmObservabilityHealthState};

pub fn unavailable_snapshot(detail: String) -> AtmObservabilityHealth {
    AtmObservabilityHealth {
        active_log_path: None,
        logging_state: AtmObservabilityHealthState::Unavailable,
        query_state: Some(AtmObservabilityHealthState::Unavailable),
        detail: Some(detail),
    }
}

pub fn environment_visibility(
    home_dir: PathBuf,
    team_override: Option<String>,
) -> DoctorEnvironmentVisibility {
    DoctorEnvironmentVisibility {
        atm_home: std::env::var_os("ATM_HOME")
            .map(PathBuf::from)
            .or(Some(home_dir)),
        atm_team: std::env::var("ATM_TEAM")
            .ok()
            .filter(|value| !value.is_empty()),
        atm_identity: std::env::var("ATM_IDENTITY")
            .ok()
            .filter(|value| !value.is_empty()),
        team_override,
    }
}

pub fn observability_finding(health: &AtmObservabilityHealth) -> DoctorFinding {
    let path = health
        .active_log_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<unavailable>".to_string());
    let detail = health
        .detail
        .as_ref()
        .map(|detail| format!(" Detail: {detail}"))
        .unwrap_or_default();
    let query_state = health.query_state.map(render_state).unwrap_or("unknown");

    match health.logging_state {
        AtmObservabilityHealthState::Healthy => DoctorFinding {
            severity: DoctorSeverity::Info,
            code: AtmErrorCode::ObservabilityHealthOk,
            message: format!(
                "shared observability active at {path}; logging health is healthy and query readiness is {query_state}.{detail}"
            ),
            remediation: None,
        },
        AtmObservabilityHealthState::Degraded => DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::WarningObservabilityHealthDegraded,
            message: format!(
                "shared observability is degraded at {path}; logging health is degraded and query readiness is {query_state}.{detail}"
            ),
            remediation: Some(
                "Inspect the shared log store and query path, then re-run `atm doctor`."
                    .to_string(),
            ),
        },
        AtmObservabilityHealthState::Unavailable => DoctorFinding {
            severity: DoctorSeverity::Error,
            code: AtmErrorCode::ObservabilityHealthFailed,
            message: format!(
                "shared observability is unavailable; active log path is {path} and query readiness is {query_state}.{detail}"
            ),
            remediation: Some(
                "Restore shared observability initialization and confirm the active log path is writable."
                    .to_string(),
            ),
        },
    }
}

pub fn observability_finding_from_error(error: &AtmError) -> DoctorFinding {
    DoctorFinding {
        severity: DoctorSeverity::Error,
        code: error.code,
        message: format!("shared observability health check failed: {error}"),
        remediation: error.recovery.clone().or(Some(
            "Restore shared observability initialization and re-run `atm doctor`.".to_string(),
        )),
    }
}

pub fn status_from_findings(findings: &[DoctorFinding]) -> DoctorStatus {
    if findings
        .iter()
        .any(|finding| finding.severity == DoctorSeverity::Error)
    {
        DoctorStatus::Error
    } else if findings
        .iter()
        .any(|finding| finding.severity == DoctorSeverity::Warning)
    {
        DoctorStatus::Warning
    } else {
        DoctorStatus::Healthy
    }
}

fn render_state(state: AtmObservabilityHealthState) -> &'static str {
    match state {
        AtmObservabilityHealthState::Healthy => "healthy",
        AtmObservabilityHealthState::Degraded => "degraded",
        AtmObservabilityHealthState::Unavailable => "unavailable",
    }
}
