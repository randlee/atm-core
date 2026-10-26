use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::boundary::{CronName, CronStartSpec, PhaseId, StopCondition};
use crate::error::AtmError;
use crate::types::{AgentName, TeamName};

use super::request::{CRON_ARGS_MAX_BYTES, custom_object};
use super::{CRON_ARGS_MAX, HOOK_PATH_MAX_BYTES};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HookRegisterRequest {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub phase: PhaseId,
    /// Raw on the wire on purpose: the register route validates it with
    /// `HookName::parse_task_hook`, so a bad name keeps the `HOOK.MODULE_INVALID`
    /// response. A validated serde type would move that rejection into the
    /// framework decode and change the response code.
    pub hook: String,
    pub module_path: PathBuf,
    pub cwd: PathBuf,
    pub timeout_ms: u32,
    #[serde(default)]
    pub custom: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HookDisableRequest {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub phase: PhaseId,
    /// Raw on the wire for the same reason as `HookRegisterRequest::hook`: the
    /// disable route validates it with `HookName::parse_task_hook`.
    pub hook: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HookListQuery {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub phase: Option<PhaseId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CronStartRequest {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub phase: PhaseId,
    pub name: String,
    pub cron_expr: String,
    pub module_path: PathBuf,
    pub cwd: PathBuf,
    pub args: Vec<String>,
    pub timeout_ms: u32,
    pub stop: StopCondition,
    #[serde(default)]
    pub custom: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CronEndRequest {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub phase: PhaseId,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CronListQuery {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub phase: Option<PhaseId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HookActionsResponse {
    pub dispatched: usize,
}

fn path_limit(path: &Path, field: &str) -> Result<(), AtmError> {
    if path.as_os_str().as_encoded_bytes().len() > HOOK_PATH_MAX_BYTES {
        return Err(AtmError::hook_module_invalid(&format!(
            "{field} must be at most {HOOK_PATH_MAX_BYTES} bytes"
        )));
    }
    Ok(())
}

/// An absent `custom` deserializes to `Null`; it means an empty map.
fn custom_or_empty(custom: &Value) -> Value {
    if custom.is_null() {
        Value::Object(serde_json::Map::new())
    } else {
        custom.clone()
    }
}

impl HookRegisterRequest {
    pub fn check_limits(&self) -> Result<Map<String, Value>, AtmError> {
        let custom = custom_object(custom_or_empty(&self.custom))?;
        path_limit(&self.module_path, "module_path")?;
        path_limit(&self.cwd, "cwd")?;
        Ok(custom)
    }
}

impl CronStartRequest {
    pub fn into_spec(self) -> Result<CronStartSpec, AtmError> {
        let custom = custom_object(custom_or_empty(&self.custom))?;
        path_limit(&self.module_path, "module_path")?;
        path_limit(&self.cwd, "cwd")?;
        let name = CronName::parse(&self.name)?;
        if self.args.len() > CRON_ARGS_MAX {
            return Err(AtmError::hook_module_invalid(&format!(
                "args must contain at most {CRON_ARGS_MAX} items"
            )));
        }
        if self.args.iter().map(String::len).sum::<usize>() > CRON_ARGS_MAX_BYTES {
            return Err(AtmError::hook_module_invalid(&format!(
                "args must contain at most {CRON_ARGS_MAX_BYTES} total bytes"
            )));
        }
        Ok(CronStartSpec {
            team: self.caller_team,
            phase: self.phase,
            name,
            cron_expr: self.cron_expr,
            module_path: self.module_path,
            cwd: self.cwd,
            args: self.args,
            timeout_ms: self.timeout_ms,
            custom,
            stop: self.stop,
            created_by: self.caller_identity,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use serde_json::json;

    const TEST_SENDER: &str = "test-sender";

    fn cron() -> CronStartRequest {
        serde_json::from_value(json!({
            "caller_identity": TEST_SENDER, "caller_team": "test-team", "phase": "bk",
            "name": "goal", "cron_expr": "* * * * *", "module_path": "/hooks/goal.py",
            "cwd": "/repo", "args": [], "timeout_ms": 1000, "stop": {"kind": "hook"},
            "custom": {}
        }))
        .unwrap()
    }

    fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: T) {
        let decoded: T = serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn request_structs_round_trip() {
        let request = cron();
        let register = HookRegisterRequest {
            caller_identity: request.caller_identity.clone(),
            caller_team: request.caller_team.clone(),
            phase: request.phase.clone(),
            hook: "task.close".into(),
            module_path: request.module_path.clone(),
            cwd: request.cwd.clone(),
            timeout_ms: request.timeout_ms,
            custom: json!({"goal": "ship"}),
        };
        round_trip(register.clone());
        round_trip(HookDisableRequest {
            caller_identity: request.caller_identity.clone(),
            caller_team: request.caller_team.clone(),
            phase: request.phase.clone(),
            hook: Some("task.close".into()),
        });
        round_trip(HookListQuery {
            caller_identity: request.caller_identity.clone(),
            caller_team: request.caller_team.clone(),
            phase: None,
        });
        round_trip(request.clone());
        round_trip(CronEndRequest {
            caller_identity: request.caller_identity.clone(),
            caller_team: request.caller_team.clone(),
            phase: request.phase.clone(),
            name: None,
        });
        round_trip(CronListQuery {
            caller_identity: request.caller_identity.clone(),
            caller_team: request.caller_team.clone(),
            phase: Some(request.phase.clone()),
        });
        round_trip(HookActionsResponse { dispatched: 3 });
        let mut value = serde_json::to_value(request).unwrap();
        value.as_object_mut().unwrap().remove("stop");
        assert!(serde_json::from_value::<CronStartRequest>(value.clone()).is_err());
        value["stop"] = json!({"kind": "until", "at": "2026-11-01T00:00:00Z"});
        let until: CronStartRequest = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(until.stop, StopCondition::Until { .. }));
        value.as_object_mut().unwrap().remove("custom");
        let absent: CronStartRequest = serde_json::from_value(value).unwrap();
        assert!(absent.custom.is_null());
        assert!(custom_object(absent.custom).is_err());
        let mut value = serde_json::to_value(register).unwrap();
        value.as_object_mut().unwrap().remove("custom");
        assert!(
            serde_json::from_value::<HookRegisterRequest>(value)
                .unwrap()
                .custom
                .is_null()
        );
    }

    fn assert_invalid(error: AtmError, field: &str, limit: usize) {
        assert_eq!(
            serde_json::to_value(&error).unwrap()["code"],
            "ATM_HOOK_MODULE_INVALID"
        );
        assert!(error.to_string().contains(field), "{error}");
        assert!(error.to_string().contains(&limit.to_string()), "{error}");
    }

    #[test]
    fn into_spec_refuses_each_oversize_field() {
        for name in [
            String::new(),
            "x".repeat(atm_storage::CRON_NAME_MAX_BYTES + 1),
        ] {
            let mut request = cron();
            request.name = name;
            assert_invalid(
                request.clone().into_spec().unwrap_err(),
                "name",
                atm_storage::CRON_NAME_MAX_BYTES,
            );
        }
        let mut request = cron();
        request.args = vec![String::new(); CRON_ARGS_MAX + 1];
        assert_invalid(
            request.clone().into_spec().unwrap_err(),
            "args",
            CRON_ARGS_MAX,
        );
        request.args = vec!["x".repeat(CRON_ARGS_MAX_BYTES + 1)];
        assert_invalid(
            request.clone().into_spec().unwrap_err(),
            "args",
            CRON_ARGS_MAX_BYTES,
        );
        for field in ["module_path", "cwd"] {
            let mut request = cron();
            let path = PathBuf::from("x".repeat(HOOK_PATH_MAX_BYTES + 1));
            if field == "module_path" {
                request.module_path = path;
            } else {
                request.cwd = path;
            }
            assert_invalid(
                request.clone().into_spec().unwrap_err(),
                field,
                HOOK_PATH_MAX_BYTES,
            );
            let registration = HookRegisterRequest {
                caller_identity: request.caller_identity,
                caller_team: request.caller_team,
                phase: request.phase,
                hook: "task.close".into(),
                module_path: request.module_path,
                cwd: request.cwd,
                timeout_ms: request.timeout_ms,
                custom: json!({}),
            };
            assert_invalid(
                registration.check_limits().unwrap_err(),
                field,
                HOOK_PATH_MAX_BYTES,
            );
        }
        let mut request = cron();
        request.name = "x".repeat(atm_storage::CRON_NAME_MAX_BYTES);
        request.args = vec!["x".repeat(CRON_ARGS_MAX_BYTES)];
        request.args.extend(vec![String::new(); CRON_ARGS_MAX - 1]);
        request.module_path = "x".repeat(HOOK_PATH_MAX_BYTES).into();
        request.cwd = "x".repeat(HOOK_PATH_MAX_BYTES).into();
        let spec = request.clone().into_spec().unwrap();
        assert_eq!(spec.name.as_str().len(), atm_storage::CRON_NAME_MAX_BYTES);
        assert_eq!(spec.custom, Map::new());
        let registration = HookRegisterRequest {
            caller_identity: request.caller_identity,
            caller_team: request.caller_team,
            phase: request.phase,
            hook: "task.close".into(),
            module_path: request.module_path,
            cwd: request.cwd,
            timeout_ms: request.timeout_ms,
            custom: json!({}),
        };
        assert!(registration.check_limits().is_ok());
        let mut request = cron();
        request.custom = json!([1]);
        assert_invalid(
            request.into_spec().unwrap_err(),
            "custom",
            super::super::HOOK_CUSTOM_MAX_BYTES,
        );
    }
}
