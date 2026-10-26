# Sprint bk-8 — per-member max_tasks: several active tasks, started-first order, one nudge for all open tasks

closure_type `integration`; target_boundary `BOUNDARY-TaskStore-Sqlite`
(task start limit, queue order, `one_active_task_per_agent` dropped) with its
two consumers: `atm teams update-member --max-tasks` (roster
`metadata_json`) and the Herdr task-pass nudge; branch
`sprint/bk-8-max-tasks`, PR → `integrate/phase-bk` (planned layer 8);
difficulty normal. `parallel_safe`: it consumes no API or artefact from
bk-1..bk-7 and shares no file with them (checked against every bk sprint's
owned paths). bk-7 routes task commands through hooks in
`storage_and_nudge_router.rs`; it never edits the task writer
(`writer/task_start.rs`, `writer/task_ops.rs`) or the Herdr task pass, and
bk-4 is the new `atm-cron` crate, so neither touches these files. bk-8 adds no
field to `PostSendHookEvent` (bk-2's `boundary/mod.rs`, constructed in
bk-7's `storage_and_nudge_router.rs`). Tracker: GitHub #1932. Governing
text: `docs/requirements.md` §15 "Task lifecycle (Phase BA)" items 3, 4, 6
(no REQ id covers them); ADR-062 (identity and constraints table,
reminder cycle); ADR-061 D2/D3 (storage change, approved below).

## Anchors (Rand, verbatim, atm-core #1932)

Request (Rand, 2026-10-20):

> sanity and quality-mgr should execute up to 3 tasks in parallel or they become bottleneck. yes, atm doesn't allow opening more than one in parallel.
> we should set this either by role or by team member i.e. max-tasks field which would default to 1.
> nudge logic would simple list 'all open tasks' in a single nudge.

Rulings (Rand, 2026-10-20):

> bead-queues and console aren't part of atm-core. they could pull task state (which they probably need to do anyway)
> member stalled would be notified for ALL tasks in a single nudge.

Simplified scope (agreed with Rand, 2026-10-20), authoritative:

> 1. `max_tasks` lives only in roster member `metadata_json`; no typed HTTP API field (no `HTTP_API_VERSION` bump), no doctor change.
> 2. Nudge, reminder and stall/escalation state are per member: one nudge lists all of the member's open tasks.
> 3. Ordering: started tasks sort ahead of all unstarted tasks; each group keeps its order. Starting a task moves it into the started group. No other placement rules.
> 4. Rollback: drop `one_active_task_per_agent`; no downgrade compatibility. Release note: before running an older daemon, close each member's extra active tasks.
> 5. `atm teams update-member --max-tasks <N>` (optional flag). Default 1 when unset. `0` is rejected with a validation error (minimum 1).

## Scope boundary

Change points, verified on `origin/develop` (6b52831ad) and
`origin/integrate/phase-bk` (94bfc9044); the files are identical on both.
Owned paths (all edited):

- `crates/atm-storage/src/task_state.rs` (two constants; 501 production lines)
- `crates/atm-storage-rusqlite/src/task_store.rs` (`TASK_INDEX_DDL` at
  `:69-78`, unique index at `:70-71`; applied by `ensure_schema` `:90-100`
  and by `task_migration.rs:226`, which reads the same constant and is not
  edited; 735 production lines)
- `crates/atm-storage-rusqlite/src/writer/task_start.rs`
  (`reject_concurrent_active_task` `:107-134`; head insert `:42-44`; 128
  lines)
- `crates/atm-storage-rusqlite/src/writer/task_ops.rs` (active move
  `:572-581`; `MoveTarget::Head` `:718-729`; 928 production lines, stays
  under 1000)
- `crates/atm-storage-rusqlite/tests/task_identity.rs` (`:733-742`
  `start_while_another_task_is_active_is_rejected`; new tests)
- `crates/atm-core/src/team_admin/member_mutation.rs`
  (`UpdateMemberRequest` `:141-160`, `apply_member_metadata_update`
  `:672-709`)
- `crates/atm-core/src/team_admin.rs` (the seven `UpdateMemberRequest { .. }`
  test literals `:845-1554` gain `max_tasks: None`; new tests beside
  `update_member_with_host_round_trips_through_the_member_projection`
  `:1290`)
- `crates/atm/src/commands/teams.rs` (`UpdateMemberCommand` `:112`,
  `build_request` `:535-580`; tests beside `:1168`)
- `crates/atm-core/src/send/nudge_template.rs` (`render_values` `:67-116`,
  `validate_built_in_nudge_template_body` `:48`, `default_template` `:118`)
- `crates/atm-core/src/send/hook.rs` (`build_built_in_dispatch` `:19`,
  `build_herdr_dispatch` `:84`, `render_built_in_nudge_for_dispatch` `:138`)
- `crates/atm-core/src/nudge_dispatch.rs` (`build_task_reminder_dispatch`
  `:170-202`)
- `crates/atm-core/tests/task_reminder_dispatch.rs` (four call sites `:63`,
  `:102`, `:134`, `:161`)
- `crates/atm-http-runtime/src/herdr_queue_wake/task_pass.rs`
  (`open_task_heads` `:327-358`, `process_task_candidate` `:184`,
  `apply_task_disposition` `:266`, `emit_task_reminder` `:361-435`)
- `crates/atm-http-runtime/src/herdr_queue_wake/tests/herdr_nudge_invariant.rs`
  (call site `:2109`; new tests)
- `docs/requirements.md` (§15 items 3 `:3059`, 4 `:3066`, 6 `:3072`)
- `docs/adr/ADR-062-task-state-machine.md` (`:68`, table row `:125`, new
  amendment)
- `CHANGELOG.md` (Unreleased: feature and rollback release note)

`crates/atm-http-runtime/src/herdr_task_disposition.rs` (`dispose`
`:43-93`) is not edited: it already decides one action per member from one
`head` row, which stays the member's bookkeeping row (below).

## Public contract

```rust
// crates/atm-storage/src/task_state.rs
pub const MAX_TASKS_METADATA_KEY: &str = "max_tasks";      // key in team_roster.metadata_json
pub const DEFAULT_MAX_TASKS: MaxTasks = MaxTasks::DEFAULT; // 1, used when the key is absent

// crates/atm-core/src/team_admin/member_mutation.rs
pub struct UpdateMemberRequest { /* existing fields */ pub max_tasks: Option<MaxTasks> } // None = leave unchanged
impl UpdateMemberRequest {
    /// `Some(0)` -> AtmError::validation (ATM_MESSAGE_VALIDATION_FAILED):
    /// "--max-tasks must be at least 1"; `None` leaves it unchanged.
    pub fn with_max_tasks(self, max_tasks: Option<u32>) -> Result<Self, AtmError>;
}
// apply_member_metadata_update: Some(n) -> metadata_json["max_tasks"] = json!(n.get())

// crates/atm/src/commands/teams.rs, UpdateMemberCommand
/// Maximum number of tasks this member may have active at once (minimum 1; default 1 when never set).
#[arg(long = "max-tasks")]
max_tasks: Option<u32>,
// build_request: ..?.with_host(..)?.with_max_tasks(self.max_tasks)

// crates/atm-core/src/nudge_dispatch.rs (public signature change; three callers)
pub fn build_task_reminder_dispatch(
    runtime: &LocalServiceRuntime,
    member: &MemberKey,
    head: &TaskRow,          // the member's bookkeeping row
    open_tasks: &[TaskId],   // every open task of the member, queue order
) -> Result<Option<BuiltInPostSendDispatch>, AtmError>;

// crates/atm-core/src/send/hook.rs: build_built_in_dispatch keeps its signature and
// calls the new pub(crate) build_built_in_dispatch_with_open_tasks(.., open_tasks: &[TaskId]);
// render_built_in_nudge_for_dispatch gains the open_tasks argument.
// crates/atm-core/src/send/nudge_template.rs: render_values(event, open_tasks) adds the
// placeholder `open_tasks` = the ids joined by one space; public render functions pass &[].
```

Storage (`task_store.rs`): `TASK_INDEX_DDL` loses the
`one_active_task_per_agent` statement and starts with
`DROP INDEX IF EXISTS one_active_task_per_agent;`, so `ensure_schema` and the
BA migration both leave a database without it. `tasks_position_per_member`
and `tasks_open_by_member` are unchanged.

Start limit (`task_start.rs`, replaces `reject_concurrent_active_task`):

```rust
fn reject_over_max_tasks(record: &Message, task_id: &TaskId, row: &TaskRow,
    connection: &Connection, target: &SharedDbTarget) -> Result<u32 /* other active */, AtmError>;
// active = SELECT COUNT(*) FROM tasks WHERE team=?1 AND assignee=?2 AND state='active' AND task_id<>?3
// max    = SELECT json_extract(metadata_json, '$.max_tasks') FROM team_roster
//          WHERE team_name=?1 AND agent_name=?2   -- no row or NULL -> DEFAULT_MAX_TASKS
//          an integer >= 1 -> that value; anything else -> AtmError::validation naming the
//          member and `max_tasks`, recovery "set it with `atm teams update-member <team> <member> --max-tasks <N>`"
// active >= max -> task_max_active_reached(
//     "task {task_id}: {assignee} already has {active} active task(s); max_tasks is {max}",
//     "Complete or cancel an active task, or raise the limit with `atm teams update-member <team> <member> --max-tasks <N>`."
// )
```

Order (simplified scope item 3): the started group occupies the leading
positions of a member's queue, in start order; the unstarted group follows
in its existing order.

- Start (`task_start.rs:42-44`): `order.insert(other_active as usize,
  task_id.clone())` instead of index 0, so the started task goes to the end
  of the started group.
- `MoveTarget::Head` (`task_ops.rs:718-729`): index = number of leading
  `active` rows of `order` (today 0 or 1).
- Active-task move (`task_ops.rs:572-581`): still a no-op with its `moved`
  event; `from` and `to` report the task's current position, not
  `QueuePosition::HEAD`.
- `End`, `Before` (target must be `assigned`) and new assignments already
  land in the unstarted group; no other placement rule is added.

Nudge (simplified scope item 2): `open_task_heads` becomes

```rust
async fn open_tasks_by_member(&self, reader: &dyn AsyncTaskLedgerReader,
    candidates: &[MemberObservation]) -> HashMap<MemberKey, Vec<TaskRow>>;  // queue order from open_tasks_for_team
```

`PreparedTaskPass.heads` becomes `open: HashMap<MemberKey, Vec<TaskRow>>`.
`dispose` receives `rows.first()` as `head` exactly as today. The head row
is the member's single bookkeeping row: reminders, resets and stall
escalation are read from and recorded on it, once per member per pass.
`TaskDisposition::Nudge` emits one reminder per member whose body lists
every open task id through `{{open_tasks}}`. The default `task_ready` and
`task_reminder` bodies gain the attribute `open="{{open_tasks}}"` after
`task="{{task_id}}"`; a team override may use the placeholder, and render
paths that hold only the event (`internal_nudge.rs`) render it empty.

ADR-061 classification and approval: dropping a unique index leaves every
older statement valid against the new database; the previous release binary
re-creates `one_active_task_per_agent` on open and fails while a member has
two active tasks. Rand approved this with no downgrade compatibility in
the agreed scope on #1932 (item 4). The ADR-062 amendment records the
approval, and the CHANGELOG carries the release note. No
`STORAGE_SCHEMA_VERSION` is added, for the reason in ADR-061 D6 Phase BB.1.

## Authoritative deliverables

1. `MAX_TASKS_METADATA_KEY` and `DEFAULT_MAX_TASKS` in `atm-storage`
   `task_state.rs`.
2. `reject_over_max_tasks` in `task_start.rs` replacing
   `reject_concurrent_active_task`; the start inserts at the end of the
   started group.
3. `TASK_INDEX_DDL` drops `one_active_task_per_agent` (`DROP INDEX IF
   EXISTS`) and no longer creates it.
4. `task_ops.rs`: `Head` skips every leading active row; an active-task move
   reports its real position.
5. `UpdateMemberRequest::max_tasks` and `with_max_tasks`, the metadata write,
   and the `--max-tasks` flag on `atm teams update-member`.
6. One Herdr task-pass nudge per member listing all its open tasks:
   `open_tasks_by_member`, the `open_tasks` placeholder in both default
   bodies, and the new `build_task_reminder_dispatch` argument.
7. `docs/requirements.md` §15 items 3, 4, 6 (edited without a REQ id or
   exception record) and ADR-062 (`:68`, row `:125`,
   a 2026-10-20 amendment quoting the five agreed items) say "at most
   `max_tasks` active tasks per member (roster `metadata_json`, default 1)",
   started-first order, one nudge per member; `CHANGELOG.md` Unreleased gets
   the feature line and the release note "before running an older daemon,
   close each member's extra active tasks".
8. Tests, same unit, in the files the criteria name. Each test sets its
   own roster rows, tasks and fixed timestamps; nothing sleeps or waits on
   a clock.

## Authoritative acceptance criteria

`crates/atm-storage-rusqlite/tests/task_identity.rs`:

- [ ] (item 5, default 1) `start_while_another_task_is_active_is_rejected`
  keeps its setup with no `max_tasks` in the roster: the second start fails
  `TaskMaxActiveReached` (`ATM_TASK_MAX_ACTIVE_REACHED`) with "max_tasks is 1"
  in the message, and T2 stays
  `Assigned`. Only the message assertion changes.
- [ ] (item 1) `max_tasks_three_allows_three_active_and_refuses_a_fourth`:
  the member's roster `metadata_json` is `{"max_tasks":3}`. T1..T3 start, and
  all three rows are `Active`. Planted negative: T4's start fails
  `TaskMaxActiveReached` (`ATM_TASK_MAX_ACTIVE_REACHED`) with "max_tasks is 3",
  and T4 stays `Assigned`.
- [ ] (item 1) `explicit_max_tasks_one_refuses_a_second_start`:
  `{"max_tasks":1}` behaves exactly as the default.
- [ ] (item 1) `invalid_stored_max_tasks_refuses_the_start`: `{"max_tasks":0}`
  and `{"max_tasks":"3"}` each fail the start with
  `ATM_MESSAGE_VALIDATION_FAILED` naming `max_tasks`, and the task stays
  `Assigned`.
- [ ] (item 3) `started_tasks_sort_ahead_in_start_order`: with max 3, assign
  T1..T4, then start T3 and then T1. `positions` reads T3=1, T1=2, T2=3,
  T4=4.
- [ ] (item 3) `head_move_skips_every_active_task`: with T3 and T1 active,
  `move T4 --head` gives T4 position 3. Planted negative: T3 and T1 keep
  positions 1 and 2.
- [ ] (item 3) `move_of_second_active_task_reports_its_position`: moving
  the active T1 at position 2 returns `from == to == 2`, appends `moved`, and
  changes no position.
- [ ] (item 4) `ensure_schema_drops_one_active_task_per_agent`: a database
  created with the pre-bk-8 `TASK_INDEX_DDL` text (frozen in the test) has
  the index. After `ensure_schema`, `sqlite_master` has no
  `one_active_task_per_agent`, two `active` rows for one member insert, and
  a second `ensure_schema` is idempotent.

`crates/atm-core/src/team_admin.rs` tests:

- [ ] (item 5) `update_member_max_tasks_writes_roster_metadata`:
  `with_max_tasks(Some(3))` stores `metadata_json["max_tasks"] == 3` and
  leaves `alias`, `host` and the backend keys unchanged.
  `with_max_tasks(None)` leaves the key absent or unchanged.
- [ ] (item 5) `update_member_max_tasks_zero_is_a_validation_error`:
  `with_max_tasks(Some(0))` returns `ATM_MESSAGE_VALIDATION_FAILED`.

`crates/atm/src/commands/teams.rs` tests:

- [ ] (item 5) `update_member_build_request_carries_max_tasks`:
  `--max-tasks 3` yields `request.max_tasks == Some(3)`. Planted negative:
  `--max-tasks 0` fails in `build_request` with
  `ATM_MESSAGE_VALIDATION_FAILED`. With the flag omitted, `max_tasks` is
  `None`.

`crates/atm-core/src/send/nudge_template.rs` tests and
`crates/atm-core/tests/task_reminder_dispatch.rs`:

- [ ] (item 2) `task_reminder_body_lists_open_tasks`: the default
  `task_reminder` and `task_ready` bodies rendered with open tasks
  `[T1, T2, T3]` contain `open="T1 T2 T3"`. A team override using
  `{{open_tasks}}` passes validation. `every_default_body_renders_with_its_placeholders`
  still passes.
- [ ] (item 2) `build_task_reminder_dispatch` with head T1 and
  `[T1, T2]` renders one body that names both ids. The existing four
  cases pass with `&[head id]`.

`crates/atm-http-runtime/src/herdr_queue_wake/tests/herdr_nudge_invariant.rs`
(existing fake emitter and manual clock fixture):

- [ ] (item 2, Rand "ALL tasks in a single nudge") `one_nudge_lists_every_open_task_of_an_idle_member`:
  an idle member has three open tasks (two active, one assigned) and the
  reminder is due. One pass emits exactly one prompt for that member, whose
  body names all three ids. One reminder is recorded, on the head row only:
  the head's `reminder_count` is 1 and the other two rows are 0.
- [ ] (item 2) `stall_escalates_once_per_member`: with the head's
  `reminder_count` at `TASK_STALLED_REMINDER_THRESHOLD`, one pass sends one
  stalled escalation for the member, not one per task.
- [ ] (item 2) `members_are_nudged_independently`: two idle members with two
  open tasks each get one prompt each, and each body lists only that
  member's ids.

## Authoritative validation

- `cargo build --workspace`
- `cargo test -p atm-storage-rusqlite -p atm-storage -p atm-core -p atm -p atm-http-runtime`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `just lint` (`.just/check_line_counts.py`: `task_ops.rs` and
  `task_store.rs` stay under 1000 production lines)

## Non-closure

- No HTTP API field or `HTTP_API_VERSION` change, no doctor change, no role
  or team-level setting, and no `bead-queues` or console change (Rand: they
  pull task state).
- Setting quality-mgr and atm-sanity to 3 is an operator step after
  release (`atm teams update-member atm-dev quality-mgr --max-tasks 3`).
- No user-document change: `docs/user-documents/cli-reference.md` and
  `tasks.md` belong to bk-5. The flag's help text is the CLI reference
  until bk-5 lands.
- No downgrade path: an older daemon fails while a member has two active
  tasks (release note).
