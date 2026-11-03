//! The plug-in surface: task-hook types, read-only facts, `Params`, `Action`, `Cron`, `Profile`.
//! A plug-in decides; it does no I/O. Times are unix seconds.
use std::collections::BTreeMap;

use serde::Deserialize;

pub use crate::hook::{
    AssignTransition, Bead, BeadPatch, BeadStore, CloseTransition, Ctx, ErrorId, Evidence, Finding, Guard, Hook, Layer,
    Merged, PhaseFile, PhaseFiles, QaFail, QaPass, Refusal, Sha, Stack, Stage, StartTransition, Status, Template, TemplateFile,
};
pub use crate::wire::{CloseVars, HookAction, HookReply, HookRequest, Outcome};

/// A profile plug-in's manifest (`profile.toml`). Paths are relative to the manifest's directory.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    /// The team pipeline in order: which stage follows which.
    pub stages: Vec<String>,
    /// ATM hook -> the script ATM runs for it.
    pub hooks: BTreeMap<Hook, String>,
    pub templates: BTreeMap<Template, String>,
    /// Holds the cron's `params` table.
    pub policy: String,
    /// The script whose `decide` the framework calls each tick (`Cron`); none: the cron decides nothing.
    #[serde(default)]
    pub cron: Option<String>,
    /// Policy agent parameter -> startup-message parameter.
    #[serde(default)]
    pub startup: BTreeMap<String, String>,
}

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

/// One optional field per kind; `None` when its read failed this tick (see `errors`).
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

/// What a review assignment points at: repo from the phase file key `repo`; branch from the
/// dev bead; pr_number and commit from the StackFact on that branch (its pr and head).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewTarget { pub repo: String, pub pr_number: u64, pub branch: String, pub commit: String }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssignVars { pub task_id: String, pub phase: String, pub profile: String, pub review: Option<ReviewTarget> }

/// `key` dedupes: a Report whose key is in REPORTED_KEYS stays a view row only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Assign { key: String, to: String, bead_id: String, template: String, vars: AssignVars },
    Report { key: String, queue: String, bead_id: Option<String>, text: String },
    Notify { to: String, keys: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamValue { Int(i64), Text(String) }

/// The `params` table of the profile's policy.toml.
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

/// The profile's cron script, the plug-in's one cron entry point: pure, no I/O. The framework collects
/// every fact, calls `decide` once per tick, then dedupes and executes the returned actions.
/// Intentionally open: each plug-in supplies its own decision logic.
pub trait Cron {
    fn decide(&self, facts: &PhaseFacts, params: &Params) -> Vec<Action>;
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn lite_profile_parses_and_its_scripts_exist() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../lite/skills/atm-bd-lite/profile.toml");
        let text = std::fs::read_to_string(&path).expect("profile.toml readable");
        let profile: Profile = toml::from_str(&text).expect("profile.toml parses");
        let dir = path.parent().expect("profile dir");
        assert_eq!(profile.hooks.len(), 3, "one script per ATM task hook");
        let cron = profile.cron.as_ref().expect("profile names a cron script");
        for script in profile.hooks.values().chain([cron]) {
            assert!(dir.join(script).is_file(), "{script} named by profile.toml does not exist");
        }
    }
}
