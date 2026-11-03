//! ATM's task-hook wire: atm-core `hook/request.rs` HookRequest in, `hook/reply.rs` HookReply::parse out.
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

use crate::hook::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome { Completed, Refused, Cancelled }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum TaskOp { Start, Close { outcome: Outcome, #[serde(default, skip_serializing_if = "Option::is_none")] reason: Option<String> } }

#[derive(Clone, Debug, Deserialize)]
enum Source { Inline(String), File(IgnoredAny), Template(IgnoredAny) }

#[derive(Clone, Debug, Deserialize)]
struct Address { agent: String }

/// The fields of ATM's task write request (`send::WriteRequest`) a hook reads or echoes.
#[derive(Clone, Debug, Deserialize)]
struct Body {
    home_dir: String,
    current_dir: String,
    caller_team: String,
    #[serde(default)] to: Option<Address>,
    message_source: Source,
    task_id: Option<String>,
    #[serde(default)] task_op: Option<TaskOp>,
    #[serde(default)] dry_run: bool,
}

#[derive(Deserialize)]
struct Caller { identity: String }

#[derive(Deserialize)]
struct Envelope { hook: String, phase: String, caller: Caller, request: Body }

#[derive(Clone, Debug)]
pub struct HookRequest { hook: Hook, phase: String, caller: String, task_id: String, body: Body }

fn invalid(field: &str) -> Refusal { Refusal::new(ErrorId::RequestInvalid).with("field", Ctx::One(field.to_owned())) }
fn bad_verdict() -> Refusal { Refusal::new(ErrorId::QaVerdict) }

enum Verdict { Pass(QaPass), Findings(QaFail) }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVerdict { verdict: String, sha: String, #[serde(default)] findings: Vec<RawFinding> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFinding { id: String, blocking: bool }

impl HookRequest {
    pub fn parse(stdin: &str) -> Result<Self, Refusal> {
        let env: Envelope = serde_json::from_str(stdin).map_err(|_| invalid("request"))?;
        let hook = match env.hook.as_str() {
            "task.assign" => Hook::TaskAssign,
            "task.start" => Hook::TaskStart,
            "task.close" => Hook::TaskClose,
            _ => return Err(invalid("hook")),
        };
        let task_id = env.request.task_id.clone().ok_or_else(|| invalid("task_id"))?;
        Ok(HookRequest { hook, phase: env.phase, caller: env.caller.identity, task_id, body: env.request })
    }
    pub fn hook(&self) -> Hook { self.hook }
    /// ATM calls a hook only for a phase it is registered in.
    pub fn is_phase_task(&self) -> bool { !self.phase.is_empty() }
    pub fn phase(&self) -> &str { &self.phase }
    pub fn phase_config_path(&self) -> String { format!(".atm-bd/phase-{}.toml", self.phase) }
    pub fn caller(&self) -> &str { &self.caller }
    pub fn task_id(&self) -> &str { &self.task_id }
    pub fn assignee(&self) -> Option<&str> { self.body.to.as_ref().map(|a| a.agent.as_str()) }
    pub fn outcome(&self) -> Option<Outcome> {
        match &self.body.task_op { Some(TaskOp::Close { outcome, .. }) => Some(*outcome), _ => None }
    }
    fn inline(&self) -> Option<&str> {
        match &self.body.message_source { Source::Inline(t) if !t.trim().is_empty() => Some(t), _ => None }
    }
    pub fn close_report(&self) -> Option<&str> { self.inline() }
    pub fn close_reason(&self) -> Option<&str> {
        match &self.body.task_op { Some(TaskOp::Close { reason: Some(r), .. }) if !r.trim().is_empty() => Some(r), _ => self.inline() }
    }

    fn verdict(&self) -> Result<Verdict, Refusal> {
        let report = self.close_report().unwrap_or("");
        let blocks: Vec<RawVerdict> = report.split("```").skip(1).step_by(2)
            .filter_map(|b| serde_json::from_str(b.strip_prefix("json").unwrap_or(b).trim()).ok()).collect();
        let [raw] = blocks.as_slice() else { return Err(bad_verdict()) };
        let sha = Sha::parse(&raw.sha).ok_or_else(bad_verdict)?;
        let findings = raw.findings.iter().map(|f| Finding { id: f.id.clone(), blocking: f.blocking }).collect();
        match raw.verdict.as_str() {
            "PASS" => Ok(Verdict::Pass(QaPass { sha })),
            "FINDINGS" => Ok(Verdict::Findings(QaFail { sha, findings })),
            _ => Err(bad_verdict()),
        }
    }
    pub fn qa_pass(&self) -> Result<QaPass, Refusal> {
        match self.verdict()? { Verdict::Pass(p) => Ok(p), Verdict::Findings(_) => Err(bad_verdict()) }
    }
    pub fn qa_findings(&self) -> Result<QaFail, Refusal> {
        match self.verdict()? { Verdict::Findings(f) => Ok(f), Verdict::Pass(_) => Err(bad_verdict()) }
    }
}

impl CloseTransition {
    /// outcome x stage (x verdict kind for QA).
    pub fn select(req: &HookRequest, bead: &Bead) -> Result<Self, Refusal> {
        match (req.outcome().ok_or_else(|| invalid("outcome"))?, bead.stage) {
            (Outcome::Refused | Outcome::Cancelled, _) => Ok(Self::GiveBack),
            (Outcome::Completed, Stage::Dev | Stage::Fix) => Ok(Self::WorkClose),
            (Outcome::Completed, Stage::Qa) => match req.verdict()? { Verdict::Pass(_) => Ok(Self::QaPass), Verdict::Findings(_) => Ok(Self::QaFindings) },
            (Outcome::Completed, Stage::Stack | Stage::Other) => Err(invalid("stage")),
        }
    }
}

/// `--vars` of the close action: a layer, a sha, a sha plus finding ids, or the give-back reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum CloseVars { Layer(Layer), Sha { sha: Sha }, ShaIds { sha: Sha, ids: Vec<String> }, Reason { reason: String } }

impl From<&Evidence> for CloseVars {
    fn from(e: &Evidence) -> Self {
        match e {
            Evidence::Layer(l) => CloseVars::Layer(l.clone()),
            Evidence::QaPass(p) => CloseVars::Sha { sha: p.sha.clone() },
            Evidence::QaFail(f) => CloseVars::ShaIds { sha: f.sha.clone(), ids: f.findings.iter().map(|x| x.id.clone()).collect() },
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct Empty {}

#[derive(Clone, Debug, Serialize)]
struct TemplateSource {
    canonical_template_path: String,
    canonical_template_root: String,
    raw_file_bytes: Vec<u8>,
    input_defaults: Empty,
    var_file_values: CloseVars,
    explicit_values: Empty,
    environment_values: Empty,
}

#[derive(Clone, Debug, Serialize)]
enum MessageSource { Template(TemplateSource) }

/// ATM `task.close` write request: the caller's own close, re-sent with a template.
#[derive(Clone, Debug, Serialize)]
struct CloseBody {
    home_dir: String,
    current_dir: String,
    caller_identity: String,
    caller_team: String,
    message_source: MessageSource,
    requires_ack: bool,
    task_id: String,
    task_op: TaskOp,
    dry_run: bool,
}

/// One `atm-action` entry: `atm task close <task> <outcome> --template <t> --vars <vars>`.
#[derive(Clone, Debug, Serialize)]
pub struct HookAction { command: &'static str, request: CloseBody }

impl HookAction {
    pub fn close(req: &HookRequest, template: &TemplateFile, vars: CloseVars) -> Self {
        let b = &req.body;
        let source = TemplateSource {
            canonical_template_path: template.path.clone(), canonical_template_root: template.root.clone(),
            raw_file_bytes: template.bytes.clone(), input_defaults: Empty {}, var_file_values: vars,
            explicit_values: Empty {}, environment_values: Empty {},
        };
        HookAction { command: "task.close", request: CloseBody {
            home_dir: b.home_dir.clone(), current_dir: b.current_dir.clone(), caller_identity: req.caller.clone(),
            caller_team: b.caller_team.clone(), message_source: MessageSource::Template(source), requires_ack: false,
            task_id: req.task_id.clone(), task_op: TaskOp::Close { outcome: req.outcome().unwrap_or(Outcome::Completed), reason: None },
            dry_run: b.dry_run,
        } }
    }
}

/// atm-core caps `atm-action` at 1..=32 entries.
pub const HOOK_ACTIONS_MAX: usize = 32;

#[derive(Clone, Debug)]
pub enum HookReply { NotApplicable, PassThrough, Refused(Refusal), Handled(Vec<HookAction>) }

#[derive(Serialize)]
#[serde(tag = "result", rename_all = "kebab-case")]
enum Wire<'a> { Handled { #[serde(rename = "atm-action")] actions: &'a [HookAction] }, PassThrough, NotApplicable, Refused { message: String } }

impl HookReply {
    pub fn not_applicable() -> Self { Self::NotApplicable }
    pub fn pass_through() -> Self { Self::PassThrough }
    pub fn refused(refusal: Refusal) -> Self { Self::Refused(refusal) }
    pub fn handled(actions: Vec<HookAction>) -> Self { Self::Handled(actions) }

    /// Exactly one of the four objects `HookReply::parse` accepts; `hook` selects the fix table.
    pub fn to_json(&self, hook: Option<Hook>) -> String {
        let wire = match self {
            Self::NotApplicable => Wire::NotApplicable,
            Self::PassThrough => Wire::PassThrough,
            Self::Refused(r) => Wire::Refused { message: r.message(hook) },
            Self::Handled(a) if (1..=HOOK_ACTIONS_MAX).contains(&a.len()) => Wire::Handled { actions: a },
            Self::Handled(_) => Wire::Refused { message: invalid("atm-action").message(hook) },
        };
        serde_json::to_string(&wire).unwrap_or_else(|_| r#"{"result":"refused","message":"REQUEST_INVALID: field=reply"}"#.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn close_request(report: &str) -> HookRequest {
        let stdin = serde_json::json!({
            "schema_version": "1.0.0", "hook": "task.close", "team": "t", "phase": "bo",
            "caller": {"identity": "qa", "team": "t", "host": null}, "custom": {},
            "request": {"home_dir": "/h", "current_dir": "/r", "caller_identity": "qa", "caller_team": "t",
                "to": null, "message_source": {"Inline": report}, "requires_ack": false, "task_id": "bo-3",
                "task_op": {"op": "close", "outcome": "completed"}, "dry_run": false}
        });
        HookRequest::parse(&stdin.to_string()).expect("fixture decodes")
    }

    #[test]
    fn reply_pass_through() { assert_eq!(HookReply::pass_through().to_json(None), r#"{"result":"pass-through"}"#); }

    #[test]
    fn reply_not_applicable() { assert_eq!(HookReply::not_applicable().to_json(None), r#"{"result":"not-applicable"}"#); }

    #[test]
    fn reply_refused() {
        let r = Refusal::new(ErrorId::BeadNotReady).with("bead", Ctx::One("bo-1".into())).with("ids", Ctx::Many(vec!["bo-2".into(), "bo-3".into()]));
        assert_eq!(HookReply::refused(r).to_json(Some(Hook::TaskStart)),
            r#"{"result":"refused","message":"BEAD_NOT_READY: bo-1 is blocked by bo-2, bo-3. Wait for the lead."}"#);
    }

    #[test]
    fn reply_handled() {
        let req = close_request(&format!("done\n```json\n{{\"verdict\": \"PASS\", \"sha\": \"{SHA}\"}}\n```\n"));
        let pass = req.qa_pass().expect("one PASS block");
        let t = TemplateFile { path: "/r/t/qa-pass.md.j2".into(), root: "/r/t".into(), bytes: b"ok".to_vec() };
        let reply = HookReply::handled(vec![HookAction::close(&req, &t, CloseVars::from(&Evidence::QaPass(pass)))]);
        assert_eq!(reply.to_json(Some(req.hook())), format!(concat!(
            r#"{{"result":"handled","atm-action":[{{"command":"task.close","request":{{"home_dir":"/h","current_dir":"/r","#,
            r#""caller_identity":"qa","caller_team":"t","message_source":{{"Template":{{"canonical_template_path":"/r/t/qa-pass.md.j2","#,
            r#""canonical_template_root":"/r/t","raw_file_bytes":[111,107],"input_defaults":{{}},"var_file_values":{{"sha":"{}"}},"#,
            r#""explicit_values":{{}},"environment_values":{{}}}}}},"requires_ack":false,"task_id":"bo-3","#,
            r#""task_op":{{"op":"close","outcome":"completed"}},"dry_run":false}}}}]}}"#), SHA));
    }
}
