//! Monitor surface: collectors -> facts -> pure rules -> actions -> executor -> view.
//! Times are unix seconds. Errors are `hook::Refusal` (policy load, cache) or `CollectError` (a failed read).
use crate::hook::{Bead, PhaseFile, Refusal, Sha, Stack, Stage, Status};

/// One member per collector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FactKind { Tasks, TaskEvents, ReportedKeys, RemoteBranches, Ancestry, Beads, Ready, Stack, TopChecks, PrBases }

/// ATM task state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState { Assigned, Active, Complete }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskFact { pub task_id: String, pub assignee: String, pub state: TaskState, pub bead_id: Option<String> }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskEvents { pub task_id: String, pub reminders_since_start: u32, pub last_start: Option<i64> }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteBranch { pub name: String, pub head: Sha }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ancestry { pub below: String, pub above: String, pub is_ancestor: bool }
/// The stage:stack bead, its ledger, trunk, and since when its assignee holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackFact { pub bead: Bead, pub stack: Stack, pub trunk: String, pub since: Option<i64> }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckState { Passing, Failing, Pending, None }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopChecks { pub head: Sha, pub state: CheckState }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrBase { pub pr: u64, pub base: String }

/// A read that failed: its fact is `None` this tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectError { pub kind: FactKind, pub argv: Vec<String>, pub message: String, pub fix: String }

/// One optional field per kind; only the kinds a policy needs are read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhaseFacts {
    pub phase: String,
    pub now: i64,
    pub phase_file: PhaseFile,
    pub tasks: Option<Vec<TaskFact>>,
    pub task_events: Option<Vec<TaskEvents>>,
    pub reported_keys: Option<Vec<String>>,
    pub remote_branches: Option<Vec<RemoteBranch>>,
    pub ancestry: Option<Vec<Ancestry>>,
    pub beads: Option<Vec<Bead>>,
    pub ready: Option<Vec<Bead>>,
    pub stack: Option<StackFact>,
    pub top_checks: Option<TopChecks>,
    pub pr_bases: Option<Vec<PrBase>>,
    pub errors: Vec<CollectError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssignVars { pub task_id: String, pub phase: String, pub profile: String }

/// `key` dedupes: a Report whose key is in REPORTED_KEYS stays a view row only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Assign { key: String, to: String, bead_id: String, template: String, vars: AssignVars },
    Report { key: String, queue: String, bead_id: Option<String>, text: String },
    Notify { to: String, keys: Vec<String> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind { Assign, Report, Notify }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamKind { Int, Text }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParamSpec { pub name: String, pub kind: ParamKind }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamValue { Int(i64), Text(String) }

/// A rule's validated `params` table from policy.toml.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Params { pub values: Vec<(String, ParamValue)> }

impl Params {
    pub fn int(&self, name: &str) -> Option<i64> {
        self.values.iter().find_map(|(k, v)| match v { ParamValue::Int(i) if k == name => Some(*i), _ => None })
    }
    pub fn text(&self, name: &str) -> Option<&str> {
        self.values.iter().find_map(|(k, v)| match v { ParamValue::Text(t) if k == name => Some(t.as_str()), _ => None })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleEntry { pub rule_id: String, pub params: Params }

/// A profile's policy.toml, validated before any read (POLICY_UNKNOWN_RULE, POLICY_NO_COLLECTOR,
/// POLICY_BAD_PARAMS, PROFILE_MISMATCH, each naming the rule id).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy { pub profile: String, pub rules: Vec<RuleEntry>, pub notify_template: String, pub needs: Vec<FactKind> }

/// A pure decision: no I/O. A rule whose needed fact is `None` is not called.
pub trait Rule {
    fn id(&self) -> &str;
    fn needs(&self) -> Vec<FactKind>;
    fn params(&self) -> Vec<ParamSpec>;
    fn decide(&self, facts: &PhaseFacts, params: &Params) -> Vec<Action>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewRow { pub queue: String, pub key: String, pub bead_id: Option<String>, pub text: String, pub since: Option<i64> }

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
pub struct ActionOutcome { pub action: Action, pub kind: OutcomeKind, pub failure: Option<String> }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickResult { pub facts: PhaseFacts, pub decision: Decision, pub view: PhaseView, pub outcomes: Vec<ActionOutcome> }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandResult { pub code: i32, pub stdout: String, pub stderr: String }

/// The only process port: collectors read through it, the executor writes through it.
pub trait CommandRunner {
    fn run(&self, argv: &[String], stdin: Option<&str>) -> CommandResult;
}

/// The beads port: `show` (from `BeadStore`) plus the phase's listed and ready beads.
pub trait BeadsCache: crate::hook::BeadStore {
    fn list(&self, phase: &str) -> Result<Vec<Bead>, Refusal>;
    fn ready(&self, phase: &str) -> Result<Vec<Bead>, Refusal>;
}

pub struct Ports<'a> { pub runner: &'a dyn CommandRunner, pub cache: &'a dyn BeadsCache }
