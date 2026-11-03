//! The framework: the same for every profile. Policy loading, the tick record, the view, executor
//! outcomes, the process and beads ports, the hook Errors tables and the reply cap. It collects facts, calls
//! the profile's `Cron::decide`, and executes what it returns; it never decides a meaningful state change itself.
use crate::plugin::{Action, Bead, BeadStore, CheckState, CollectError, Evidence, Params, PhaseFacts, Refusal, Stage, Status, TaskState};

pub use crate::hook::{fix, Fix, Owner};
pub use crate::wire::HOOK_ACTIONS_MAX;

/// Match the journal's existing compact, sorted, ASCII-escaped JSON encoding.
fn ascii_json(value: &serde_json::Value) -> String {
    use std::fmt::Write;
    let mut result = String::new();
    for character in value.to_string().chars() {
        if character >= '\u{7f}' {
            for unit in character.encode_utf16(&mut [0; 2]).iter() {
                write!(&mut result, "\\u{unit:04x}").expect("writing to a String cannot fail");
            }
        } else {
            result.push(character);
        }
    }
    result
}

/// Encode close evidence for the beads journal; the guarded patch itself stays typed.
fn evidence_json(evidence: &Evidence) -> serde_json::Value {
    match evidence {
        Evidence::Layer(layer) => serde_json::json!({"kind": "layer", "branch": layer.branch,
            "head": layer.head.as_str(), "pr": layer.pr, "base": layer.base}),
        Evidence::QaPass(pass) => serde_json::json!({"kind": "qa_pass", "sha": pass.sha.as_str()}),
        Evidence::QaFail(fail) => serde_json::json!({"kind": "qa_fail", "sha": fail.sha.as_str(),
            "findings": fail.findings.iter().map(|f| serde_json::json!({"id": f.id, "blocking": f.blocking})).collect::<Vec<_>>()}),
        Evidence::Merged(merged) => {
            serde_json::json!({"kind": "merged", "sha": merged.sha.as_str()})
        }
    }
}

/// Encode typed close evidence at the journal boundary in the established canonical form.
pub fn encode_evidence(evidence: &Evidence) -> String {
    ascii_json(&evidence_json(evidence))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind { Assign, Report, Notify }

/// A profile's policy.toml, loaded before any read: POLICY_BAD_PARAMS (a param neither int nor text), PROFILE_MISMATCH.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy { pub profile: String, pub params: Params, pub notify_template: String }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewRow { pub queue: String, pub key: String, pub bead_id: Option<String>, pub text: String, pub since: Option<i64> }

/// One tick's `decide` result after dedupe: actions to execute, and Reports already sent kept as rows only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision { pub actions: Vec<Action>, pub rows: Vec<ViewRow> }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewStep { pub bead_id: String, pub stage: Stage, pub status: Status, pub assignee: Option<String>, pub ready: bool }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewSprint { pub id: String, pub title: String, pub status: Status, pub depends_on: Vec<String>, pub steps: Vec<ViewStep> }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewTask { pub task_id: String, pub assignee: String, pub state: TaskState, pub reminders: u32, pub since: Option<i64> }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutcomeKind { Sent, Failed, DryRun }

/// `outcome` is `None` when the action was planned, not executed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewAction { pub kind: ActionKind, pub to: String, pub bead_id: Option<String>, pub key: String, pub outcome: Option<OutcomeKind> }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhaseView {
    pub phase: String,
    pub root_id: String,
    pub root_closed: bool,
    pub lead: String,
    pub generated_at: i64,
    pub sprints: Vec<ViewSprint>,
    pub tasks: Vec<ViewTask>,
    pub rows: Vec<ViewRow>,
    pub actions: Vec<ViewAction>,
    pub top_ci: Option<CheckState>,
    pub errors: Vec<CollectError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionOutcome { pub action: Action, pub kind: OutcomeKind, pub failure: Option<Refusal> }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickResult { pub facts: PhaseFacts, pub decision: Decision, pub view: PhaseView, pub outcomes: Vec<ActionOutcome> }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandResult { pub code: i32, pub stdout: String, pub stderr: String }

/// The only process port: collectors read through it, the executor writes through it.
/// Intentionally open: a plug-in composition root may replace this framework port.
pub trait CommandRunner {
    fn run(&self, argv: &[String], stdin: Option<&str>) -> CommandResult;
}

/// The beads port: `show` (from `BeadStore`) plus the phase's listed and ready beads.
/// Intentionally open: a plug-in composition root may replace this framework port.
pub trait BeadsCache: BeadStore {
    fn list(&self, phase: &str) -> Result<Vec<Bead>, Refusal>;
    fn ready(&self, phase: &str) -> Result<Vec<Bead>, Refusal>;
}

pub struct Ports<'a> { pub runner: &'a dyn CommandRunner, pub cache: &'a dyn BeadsCache }
