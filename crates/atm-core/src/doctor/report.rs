use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error_codes::AtmErrorCode;
use crate::observability::AtmObservabilityHealth;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorRuntimeHealth {
    pub singleton_state: DoctorStatus,
    pub singleton_detail: String,
    pub status_cache_state: DoctorStatus,
    pub status_cache_detail: String,
    pub sqlite_runtime_state: DoctorStatus,
    pub sqlite_runtime_detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorReport {
    pub summary: DoctorSummary,
    pub findings: Vec<DoctorFinding>,
    pub recommendations: Vec<String>,
    pub environment: DoctorEnvironmentVisibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_roster: Option<MembersList>,
    pub observability: AtmObservabilityHealth,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<DoctorRuntimeHealth>,
}

impl DoctorReport {
    pub fn has_errors(&self) -> bool {
        self.summary.status == DoctorStatus::Error
    }
}
