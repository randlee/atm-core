use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::boundary::{ConfigDoctorReport, MailStoreDoctorReport, RosterStoreDoctorReport};
use crate::error_codes::AtmErrorCode;
use crate::observability::AtmObservabilityHealth;
use crate::protocol::RuntimeStatusSnapshot;
use crate::team_admin::MembersList;
use crate::types::{AgentName, TeamName};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Healthy,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorFinding {
    pub severity: DoctorSeverity,
    pub code: AtmErrorCode,
    pub message: String,
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorSummary {
    pub status: DoctorStatus,
    pub message: String,
    pub info_count: usize,
    pub warning_count: usize,
    pub error_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorEnvironmentVisibility {
    pub atm_home: Option<PathBuf>,
    pub atm_team: Option<TeamName>,
    pub atm_identity: Option<AgentName>,
    pub team_override: Option<TeamName>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapConnectOutcome {
    Connected,
    NotFound,
    Timeout,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapLaunchGateOutcome {
    Launched,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapAutoStartOutcome {
    AutoStarted,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapTraceReport {
    pub daemon_connect: BootstrapConnectOutcome,
    pub daemon_launch_gate: BootstrapLaunchGateOutcome,
    pub daemon_auto_start: BootstrapAutoStartOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_gate_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_start_detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DaemonRuntimeDoctorReport {
    pub findings: Vec<DoctorFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DoctorReport {
    pub summary: DoctorSummary,
    pub findings: Vec<DoctorFinding>,
    pub recommendations: Vec<String>,
    pub environment: DoctorEnvironmentVisibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_roster: Option<MembersList>,
    pub observability: AtmObservabilityHealth,
    pub config: ConfigDoctorReport,
    pub mail_store: MailStoreDoctorReport,
    pub roster_store: RosterStoreDoctorReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_runtime: Option<DaemonRuntimeDoctorReport>,
    #[serde(default)]
    pub drift_findings: Vec<DoctorFinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_status: Option<RuntimeStatusSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bootstrap_trace: Option<BootstrapTraceReport>,
}

impl DoctorReport {
    pub fn has_errors(&self) -> bool {
        self.summary.status == DoctorStatus::Error
    }
}
