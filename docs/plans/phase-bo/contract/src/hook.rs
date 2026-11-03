//! Task-hook surface (hooks/task-*.md): beads, transitions, guarded writes, typed refusals.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status { Open, InProgress, Blocked, Deferred, Closed }

/// The bead's `stage:` label; an unknown label decodes to `Other`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage { Dev, Fix, Qa, Land, Stack, Other }

/// A 40-hex commit id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Sha(String);

impl Sha {
    pub fn parse(text: &str) -> Option<Sha> {
        (text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit())).then(|| Sha(text.to_owned()))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

macro_rules! error_ids {
    ($($id:ident = $code:literal),* $(,)?) => {
        /// One id per failure, both surfaces. Hook rows: hooks/*.md Errors tables; POLICY_BAD_PARAMS: policy load.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum ErrorId { $($id),* }
        impl ErrorId {
            pub const fn code(self) -> &'static str { match self { $(Self::$id => $code),* } }
        }
    };
}

error_ids! {
    RequestInvalid = "REQUEST_INVALID", PhaseUnreadable = "PHASE_UNREADABLE", ProfileMismatch = "PROFILE_MISMATCH",
    CacheUnavailable = "CACHE_UNAVAILABLE", BeadNotFound = "BEAD_NOT_FOUND", BeadNotReady = "BEAD_NOT_READY",
    BeadClosed = "BEAD_CLOSED", BeadRaced = "BEAD_RACED", AssignNotLead = "ASSIGN_NOT_LEAD",
    TaskNotAssignee = "TASK_NOT_ASSIGNEE", CloseReasonEmpty = "CLOSE_REASON_EMPTY",
    CloseReportNotInline = "CLOSE_REPORT_NOT_INLINE", StackNotRecorded = "STACK_NOT_RECORDED",
    QaVerdict = "QA_VERDICT", QaNotOnStack = "QA_NOT_ON_STACK", QaNothingToReview = "QA_NOTHING_TO_REVIEW",
    FindingsNotPoured = "FINDINGS_NOT_POURED", PolicyBadParams = "POLICY_BAD_PARAMS", LandEvidence = "LAND_EVIDENCE",
}

/// The ATM task hook a request came from (selects the fix table); a profile registers one script per hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum Hook {
    #[serde(rename = "task.assign")] TaskAssign,
    #[serde(rename = "task.start")] TaskStart,
    #[serde(rename = "task.close")] TaskClose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner { Dev, Qa, Lead }

/// One Errors-table row: owner and fix text; alternatives are tried in order (first fully filled wins).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fix { pub owner: Owner, pub text: &'static [&'static str] }

const fn row(owner: Owner, text: &'static [&'static str]) -> Fix { Fix { owner, text } }

/// The Errors tables of hooks/task-assign.md, task-start.md and task-close.md, verbatim.
pub fn fix(hook: Hook, id: ErrorId) -> Fix {
    use {ErrorId::*, Hook::*, Owner::*};
    match (hook, id) {
        (TaskAssign, AssignNotLead) => row(Lead, &["Only the lead (<lead>) assigns. Ask the lead."]),
        (TaskAssign, BeadNotReady) => row(Lead, &["<bead> is blocked by <ids>. Close the blockers first.", "<bead> is <status>. Close the blockers first."]),
        (TaskAssign, BeadClosed) => row(Lead, &["<bead> is closed. Nothing to assign."]),
        (TaskAssign, _) => row(Lead, &["<what failed>. Retry or fix the phase."]),
        (TaskStart, TaskNotAssignee) => row(Dev, &["<bead> is assigned to <agent>, not you. Start only your own task."]),
        (TaskStart, BeadNotReady) => row(Dev, &["<bead> is blocked by <ids>. Wait for the lead.", "<bead> is <status>. Wait for the lead."]),
        (TaskStart, BeadClosed) => row(Dev, &["<bead> is closed. Nothing to start."]),
        (TaskClose, StackNotRecorded) => row(Dev, &["<branch> is not on the ledger. Run the stack block in your task, then close again."]),
        (TaskClose, CloseReportNotInline | CloseReasonEmpty) => row(Dev, &["Run `atm task close <bead> <outcome> --stdin` with the report (or reason)."]),
        (TaskClose, TaskNotAssignee) => row(Dev, &["<bead> is assigned to <agent>, not you."]),
        (TaskClose, BeadClosed) => row(Dev, &["<bead> is already closed."]),
        (TaskClose, BeadNotReady) => row(Dev, &["<bead> is <status>; `atm task start` it first."]),
        (TaskClose, QaVerdict) => row(Qa, &["End the report with one fenced block {\"verdict\": PASS or FINDINGS, \"sha\": …, \"findings\": [{\"id\": …, \"blocking\": true or false}]}."]),
        (TaskClose, LandEvidence) => row(Lead, &["Include one fenced JSON block with merged and a 40-hex merge SHA."]),
        (TaskClose, QaNotOnStack) => row(Qa, &["Your verdict is for <sha>; <branch> is at <head>. Review <head>, then close again."]),
        (TaskClose, FindingsNotPoured) => row(Qa, &["Findings <ids> are not poured or do not block <sprint>. Run the pour block, then close again."]),
        (TaskStart | TaskClose, _) => row(Lead, &["Not yours to fix: <what failed>. The lead has it."]),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ctx { One(String), Many(Vec<String>) }

impl Ctx {
    fn text(&self) -> String { match self { Ctx::One(s) => s.clone(), Ctx::Many(v) => v.join(", ") } }
}

/// A typed refusal: an id plus the named values its fix text fills (`<bead>`, `<ids>`, ...).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal { pub id: ErrorId, pub context: Vec<(String, Ctx)> }

impl Refusal {
    pub fn new(id: ErrorId) -> Self { Refusal { id, context: Vec::new() } }
    pub fn with(mut self, key: &str, value: Ctx) -> Self { self.context.push((key.to_owned(), value)); self }

    /// `<CODE>: <fix>` for a hook; `<CODE>: <what failed>` without one (request parse, monitor).
    pub fn message(&self, hook: Option<Hook>) -> String {
        let failed = self.context.iter().map(|(k, v)| format!("{k}={}", v.text())).collect::<Vec<_>>().join(", ");
        let Some(hook) = hook else { return format!("{}: {}", self.id.code(), failed) };
        let alts = fix(hook, self.id).text;
        let text = alts.iter().find_map(|t| self.fill(t, &failed)).unwrap_or_else(|| alts.last().map_or(String::new(), |t| (*t).to_owned()));
        format!("{}: {text}", self.id.code())
    }

    fn fill(&self, text: &str, failed: &str) -> Option<String> {
        let (mut out, mut rest) = (String::new(), text);
        while let Some((i, j)) = rest.find('<').and_then(|i| rest[i..].find('>').map(|j| (i, i + j))) {
            let key = &rest[i + 1..j];
            out.push_str(&rest[..i]);
            out.push_str(&if key == "what failed" { failed.to_owned() } else { self.context.iter().find(|(k, _)| k == key)?.1.text() });
            rest = &rest[j + 1..];
        }
        out.push_str(rest);
        Some(out)
    }
}

/// One stack ledger layer; also the WORK_CLOSE evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Layer { pub branch: String, pub head: Sha, pub pr: u64, pub base: String }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stack { pub layers: Vec<Layer> }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bead {
    pub id: String,
    pub status: Status,
    pub stage: Stage,
    pub assignee: Option<String>,
    pub open_blockers: Vec<String>,
    /// Ids of the beads this bead blocks.
    pub blocks_ids: Vec<String>,
    pub branch: String,
    pub stack_bead: String,
    pub sprint: String,
    pub phase: String,
    pub checked_bead: Option<String>,
    /// Set on the stage:stack bead only.
    pub stack: Option<Stack>,
}

impl Bead {
    pub fn blocks(&self, bead_id: &str) -> bool { self.blocks_ids.iter().any(|b| b == bead_id) }
    pub fn layer_for(&self, branch: &str) -> Option<&Layer> {
        self.stack.as_ref()?.layers.iter().find(|l| l.branch == branch)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding { pub id: String, pub blocking: bool }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QaPass { pub sha: Sha }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QaFail { pub sha: Sha, pub findings: Vec<Finding> }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merged { pub sha: String }

/// What a close records on the bead (T-CLOSE-02..04).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evidence { Layer(Layer), QaPass(QaPass), QaFail(QaFail), Merged(Merged) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignTransition { AssignSame, Assign, AssignClosed, AssignNotReady }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartTransition { Start, StartAgain, StartClosed, StartNotReady }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseTransition { GiveBack, WorkClose, QaPass, QaFindings, Land }

impl AssignTransition {
    pub fn select(status: Status, assignee: Option<&str>, wanted: &str) -> Self {
        match status {
            Status::Closed => Self::AssignClosed,
            Status::Blocked | Status::Deferred => Self::AssignNotReady,
            Status::Open | Status::InProgress if assignee == Some(wanted) => Self::AssignSame,
            Status::Open | Status::InProgress => Self::Assign,
        }
    }
}

impl StartTransition {
    pub fn select(status: Status) -> Self {
        match status {
            Status::Open => Self::Start,
            Status::InProgress => Self::StartAgain,
            Status::Closed => Self::StartClosed,
            Status::Blocked | Status::Deferred => Self::StartNotReady,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Template { Refuse, DevClose, QaPass, QaFindings, LandClose }

/// A template the phase file names, read at load (the daemon renders from these bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateFile { pub path: String, pub root: String, pub bytes: Vec<u8> }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhaseFile {
    pub profile: String,
    pub lead: String,
    /// Indexed by `Template as usize`.
    pub templates: [TemplateFile; 5],
    /// Other top-level string keys (`root`, `integration_branch`, keys a policy param names).
    pub keys: Vec<(String, String)>,
}

impl PhaseFile {
    pub fn template(&self, which: Template) -> &TemplateFile { &self.templates[which as usize] }
    pub fn key(&self, name: &str) -> Option<&str> { self.keys.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()) }
}

/// The snapshot a guarded write compares: applied only if status and assignee are unchanged, else BEAD_RACED.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Guard { pub bead_id: String, pub status: Status, pub assignee: Option<String> }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BeadPatch {
    Assign { guard: Guard, agent: String },
    Start { guard: Guard },
    Reopen { guard: Guard },
    Close { guard: Guard, evidence: Evidence },
}

impl BeadPatch {
    fn guard(b: &Bead) -> Guard { Guard { bead_id: b.id.clone(), status: b.status, assignee: b.assignee.clone() } }
    pub fn assign(snapshot: &Bead, agent: &str) -> Self { Self::Assign { guard: Self::guard(snapshot), agent: agent.to_owned() } }
    pub fn start(snapshot: &Bead) -> Self { Self::Start { guard: Self::guard(snapshot) } }
    pub fn reopen(snapshot: &Bead) -> Self { Self::Reopen { guard: Self::guard(snapshot) } }
    pub fn close(snapshot: &Bead, evidence: Evidence) -> Self { Self::Close { guard: Self::guard(snapshot), evidence } }
}

/// Reads and the one guarded write a hook makes. The beads-cache adapter implements it.
pub trait BeadStore {
    fn show(&self, bead_id: &str) -> Result<Bead, Refusal>;
    /// The ledger layer of the QA bead's closed dev/fix blocker, or of its `checked_bead`.
    fn target_layer(&self, qa_bead: &Bead) -> Result<Layer, Refusal>;
    fn update(&self, patch: &BeadPatch, actor: &str) -> Result<(), Refusal>;
}

/// Loads `.atm-bd/phase-<x>.toml`; PHASE_UNREADABLE, or PROFILE_MISMATCH when `profile` differs.
pub trait PhaseFiles {
    fn load(&self, path: &str, profile: &str) -> Result<PhaseFile, Refusal>;
}
