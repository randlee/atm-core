use std::path::PathBuf;

use serde::Serialize;

use crate::error_codes::AtmErrorCode;
use crate::observability::AtmObservabilityHealth;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Healthy,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DoctorFinding {
    pub severity: DoctorSeverity,
    pub code: AtmErrorCode,
    pub message: String,
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DoctorSummary {
    pub status: DoctorStatus,
    pub message: String,
    pub info_count: usize,
    pub warning_count: usize,
    pub error_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DoctorEnvironmentVisibility {
    pub atm_home: Option<PathBuf>,
    pub atm_team: Option<String>,
    pub atm_identity: Option<String>,
    pub team_override: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DoctorReport {
    pub summary: DoctorSummary,
    pub findings: Vec<DoctorFinding>,
    pub recommendations: Vec<String>,
    pub environment: DoctorEnvironmentVisibility,
    pub observability: AtmObservabilityHealth,
}

impl DoctorReport {
    pub fn has_errors(&self) -> bool {
        self.summary.status == DoctorStatus::Error
    }
}
