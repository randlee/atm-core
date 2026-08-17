use std::path::PathBuf;

use crate::caller_context::{read_cli_identity_from_env_or_warn, read_cli_team_from_env_or_warn};
use crate::doctor::report::{
    DoctorEnvironmentVisibility, DoctorFinding, DoctorSeverity, DoctorStatus,
};
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::observability::{AtmObservabilityHealth, AtmObservabilityHealthState};
use crate::types::{AgentName, TeamName};

pub fn unavailable_snapshot(detail: String) -> AtmObservabilityHealth {
    AtmObservabilityHealth {
        active_log_path: None,
        logging_state: AtmObservabilityHealthState::Unavailable,
        query_state: Some(AtmObservabilityHealthState::Unavailable),
        maintenance: None,
        diagnostic: None,
        detail: Some(detail),
    }
}

/// Assemble the caller-facing environment visibility for a doctor report.
///
/// The `caller_team`/`caller_identity` arguments carry the invoking CLI
/// process's resolved `ATM_TEAM`/`ATM_IDENTITY`. They take precedence over
/// reading the process environment directly, which is essential when this code
/// runs inside the long-lived daemon whose own environment is frozen at launch
/// time and does not reflect the requesting shell. The direct-read fallback is
/// only used for the in-process direct-local doctor path, where the current
/// process environment already is the caller's.
pub fn environment_visibility(
    home_dir: PathBuf,
    team_override: Option<TeamName>,
    caller_team: Option<TeamName>,
    caller_identity: Option<AgentName>,
) -> DoctorEnvironmentVisibility {
    DoctorEnvironmentVisibility {
        // `atm_home` intentionally continues to reflect this process env
        // directly; re-plumbing ATM_HOME precedence is out of scope for #548.
        atm_home: std::env::var_os("ATM_HOME")
            .map(PathBuf::from)
            .or(Some(home_dir)),
        atm_team: caller_team
            .or_else(|| read_cli_team_from_env_or_warn("atm_core::doctor::environment_visibility")),
        atm_identity: caller_identity.or_else(|| {
            read_cli_identity_from_env_or_warn("atm_core::doctor::environment_visibility")
        }),
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
        code: error.code(),
        message: format!("shared observability health check failed: {error}"),
        remediation: Some(error.message().to_owned()),
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::environment_visibility;
    use crate::test_support::{EnvGuard, TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, TeamName};

    // Synthetic stand-ins for the daemon's frozen launch-time environment.
    // Deliberately distinct from the caller's values so the test proves the
    // caller wins even when the ambient process env disagrees (issue #548).
    const DAEMON_LAUNCH_TEAM: &str = "daemon-launch-team";
    const DAEMON_LAUNCH_IDENTITY: &str = "daemon-launch-identity";

    #[test]
    #[serial_test::serial(env)]
    fn caller_values_win_over_process_environment() {
        // Simulate the daemon process whose env was frozen at launch time.
        let _env = EnvGuard::set_many([
            ("ATM_TEAM", Some(DAEMON_LAUNCH_TEAM)),
            ("ATM_IDENTITY", Some(DAEMON_LAUNCH_IDENTITY)),
        ]);

        let visibility = environment_visibility(
            PathBuf::from("/home"),
            None,
            Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            Some(TEST_SENDER.parse::<AgentName>().expect("identity")),
        );

        assert_eq!(
            visibility.atm_team.as_ref().map(TeamName::as_str),
            Some(TEST_TEAM)
        );
        assert_eq!(
            visibility.atm_identity.as_ref().map(AgentName::as_str),
            Some(TEST_SENDER)
        );
    }

    #[test]
    #[serial_test::serial(env)]
    fn falls_back_to_process_environment_when_caller_absent() {
        let _env = EnvGuard::set_many([
            ("ATM_TEAM", Some(TEST_TEAM)),
            ("ATM_IDENTITY", Some(TEST_SENDER)),
        ]);

        let visibility = environment_visibility(PathBuf::from("/home"), None, None, None);

        assert_eq!(
            visibility.atm_team.as_ref().map(TeamName::as_str),
            Some(TEST_TEAM)
        );
        assert_eq!(
            visibility.atm_identity.as_ref().map(AgentName::as_str),
            Some(TEST_SENDER)
        );
    }
}
