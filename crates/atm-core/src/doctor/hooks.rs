use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::boundary::{HookName, PhaseId};
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::hook::{HOOK_SCHEMA_VERSION, HookTarget};
use crate::types::TeamName;

use super::report::DoctorSeverity;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookDoctorRequest {
    pub schema_version: String,
    pub hook: String,
    pub team: TeamName,
    pub phase: PhaseId,
    pub custom: Map<String, Value>,
}

impl HookDoctorRequest {
    pub fn for_target(target: &HookTarget) -> Self {
        Self {
            schema_version: HOOK_SCHEMA_VERSION.to_owned(),
            hook: "doctor".to_owned(),
            team: target.team.clone(),
            phase: target.phase.clone(),
            custom: target.custom.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookDoctorReply {
    pub success: bool,
    pub message: String,
    #[serde(default)]
    pub findings: Vec<HookDoctorFinding>,
}

impl HookDoctorReply {
    pub fn parse(stdout: &[u8]) -> Result<Self, AtmError> {
        serde_json::from_slice(stdout)
            .map_err(|error| AtmError::hook_malformed(&format!("doctor reply: {error}")))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookDoctorFinding {
    pub severity: DoctorSeverity,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub remediation: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookDoctorEntry {
    pub module_path: PathBuf,
    pub cwd: PathBuf,
    pub team: TeamName,
    pub phase: PhaseId,
    pub hooks: Vec<HookName>,
    pub schedules: Vec<String>,
    pub timeout_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub findings: Vec<HookDoctorFinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<HookDoctorError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookDoctorError {
    pub code: AtmErrorCode,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn doctor_request_and_reply_shapes() {
        let target = HookTarget {
            team: "test-team".parse().unwrap(),
            phase: PhaseId::parse("bk").unwrap(),
            module_path: "/hooks/goal.py".into(),
            cwd: "/repo".into(),
            timeout_ms: 1000,
            custom: json!({"goal": "ship"}).as_object().unwrap().clone(),
        };
        let request = HookDoctorRequest::for_target(&target);
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "schema_version": "1.0.0", "hook": "doctor", "team": "test-team", "phase": "bk", "custom": {"goal": "ship"}
            })
        );
        assert_eq!(
            serde_json::from_value::<HookDoctorRequest>(serde_json::to_value(&request).unwrap())
                .unwrap(),
            request
        );
        let reply = HookDoctorReply::parse(br#"{"success":true,"message":"ok"}"#).unwrap();
        assert!(reply.findings.is_empty());
        let example = json!({"success": true, "message": "manifest ok", "findings": [{
            "severity": "warning", "code": "ATM_BD_MANIFEST_DRIFT",
            "message": "skills/x.md differs from the published manifest",
            "remediation": "reinstall atm-bd-orchestration"
        }]});
        let reply = HookDoctorReply::parse(&serde_json::to_vec(&example).unwrap()).unwrap();
        assert_eq!(serde_json::to_value(&reply).unwrap(), example);
        let finding = serde_json::to_value(&reply.findings[0]).unwrap();
        let mut keys: Vec<_> = finding
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["code", "message", "remediation", "severity"]);
        for malformed in [
            json!({"message": "ok"}),
            json!({"success": true}),
            json!({"success": true, "message": "ok", "extra": 1}),
            json!({"success": true, "message": "ok", "findings": [{"severity": "fatal", "code": "X", "message": "bad"}]}),
            json!({"success": true, "message": "ok", "findings": [{"severity": "info", "message": "bad"}]}),
            json!({"success": true, "message": "ok", "findings": {}}),
            json!({"success": true, "message": "ok", "findings": [{"severity": "info", "code": "X", "message": "bad", "extra": 1}]}),
            json!(null),
        ] {
            let error =
                HookDoctorReply::parse(&serde_json::to_vec(&malformed).unwrap()).unwrap_err();
            assert_eq!(
                serde_json::to_value(error).unwrap()["code"],
                "ATM_HOOK_MALFORMED"
            );
        }
        for malformed in [b"ok".as_slice(), b"{} {}", b"", b"{\"success\":"] {
            let error = HookDoctorReply::parse(malformed).unwrap_err();
            assert_eq!(
                serde_json::to_value(error).unwrap()["code"],
                "ATM_HOOK_MALFORMED"
            );
        }
    }
}
