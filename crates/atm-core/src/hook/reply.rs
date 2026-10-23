use serde::de::{DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::api::{ApiRequest, MessageCollectionRequest};
use crate::boundary::{HookName, TaskCommand};
use crate::clear::ClearQuery;
use crate::doctor::DoctorQuery;
use crate::error::AtmError;
use crate::list::{ListQuery, TaskLedgerQuery};
use crate::protocol::TaskMoveRequest;
use crate::read::{PeekQuery, ReadQuery};
use crate::search::SearchRequest;
use crate::send::WriteRequest;
use crate::types::{AgentName, TeamName};
use atm_storage::{CronDiagnosticDetail, CronModuleOutput, CronOutputBody};

use super::{CRON_OUTPUT_MAX_ITEMS, CronStopReply, CronTickReply};

pub(crate) const HOOK_ACTIONS_MAX: usize = 32;

/// Parse a cron tick reply without widening the task-hook reply grammar.
///
/// Unlike `serde_json::Value`, the small strict-value reader below rejects a
/// duplicate key before it can be silently overwritten.  Cron replies are a
/// security boundary: the hook receives authority to request actions, so its
/// complete JSON tree is duplicate-free, not merely the top-level object.
pub fn parse_cron_tick_reply(stdout: &[u8]) -> Result<CronTickReply, AtmError> {
    let object = strict_object(stdout, "cron tick reply")?;
    let result = required_string(&object, "result")?;
    let output = parse_cron_output(object.get("cron-output"))?;

    match result {
        "pass-through" if keys_are(&object, &["result", "cron-output"]) => {
            Ok(CronTickReply::PassThrough { output })
        }
        "handled" if keys_are(&object, &["result", "atm-action", "cron-output"]) => {
            let actions = object.get("atm-action").ok_or_else(|| {
                AtmError::hook_malformed("handled cron reply requires atm-action")
            })?;
            let actions: Vec<HookAction> = serde_json::from_value(actions.clone())
                .map_err(|error| AtmError::hook_malformed(&format!("cron atm-action: {error}")))?;
            if actions.is_empty() || actions.len() > HOOK_ACTIONS_MAX {
                return Err(AtmError::hook_malformed(&format!(
                    "cron atm-action must contain 1..={HOOK_ACTIONS_MAX} actions"
                )));
            }
            Ok(CronTickReply::Handled { actions, output })
        }
        "not-applicable" if object.len() == 1 => Ok(CronTickReply::NotApplicable),
        "refused" if keys_are(&object, &["result", "message"]) => Ok(CronTickReply::Refused {
            message: CronDiagnosticDetail::parse(required_string(&object, "message")?.to_owned())?,
        }),
        _ => Err(AtmError::hook_malformed(
            "cron tick reply keys and result must match pass-through, handled, not-applicable or refused exactly",
        )),
    }
}

/// Parse the cron-only stop-check reply.  Task hooks do not use this shape.
pub fn parse_cron_stop_reply(stdout: &[u8]) -> Result<CronStopReply, AtmError> {
    let object = strict_object(stdout, "cron stop reply")?;
    let stop = object
        .get("stop")
        .and_then(Value::as_bool)
        .ok_or_else(|| AtmError::hook_malformed("cron stop reply requires boolean stop"))?;
    let reason = object
        .get("reason")
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| AtmError::hook_malformed("cron stop reason must be a string"))
        })
        .transpose()?
        .map(|value| CronDiagnosticDetail::parse(value.to_owned()))
        .transpose()?;
    if !keys_are(&object, &["stop", "reason", "cron-output"]) {
        return Err(AtmError::hook_malformed(
            "cron stop reply permits only stop, reason and cron-output",
        ));
    }
    CronStopReply::new(stop, reason, parse_cron_output(object.get("cron-output"))?)
}

fn required_string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, AtmError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AtmError::hook_malformed(&format!("cron reply requires non-empty {key}")))
}

fn keys_are(object: &Map<String, Value>, allowed: &[&str]) -> bool {
    object.keys().all(|key| allowed.contains(&key.as_str()))
}

fn parse_cron_output(value: Option<&Value>) -> Result<Vec<CronModuleOutput>, AtmError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| AtmError::hook_malformed("cron-output must be an array"))?;
    if values.len() > CRON_OUTPUT_MAX_ITEMS {
        return Err(AtmError::hook_malformed(&format!(
            "cron-output must contain at most {CRON_OUTPUT_MAX_ITEMS} entries"
        )));
    }
    values
        .iter()
        .map(|item| {
            let item = item
                .as_object()
                .ok_or_else(|| AtmError::hook_malformed("cron-output entries must be objects"))?;
            if item.len() != 2 || !item.contains_key("kind") || !item.contains_key("body") {
                return Err(AtmError::hook_malformed(
                    "cron-output entries require exactly kind and body",
                ));
            }
            let body = item
                .get("body")
                .and_then(Value::as_str)
                .ok_or_else(|| AtmError::hook_malformed("cron-output body must be a string"))?;
            let body = CronOutputBody::parse(body.to_owned())?;
            match item.get("kind").and_then(Value::as_str) {
                Some("message") => Ok(CronModuleOutput::Message { body }),
                Some("error") => Ok(CronModuleOutput::Error { body }),
                _ => Err(AtmError::hook_malformed(
                    "cron-output kind must be message or error",
                )),
            }
        })
        .collect()
}

fn strict_object(stdout: &[u8], context: &str) -> Result<Map<String, Value>, AtmError> {
    let mut deserializer = serde_json::Deserializer::from_slice(stdout);
    let value = StrictValue::deserialize(&mut deserializer)
        .map_err(|error| AtmError::hook_malformed(&format!("{context} JSON: {error}")))?;
    deserializer.end().map_err(|error| {
        AtmError::hook_malformed(&format!("{context} must contain one JSON object: {error}"))
    })?;
    match value.0 {
        Value::Object(object) => Ok(object),
        _ => Err(AtmError::hook_malformed(&format!(
            "{context} must be one JSON object"
        ))),
    }
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct StrictVisitor;
        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictValue;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON value without duplicate object keys")
            }
            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(value)))
            }
            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(value.into())))
            }
            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(value.into())))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|value| StrictValue(Value::Number(value)))
                    .ok_or_else(|| serde::de::Error::custom("invalid JSON number"))
            }
            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(value.to_owned())))
            }
            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(value)))
            }
            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<StrictValue>()? {
                    values.push(value.0);
                }
                Ok(StrictValue(Value::Array(values)))
            }
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate object key `{key}`"
                        )));
                    }
                    values.insert(key, map.next_value::<StrictValue>()?.0);
                }
                Ok(StrictValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(StrictVisitor)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HookReply {
    Handled(Vec<HookAction>),
    PassThrough,
    NotApplicable,
    Refused { message: String },
}

impl HookReply {
    pub fn parse(stdout: &[u8]) -> Result<Self, AtmError> {
        let value: Value = serde_json::from_slice(stdout)
            .map_err(|error| AtmError::hook_malformed(&format!("reply JSON: {error}")))?;
        let object = value.as_object().ok_or_else(|| {
            AtmError::hook_malformed("reply must be one JSON object with a result")
        })?;
        match object.get("result").and_then(Value::as_str) {
            Some("handled") if object.len() == 2 && object.contains_key("atm-action") => {
                let actions: Vec<HookAction> = serde_json::from_value(object["atm-action"].clone())
                    .map_err(|error| AtmError::hook_malformed(&format!("atm-action: {error}")))?;
                if actions.is_empty() || actions.len() > HOOK_ACTIONS_MAX {
                    return Err(AtmError::hook_malformed(&format!(
                        "atm-action must contain 1..={HOOK_ACTIONS_MAX} actions"
                    )));
                }
                Ok(Self::Handled(actions))
            }
            Some("pass-through") if object.len() == 1 => Ok(Self::PassThrough),
            Some("not-applicable") if object.len() == 1 => Ok(Self::NotApplicable),
            Some("refused") if object.len() == 2 => {
                let message = object
                    .get("message")
                    .and_then(Value::as_str)
                    .filter(|message| !message.is_empty())
                    .ok_or_else(|| {
                        AtmError::hook_malformed("refused reply requires a non-empty message")
                    })?;
                Ok(Self::Refused {
                    message: message.to_owned(),
                })
            }
            _ => Err(AtmError::hook_malformed(
                "reply keys and result must match handled, pass-through, not-applicable or refused exactly",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum HookActionCommand {
    Send,
    Ack,
    TaskAssign,
    TaskStart,
    TaskClose,
    TaskMove,
    TaskList,
    TaskEvents,
    TaskHistory,
    Read,
    Peek,
    Clear,
    Search,
    Doctor,
}

impl HookActionCommand {
    pub const ALL: [Self; 14] = [
        Self::Send,
        Self::Ack,
        Self::TaskAssign,
        Self::TaskStart,
        Self::TaskClose,
        Self::TaskMove,
        Self::TaskList,
        Self::TaskEvents,
        Self::TaskHistory,
        Self::Read,
        Self::Peek,
        Self::Clear,
        Self::Search,
        Self::Doctor,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Ack => "ack",
            Self::TaskAssign => "task.assign",
            Self::TaskStart => "task.start",
            Self::TaskClose => "task.close",
            Self::TaskMove => "task.move",
            Self::TaskList => "task.list",
            Self::TaskEvents => "task.events",
            Self::TaskHistory => "task.history",
            Self::Read => "read",
            Self::Peek => "peek",
            Self::Clear => "clear",
            Self::Search => "search",
            Self::Doctor => "doctor",
        }
    }

    pub fn task_hook(self) -> Option<HookName> {
        match self {
            Self::TaskAssign
            | Self::TaskStart
            | Self::TaskClose
            | Self::TaskMove
            | Self::TaskList
            | Self::TaskEvents => HookName::parse_task_hook(self.as_str()).ok(),
            _ => None,
        }
    }
}

impl TryFrom<String> for HookActionCommand {
    type Error = AtmError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::ALL
            .into_iter()
            .find(|command| command.as_str() == value)
            .ok_or_else(|| {
                AtmError::hook_malformed(&format!("command {value} is not an allowed hook action"))
            })
    }
}

impl From<HookActionCommand> for String {
    fn from(command: HookActionCommand) -> Self {
        command.as_str().to_owned()
    }
}

impl std::fmt::Display for HookActionCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookAction {
    pub command: HookActionCommand,
    #[serde(rename = "as", default)]
    pub run_as: Option<AgentName>,
    pub request: Value,
}

impl HookAction {
    fn malformed(&self, detail: &str) -> AtmError {
        AtmError::hook_malformed(&format!("action {}: {detail}", self.command))
    }

    fn body<T: DeserializeOwned>(&self) -> Result<T, AtmError> {
        serde_json::from_value(self.request.clone())
            .map_err(|error| self.malformed(&format!("request: {error}")))
    }

    fn check_team(&self, actual: &TeamName, expected: &TeamName) -> Result<(), AtmError> {
        if actual != expected {
            return Err(self.malformed("caller_team must equal the registered team"));
        }
        Ok(())
    }

    fn write_request(&self, team: &TeamName, identity: &AgentName) -> Result<ApiRequest, AtmError> {
        let mut request: WriteRequest = self.body()?;
        self.check_team(&request.caller_team, team)?;
        if request.authenticated_source_host.is_some()
            || request.peer_http_api_version.is_some()
            || request.origin_message_id.is_some()
            || request.origin_timestamp.is_some()
        {
            return Err(
                self.malformed("request must not supply authenticated peer provenance fields")
            );
        }
        request.caller_identity = identity.clone();
        match self.command {
            HookActionCommand::Send
                if request.task_id.is_none()
                    && request.task_op.is_none()
                    && request.task_complete.is_none()
                    && request.acknowledges_message_id.is_none() => {}
            HookActionCommand::Ack if request.acknowledges_message_id.is_some() => {}
            HookActionCommand::TaskAssign
            | HookActionCommand::TaskStart
            | HookActionCommand::TaskClose => {
                let classified = TaskCommand::classify(ApiRequest::Write(Box::new(
                    request.clone(),
                )))
                .map_err(|_| self.malformed("request does not classify as a task command"))?;
                if Some(classified.hook_name()) != self.command.task_hook() {
                    return Err(self.malformed("request classifies as a different task command"));
                }
            }
            _ => {
                return Err(
                    self.malformed("request fields do not match the named send or ack command")
                );
            }
        }
        Ok(ApiRequest::Write(Box::new(request)))
    }

    fn list_request(&self, team: &TeamName, identity: &AgentName) -> Result<ApiRequest, AtmError> {
        let mut request: ListQuery = self.body()?;
        self.check_team(&request.caller_team, team)?;
        let is_history = self.command == HookActionCommand::TaskHistory
            && matches!(request.task_ledger, Some(TaskLedgerQuery::History { .. }));
        request.caller_identity = identity.clone();
        let api_request = ApiRequest::Messages(Box::new(MessageCollectionRequest::List(request)));
        if !is_history
            && !TaskCommand::classify(api_request.clone())
                .is_ok_and(|classified| Some(classified.hook_name()) == self.command.task_hook())
        {
            return Err(self.malformed("task_ledger must match the named task query"));
        }
        Ok(api_request)
    }

    pub fn into_api_request(
        &self,
        team: &TeamName,
        default_as: &AgentName,
    ) -> Result<ApiRequest, AtmError> {
        let identity = self.run_as.as_ref().unwrap_or(default_as);
        match self.command {
            HookActionCommand::Send
            | HookActionCommand::Ack
            | HookActionCommand::TaskAssign
            | HookActionCommand::TaskStart
            | HookActionCommand::TaskClose => self.write_request(team, identity),
            HookActionCommand::TaskMove => {
                let mut request: TaskMoveRequest = self.body()?;
                self.check_team(&request.caller_team, team)?;
                request.caller_identity = identity.clone();
                Ok(ApiRequest::TaskMove(request))
            }
            HookActionCommand::TaskList
            | HookActionCommand::TaskEvents
            | HookActionCommand::TaskHistory => self.list_request(team, identity),
            HookActionCommand::Read => {
                let mut request: ReadQuery = self.body()?;
                self.check_team(&request.caller_team, team)?;
                request.caller_identity = identity.clone();
                Ok(ApiRequest::Messages(Box::new(
                    MessageCollectionRequest::Receive(request),
                )))
            }
            HookActionCommand::Peek => {
                let mut request: PeekQuery = self.body()?;
                self.check_team(&request.caller_team, team)?;
                request.caller_identity = identity.clone();
                Ok(ApiRequest::Messages(Box::new(
                    MessageCollectionRequest::Peek(request),
                )))
            }
            HookActionCommand::Clear => {
                let mut request: ClearQuery = self.body()?;
                self.check_team(&request.caller_team, team)?;
                request.caller_identity = identity.clone();
                Ok(ApiRequest::Clear(request))
            }
            HookActionCommand::Search => {
                let mut request: SearchRequest = self.body()?;
                if self.run_as.is_some()
                    || request
                        .query
                        .team
                        .as_ref()
                        .is_some_and(|actual| actual != team.as_str())
                {
                    return Err(self.malformed("search requires no as and no foreign query.team"));
                }
                request.query.team = Some(team.to_string());
                Ok(ApiRequest::Search(Box::new(request)))
            }
            HookActionCommand::Doctor => {
                let mut request: DoctorQuery = self.body()?;
                if self.run_as.is_some()
                    || request.all_teams
                    || request
                        .team_override
                        .as_ref()
                        .is_some_and(|actual| actual != team)
                    || request
                        .caller_team
                        .as_ref()
                        .is_some_and(|actual| actual != team)
                {
                    return Err(self
                        .malformed("doctor requires no as, all_teams false and no foreign team"));
                }
                request.caller_team = Some(team.clone());
                request.team_override = Some(team.clone());
                Ok(ApiRequest::Doctor(request))
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopReply {
    pub stop: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

impl StopReply {
    pub fn parse(stdout: &[u8]) -> Result<Self, AtmError> {
        let value: Value = serde_json::from_slice(stdout)
            .map_err(|error| AtmError::hook_malformed(&format!("stop reply JSON: {error}")))?;
        let reply: Self = serde_json::from_value(value.clone())
            .map_err(|error| AtmError::hook_malformed(&format!("stop reply: {error}")))?;
        if (reply.stop
            && reply
                .reason
                .as_ref()
                .is_some_and(|reason| !reason.is_empty()))
            || (!reply.stop && value.as_object().is_some_and(|object| object.len() == 1))
        {
            return Ok(reply);
        }
        Err(AtmError::hook_malformed(
            "stop reply must be exactly {stop:true,reason:<non-empty string>} or {stop:false}",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::encode_http_request;
    use crate::protocol::RequestEnvelope;
    use crate::search::SearchInput;
    use crate::send::SendMessageSource;
    use serde_json::json;
    use std::path::PathBuf;

    fn team() -> TeamName {
        "test-team".parse().unwrap()
    }

    fn identity() -> AgentName {
        "caller".parse().unwrap()
    }

    fn action(command: HookActionCommand, request: Value) -> HookAction {
        HookAction {
            command,
            run_as: None,
            request,
        }
    }

    fn write_body() -> Value {
        serde_json::to_value(
            WriteRequest::new(
                PathBuf::from("/home"),
                PathBuf::from("/repo"),
                "untrusted".parse().unwrap(),
                "receiver@test-team",
                team(),
                SendMessageSource::Inline("message".to_owned()),
                None,
                false,
                None,
                false,
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn list_body(ledger: Value) -> Value {
        json!({
            "home_dir":"/home", "current_dir":"/repo", "caller_identity":"untrusted",
            "caller_team":"test-team", "selection_mode":"all", "seen_state_filter":false,
            "task_ledger":ledger
        })
    }

    fn mailbox_body(read: bool) -> Value {
        let mut body = json!({
            "mailbox":{"home_dir":"/home", "current_dir":"/repo", "selection_mode":"all", "seen_state_filter":false},
            "caller_identity":"untrusted", "caller_team":"test-team"
        });
        if read {
            body["seen_state_update"] = json!(true);
        }
        body
    }

    fn search_body() -> Value {
        serde_json::to_value(SearchRequest {
            query: SearchInput::default(),
            lifecycle: None,
        })
        .unwrap()
    }

    fn doctor_body() -> Value {
        json!({"home_dir":"/home", "current_dir":"/repo"})
    }

    fn mapped_body(request: ApiRequest) -> (&'static str, Value) {
        match request {
            ApiRequest::Write(body) => ("write", serde_json::to_value(body).unwrap()),
            ApiRequest::TaskMove(body) => ("move", serde_json::to_value(body).unwrap()),
            ApiRequest::Messages(body) => match *body {
                MessageCollectionRequest::List(body) => {
                    ("list", serde_json::to_value(body).unwrap())
                }
                MessageCollectionRequest::Peek(body) => {
                    ("peek", serde_json::to_value(body).unwrap())
                }
                MessageCollectionRequest::Receive(body) => {
                    ("read", serde_json::to_value(body).unwrap())
                }
            },
            ApiRequest::Clear(body) => ("clear", serde_json::to_value(body).unwrap()),
            ApiRequest::Search(body) => ("search", serde_json::to_value(body).unwrap()),
            ApiRequest::Doctor(body) => ("doctor", serde_json::to_value(body).unwrap()),
            unexpected => panic!("unexpected mapped request: {unexpected:?}"),
        }
    }

    fn malformed<T: std::fmt::Debug>(result: Result<T, AtmError>) {
        assert_eq!(result.unwrap_err().code().as_str(), "ATM_HOOK_MALFORMED");
    }

    #[test]
    fn action_maps_each_command() {
        let send = write_body();
        let mut ack = send.clone();
        ack["acknowledges_message_id"] = json!("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        let mut assign = send.clone();
        assign["task_id"] = json!("work-1");
        let mut start = assign.clone();
        start["task_op"] = json!({"op":"start"});
        let mut close = assign.clone();
        close["task_op"] = json!({"op":"close", "outcome":"completed"});
        let cases = [
            (HookActionCommand::Send, send, "write"),
            (HookActionCommand::Ack, ack, "write"),
            (HookActionCommand::TaskAssign, assign.clone(), "write"),
            (HookActionCommand::TaskStart, start, "write"),
            (HookActionCommand::TaskClose, close, "write"),
            (
                HookActionCommand::TaskMove,
                json!({"caller_identity":"untrusted", "caller_team":"test-team", "task_id":"work-1", "target":{"to":"head"}}),
                "move",
            ),
            (
                HookActionCommand::TaskList,
                list_body(json!({"Tasks":{"member":null}})),
                "list",
            ),
            (
                HookActionCommand::TaskEvents,
                list_body(json!({"Events":{"task_id":"work-1", "member":null}})),
                "list",
            ),
            (
                HookActionCommand::TaskHistory,
                list_body(json!({"History":{"member":null, "limit":10}})),
                "list",
            ),
            (HookActionCommand::Read, mailbox_body(true), "read"),
            (HookActionCommand::Peek, mailbox_body(false), "peek"),
            (
                HookActionCommand::Clear,
                json!({"home_dir":"/home", "current_dir":"/repo", "caller_identity":"untrusted", "caller_team":"test-team", "idle_only":false, "dry_run":false}),
                "clear",
            ),
        ];
        for (command, body, variant) in cases {
            for run_as in [None, Some("lead".parse().unwrap())] {
                let candidate = HookAction {
                    run_as: run_as.clone(),
                    ..action(command, body.clone())
                };
                let (actual_variant, mapped) =
                    mapped_body(candidate.into_api_request(&team(), &identity()).unwrap());
                assert_eq!(actual_variant, variant, "{command}");
                assert_eq!(
                    mapped["caller_identity"],
                    run_as.as_ref().map_or("caller", AgentName::as_str),
                    "{command}"
                );
                assert_eq!(mapped["caller_team"], "test-team");
            }
            let mut foreign = body;
            foreign["caller_team"] = json!("foreign");
            malformed(action(command, foreign).into_api_request(&team(), &identity()));
        }
        let route_request: WriteRequest = serde_json::from_value(assign.clone()).unwrap();
        let encoded =
            encode_http_request(&RequestEnvelope::Write(Box::new(route_request)), &[]).unwrap();
        let route_body: WriteRequest = serde_json::from_slice(&encoded.body).unwrap();
        let action_identity: AgentName = "lead".parse().unwrap();
        let mut expected = serde_json::to_value(route_body).unwrap();
        expected["caller_identity"] = serde_json::to_value(&action_identity).unwrap();
        let candidate = HookAction {
            run_as: Some(action_identity),
            ..action(HookActionCommand::TaskAssign, assign)
        };
        let ApiRequest::Write(mapped) = candidate.into_api_request(&team(), &identity()).unwrap()
        else {
            panic!("task assign was not a write")
        };
        assert_eq!(serde_json::to_value(mapped).unwrap(), expected);

        for command in [HookActionCommand::Search, HookActionCommand::Doctor] {
            let mut body = if command == HookActionCommand::Search {
                search_body()
            } else {
                doctor_body()
            };
            for configured in [false, true] {
                if configured {
                    if command == HookActionCommand::Search {
                        body["query"]["team"] = json!("test-team");
                    } else {
                        body["team_override"] = json!("test-team");
                        body["caller_team"] = json!("test-team");
                    }
                }
                let (variant, mapped) = mapped_body(
                    action(command, body.clone())
                        .into_api_request(&team(), &identity())
                        .unwrap(),
                );
                assert_eq!(variant, command.as_str());
                if command == HookActionCommand::Search {
                    assert_eq!(mapped["query"]["team"], "test-team");
                } else {
                    assert_eq!(mapped["team_override"], "test-team");
                    assert_eq!(mapped["caller_team"], "test-team");
                }
            }
            let with_as = HookAction {
                run_as: Some(identity()),
                ..action(command, body.clone())
            };
            malformed(with_as.into_api_request(&team(), &identity()));
            if command == HookActionCommand::Search {
                body["query"]["team"] = json!("foreign");
            } else {
                body["team_override"] = json!("foreign");
            }
            malformed(action(command, body).into_api_request(&team(), &identity()));
        }
        for change in [json!({"all_teams":true}), json!({"caller_team":"foreign"})] {
            let mut body = doctor_body();
            body.as_object_mut()
                .unwrap()
                .extend(change.as_object().unwrap().clone());
            malformed(
                action(HookActionCommand::Doctor, body).into_api_request(&team(), &identity()),
            );
        }
    }

    #[test]
    fn registry_cron_and_internal_commands_are_malformed() {
        for command in [
            "hook.register",
            "hook.disable",
            "hook.list",
            "cron.start",
            "cron.end",
            "cron.list",
            "heartbeat",
            "graft",
            "reload",
            "compatibility",
            "queue",
            "task.foo",
            "",
        ] {
            let reply = format!(
                r#"{{"result":"handled","atm-action":[{{"command":"{command}","request":{{}}}}]}}"#
            );
            malformed(HookReply::parse(reply.as_bytes()));
        }
        let mut body = write_body();
        body["task_id"] = json!("work-1");
        malformed(
            action(HookActionCommand::Send, body.clone()).into_api_request(&team(), &identity()),
        );
        malformed(
            action(HookActionCommand::TaskStart, body.clone())
                .into_api_request(&team(), &identity()),
        );
        malformed(
            action(HookActionCommand::TaskClose, body.clone())
                .into_api_request(&team(), &identity()),
        );
        malformed(action(HookActionCommand::Ack, body).into_api_request(&team(), &identity()));
        malformed(
            action(
                HookActionCommand::TaskList,
                list_body(json!({"History":{"member":null,"limit":1}})),
            )
            .into_api_request(&team(), &identity()),
        );
        malformed(
            action(HookActionCommand::Send, json!({})).into_api_request(&team(), &identity()),
        );
        for (field, value) in [
            ("authenticated_source_host", json!("peer.example.test")),
            ("peer_http_api_version", json!("1.0.0")),
            ("origin_message_id", json!("01ARZ3NDEKTSV4RRFFQ69G5FAV")),
            ("origin_timestamp", json!("2026-10-20T00:00:00Z")),
        ] {
            let mut body = write_body();
            body[field] = value;
            malformed(action(HookActionCommand::Send, body).into_api_request(&team(), &identity()));
        }
        assert!(
            serde_json::from_value::<HookAction>(
                json!({"command":"send", "request":{}, "extra":1})
            )
            .is_err()
        );
    }

    #[test]
    fn hook_action_command_round_trips_every_variant() {
        for command in HookActionCommand::ALL {
            assert_eq!(
                HookActionCommand::try_from(command.as_str().to_owned()),
                Ok(command)
            );
            assert_eq!(
                serde_json::to_value(command).unwrap(),
                json!(command.as_str())
            );
            assert_eq!(
                command.task_hook().is_some(),
                matches!(
                    command,
                    HookActionCommand::TaskAssign
                        | HookActionCommand::TaskStart
                        | HookActionCommand::TaskClose
                        | HookActionCommand::TaskMove
                        | HookActionCommand::TaskList
                        | HookActionCommand::TaskEvents
                )
            );
        }
    }

    #[test]
    fn task_query_action_must_match_classified_command_or_history() {
        let ledgers = [
            (
                Some(HookActionCommand::TaskList),
                json!({"Tasks":{"member":null}}),
            ),
            (
                Some(HookActionCommand::TaskEvents),
                json!({"Events":{"task_id":"work-1", "member":null}}),
            ),
            (
                Some(HookActionCommand::TaskHistory),
                json!({"History":{"member":null, "limit":10}}),
            ),
            (None, Value::Null),
        ];
        for command in [
            HookActionCommand::TaskList,
            HookActionCommand::TaskEvents,
            HookActionCommand::TaskHistory,
        ] {
            for (expected, ledger) in &ledgers {
                let candidate = action(command, list_body(ledger.clone()));
                let result = candidate.into_api_request(&team(), &identity());
                if Some(command) == *expected {
                    let (variant, body) = mapped_body(result.unwrap());
                    assert_eq!(variant, "list");
                    assert_eq!(body["caller_identity"], "caller");
                    assert_eq!(body["caller_team"], "test-team");
                    assert_eq!(body["task_ledger"], *ledger);
                } else {
                    malformed(result);
                }
            }
        }
    }

    fn malformed_replies() -> Vec<Vec<u8>> {
        let mut cases = vec![
            r#"{"result":"handled","atm-action":{}}"#,
            r#"{"result":"handled","atm-action":[]}"#,
            r#"{"result":"handled","atm-action":[1]}"#,
            r#"{"result":"handled"}"#,
            r#"{"result":"pass-through","extra":1}"#,
            r#"{"result":"not-applicable","message":"x"}"#,
            r#"{"result":"refused","message":""}"#,
            r#"{"result":"refused"}"#,
            r#"{"result":"refused","message":1}"#,
            r#"{"result":"refused","message":"x","extra":1}"#,
            r#"{"result":"handled","atm-action":[{"command":"send","request":{},"extra":1}]}"#,
            r#"{"result":"handled","atm-action":[{"request":{}}]}"#,
            r#"{"result":"skip"}"#,
            r#"{"result":true}"#,
            r#"{"success":true}"#,
            r#"{"success":false,"message":"m"}"#,
            r#"{} {}"#,
            "",
            "null",
            "[]",
            r#"{"result":"#,
        ]
        .into_iter()
        .map(|case| case.as_bytes().to_vec())
        .collect::<Vec<_>>();
        cases.push(vec![255]);
        cases.push(serde_json::to_vec(&json!({"result":"handled","atm-action":vec![action(HookActionCommand::Send, write_body()); HOOK_ACTIONS_MAX + 1]})).unwrap());
        cases
    }

    #[test]
    fn reply_parse_case_table() {
        assert_eq!(
            HookReply::parse(br#"{"result":"pass-through"}"#).unwrap(),
            HookReply::PassThrough
        );
        assert_eq!(
            HookReply::parse(br#"{"result":"not-applicable"}"#).unwrap(),
            HookReply::NotApplicable
        );
        assert_eq!(
            HookReply::parse(br#"{"result":"refused","message":"verbatim message"}"#).unwrap(),
            HookReply::Refused {
                message: "verbatim message".to_owned()
            }
        );
        let actions = vec![
            action(HookActionCommand::Send, write_body()),
            action(HookActionCommand::Doctor, doctor_body()),
        ];
        assert_eq!(
            HookReply::parse(
                &serde_json::to_vec(&json!({"result":"handled","atm-action":actions})).unwrap()
            )
            .unwrap(),
            HookReply::Handled(actions)
        );
        let actions = vec![action(HookActionCommand::Send, write_body()); HOOK_ACTIONS_MAX];
        assert_eq!(
            HookReply::parse(
                &serde_json::to_vec(&json!({"result":"handled","atm-action":actions})).unwrap()
            )
            .unwrap(),
            HookReply::Handled(actions)
        );
        for stdout in malformed_replies() {
            malformed(HookReply::parse(&stdout));
        }
    }

    #[test]
    fn malformed_is_never_pass_through() {
        for stdout in malformed_replies() {
            let reply = HookReply::parse(&stdout);
            assert!(!matches!(
                reply,
                Ok(HookReply::PassThrough | HookReply::NotApplicable)
            ));
            malformed(reply);
        }
    }

    #[test]
    fn stop_reply_shapes() {
        assert_eq!(
            StopReply::parse(br#"{"stop":true,"reason":"r"}"#).unwrap(),
            StopReply {
                stop: true,
                reason: Some("r".to_owned())
            }
        );
        assert_eq!(
            StopReply::parse(br#"{"stop":false}"#).unwrap(),
            StopReply {
                stop: false,
                reason: None
            }
        );
        for stdout in [
            r#"{"stop":true}"#,
            r#"{"stop":true,"reason":""}"#,
            r#"{"stop":false,"reason":"x"}"#,
            r#"{"stop":false,"reason":null}"#,
            r#"{"stop":"yes"}"#,
            r#"{"stop":1}"#,
            r#"{"stop":true,"reason":"r","extra":1}"#,
            r#"{"stop":false,"extra":1}"#,
            r#"{"stop":true,"reason":null}"#,
            "null",
            "[]",
            "",
            "{} {}",
            r#"{"stop":"#,
        ] {
            malformed(StopReply::parse(stdout.as_bytes()));
        }
    }

    #[test]
    fn cron_tick_parser_accepts_only_cron_grammar() {
        assert!(matches!(
            parse_cron_tick_reply(br#"{"result":"pass-through","cron-output":[{"kind":"message","body":"kept verbatim"}]}"#),
            Ok(CronTickReply::PassThrough { output }) if output.len() == 1
        ));
        assert!(matches!(
            parse_cron_tick_reply(br#"{"result":"refused","message":"redacted diagnostic"}"#),
            Ok(CronTickReply::Refused { .. })
        ));
        for malformed_reply in [
            br#"{"result":"not-applicable","cron-output":[]}"#.as_slice(),
            br#"{"result":"handled","atm-action":[],"cron-output":[]}"#.as_slice(),
            br#"{"result":"pass-through","unknown":true}"#.as_slice(),
            br#"{"result":"pass-through","result":"handled"}"#.as_slice(),
            br#"{"result":"pass-through","cron-output":[{"kind":"message","kind":"error","body":"x"}]}"#.as_slice(),
            br#"{"result":"pass-through","cron-output":[{"kind":"message","body":""}]}"#.as_slice(),
        ] {
            malformed(parse_cron_tick_reply(malformed_reply));
        }
    }

    #[test]
    fn cron_stop_parser_requires_exact_reason_invariant_and_nested_keys() {
        assert!(matches!(
            parse_cron_stop_reply(
                br#"{"stop":true,"reason":"done","cron-output":[{"kind":"error","body":"mail"}]}"#
            ),
            Ok(CronStopReply { stop: true, .. })
        ));
        for malformed_reply in [
            br#"{"stop":true}"#.as_slice(),
            br#"{"stop":false,"reason":"no"}"#.as_slice(),
            br#"{"stop":false,"extra":true}"#.as_slice(),
            br#"{"stop":false,"cron-output":[{"kind":"message","body":"x","extra":true}]}"#
                .as_slice(),
            br#"{"stop":false,"stop":true}"#.as_slice(),
        ] {
            malformed(parse_cron_stop_reply(malformed_reply));
        }
    }
}
