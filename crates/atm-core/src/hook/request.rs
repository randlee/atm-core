use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::boundary::{CronSchedule, HookName, HookRegistration, PhaseId, TaskCommand};
use crate::error::AtmError;
use crate::types::{AgentName, HostName, TeamName};

pub const HOOK_SCHEMA_VERSION: &str = "1.0.0";
pub const HOOK_CUSTOM_MAX_BYTES: usize = 65_536;
pub const HOOK_PATH_MAX_BYTES: usize = 4096;
pub const CRON_NAME_MAX_BYTES: usize = 64;
pub const CRON_ARGS_MAX: usize = 32;
pub const CRON_ARGS_MAX_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HookCaller {
    pub identity: AgentName,
    pub team: TeamName,
    pub host: Option<HostName>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookRequest {
    pub schema_version: String,
    pub hook: HookName,
    pub team: TeamName,
    pub phase: PhaseId,
    pub caller: HookCaller,
    pub request: Value,
    pub custom: Map<String, Value>,
}

pub fn custom_object(value: Value) -> Result<Map<String, Value>, AtmError> {
    let message =
        format!("custom must be an object of at most {HOOK_CUSTOM_MAX_BYTES} serialized bytes");
    let Value::Object(object) = value else {
        return Err(AtmError::hook_module_invalid(&message));
    };
    let bytes = serde_json::to_vec(&object).map_err(|_| AtmError::hook_module_invalid(&message))?;
    if bytes.len() > HOOK_CUSTOM_MAX_BYTES {
        return Err(AtmError::hook_module_invalid(&message));
    }
    Ok(object)
}

#[derive(Clone, Debug, PartialEq)]
pub struct HookTarget {
    pub team: TeamName,
    pub phase: PhaseId,
    pub module_path: PathBuf,
    pub cwd: PathBuf,
    pub timeout_ms: u32,
    pub custom: Map<String, Value>,
}

impl From<&HookRegistration> for HookTarget {
    fn from(row: &HookRegistration) -> Self {
        Self {
            team: row.team.clone(),
            phase: row.phase.clone(),
            module_path: row.module_path.clone(),
            cwd: row.cwd.clone(),
            timeout_ms: row.timeout_ms,
            custom: row.custom.clone(),
        }
    }
}

impl From<&CronSchedule> for HookTarget {
    fn from(row: &CronSchedule) -> Self {
        Self {
            team: row.team.clone(),
            phase: row.phase.clone(),
            module_path: row.module_path.clone(),
            cwd: row.cwd.clone(),
            timeout_ms: row.timeout_ms,
            custom: row.custom.clone(),
        }
    }
}

impl HookRequest {
    pub fn for_task(
        target: &HookTarget,
        hook: HookName,
        caller: HookCaller,
        command: &TaskCommand,
    ) -> Self {
        Self {
            schema_version: HOOK_SCHEMA_VERSION.to_owned(),
            hook,
            team: target.team.clone(),
            phase: target.phase.clone(),
            caller,
            request: command.request_body(),
            custom: target.custom.clone(),
        }
    }

    pub fn for_cron(
        target: &HookTarget,
        caller: HookCaller,
        hook: HookName,
        args: &[String],
    ) -> Self {
        let request = serde_json::json!({"args": args, "created_by": caller.identity});
        Self {
            schema_version: HOOK_SCHEMA_VERSION.to_owned(),
            hook,
            team: target.team.clone(),
            phase: target.phase.clone(),
            caller,
            request,
            custom: target.custom.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ApiRequest, encode_http_request};
    use crate::boundary::TaskOp;
    use crate::list::{ListQuery, TaskLedgerQuery};
    use crate::protocol::{RequestEnvelope, TaskMoveRequest};
    use crate::send::{SendMessageSource, WriteRequest};
    use crate::types::ReadSelection;
    use atm_storage::MoveTarget;
    use serde_json::json;

    fn target() -> HookTarget {
        HookTarget {
            team: "test-team".parse().unwrap(),
            phase: PhaseId::parse("bk").unwrap(),
            module_path: "/hooks/task.py".into(),
            cwd: "/repo".into(),
            timeout_ms: 1000,
            custom: custom_object(json!({"x": [1, {"y": null}]})).unwrap(),
        }
    }

    fn caller() -> HookCaller {
        HookCaller {
            identity: "solar".parse().unwrap(),
            team: "test-team".parse().unwrap(),
            host: Some("mbp".parse().unwrap()),
        }
    }

    fn task_envelopes() -> Vec<RequestEnvelope> {
        let write = WriteRequest::new(
            "/home".into(),
            "/repo".into(),
            "solar".parse().unwrap(),
            "trinity@test-team",
            "test-team".parse().unwrap(),
            SendMessageSource::Inline("work".into()),
            None,
            false,
            Some("task-1".parse().unwrap()),
            false,
        )
        .unwrap();
        let mut start = write.clone();
        start.task_op = Some(TaskOp::Start);
        let mut close = write.clone();
        close.task_complete = write.task_id.clone();
        let list = ListQuery::new(
            "/home".into(),
            "/repo".into(),
            "solar".parse().unwrap(),
            None,
            "test-team".parse().unwrap(),
            ReadSelection::All,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let mut tasks = list.clone();
        tasks.task_ledger = Some(TaskLedgerQuery::Tasks { member: None });
        let mut events = list;
        events.task_ledger = Some(TaskLedgerQuery::Events {
            task_id: "task-1".parse().unwrap(),
            member: None,
        });
        vec![
            RequestEnvelope::Write(Box::new(write)),
            RequestEnvelope::Write(Box::new(start)),
            RequestEnvelope::Write(Box::new(close)),
            RequestEnvelope::TaskMove(TaskMoveRequest {
                caller_identity: "solar".parse().unwrap(),
                caller_team: "test-team".parse().unwrap(),
                task_id: "task-1".parse().unwrap(),
                target: MoveTarget::Head,
            }),
            RequestEnvelope::List(tasks),
            RequestEnvelope::List(events),
        ]
    }

    fn assert_keys(request: &HookRequest) {
        let value = serde_json::to_value(request).unwrap();
        let mut keys: Vec<_> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "caller",
                "custom",
                "hook",
                "phase",
                "request",
                "schema_version",
                "team"
            ]
        );
        let mut caller_keys: Vec<_> = value["caller"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        caller_keys.sort_unstable();
        assert_eq!(caller_keys, ["host", "identity", "team"]);
        assert_eq!(value["schema_version"], HOOK_SCHEMA_VERSION);
    }

    #[test]
    fn hook_request_top_level_keys_and_request_shape_are_pinned() {
        for envelope in task_envelopes() {
            let encoded = encode_http_request(&envelope, &[]).unwrap();
            let command = TaskCommand::classify(ApiRequest::new(envelope)).unwrap();
            let request = HookRequest::for_task(&target(), command.hook_name(), caller(), &command);
            assert_keys(&request);
            assert_eq!(
                request.request,
                serde_json::from_slice::<Value>(&encoded.body).unwrap()
            );
        }
        for name in [HookName::CRON_TICK, HookName::CRON_STOP] {
            let request = HookRequest::for_cron(
                &target(),
                caller(),
                HookName::parse(name).unwrap(),
                &["goal".into()],
            );
            assert_keys(&request);
            assert_eq!(
                request.request,
                json!({"args": ["goal"], "created_by": "solar"})
            );
        }
    }

    #[test]
    fn custom_round_trips_and_comes_from_the_registration() {
        let target = target();
        let registration: HookRegistration = serde_json::from_value(json!({
            "team": "test-team", "phase": "bk", "hook": "task.close",
            "module_path": "/hooks/task.py", "cwd": "/repo", "timeout_ms": 1000,
            "custom": {"x": [1, {"y": null}]}, "active": true,
            "registered_by": "solar", "registered_at": "2026-10-18T00:00:00Z",
            "disabled_at": null
        }))
        .unwrap();
        assert_eq!(HookTarget::from(&registration), target);
        let schedule: CronSchedule = serde_json::from_value(json!({
            "id": 1, "generation": 1, "team": "test-team", "phase": "bk", "name": "goal",
            "cron_expr": "* * * * *", "module_path": "/hooks/task.py", "cwd": "/repo", "args": [],
            "timeout_ms": 1000, "jitter_s": 0, "custom": {"x": [1, {"y": null}]},
            "stop": {"kind": "hook"}, "active": true, "created_by": "solar",
            "created_at": "2026-10-18T00:00:00Z", "last_run_at": null, "last_outcome": null, "ended_at": null
        })).unwrap();
        assert_eq!(HookTarget::from(&schedule), target);
        let expected = serde_json::to_vec(&target.custom).unwrap();
        for envelope in task_envelopes() {
            let command = TaskCommand::classify(ApiRequest::new(envelope)).unwrap();
            let request = HookRequest::for_task(&target, command.hook_name(), caller(), &command);
            let decoded: HookRequest =
                serde_json::from_slice(&serde_json::to_vec(&request).unwrap()).unwrap();
            assert_eq!(serde_json::to_vec(&decoded.custom).unwrap(), expected);
        }
        for name in [HookName::CRON_TICK, HookName::CRON_STOP] {
            let request =
                HookRequest::for_cron(&target, caller(), HookName::parse(name).unwrap(), &[]);
            assert_eq!(serde_json::to_vec(&request.custom).unwrap(), expected);
        }
    }

    #[test]
    fn custom_object_refuses_non_object_and_oversize() {
        for value in [
            json!([1]),
            json!("s"),
            json!({"x": "x".repeat(HOOK_CUSTOM_MAX_BYTES - 7)}),
        ] {
            let error = custom_object(value).unwrap_err();
            let wire = serde_json::to_value(&error).unwrap();
            assert_eq!(wire["code"], "ATM_HOOK_MODULE_INVALID");
            assert!(error.to_string().contains("custom"));
            assert!(error.to_string().contains("65536"));
        }
        let boundary = json!({"x": "x".repeat(HOOK_CUSTOM_MAX_BYTES - 8)});
        assert_eq!(
            serde_json::to_vec(&boundary).unwrap().len(),
            HOOK_CUSTOM_MAX_BYTES
        );
        assert!(custom_object(boundary).is_ok());
    }
}
