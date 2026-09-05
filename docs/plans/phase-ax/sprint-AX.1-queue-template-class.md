---
phase: AX
sprint: AX.1
title: Queue template class and default template fixes
branch: feature/ax1-queue-template-class
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ax1-queue-template-class
integration_branch: integrate/phase-ax
status: draft
recommended_agent: Cipher-311d
recommended_model: fast
execution_track: A
parallel_with: [AX.3]
dependency_relations:
  - prerequisite: AX.1
    dependent: AX.2
    relation: must_follow
    rationale: AX.2 renders Queue-family templates on the Herdr pump path and both sprints edit crates/atm-core/src/send/hook.rs (this sprint changes render_built_in_nudge_for_dispatch; AX.2 changes the Herdr branch of build_built_in_dispatch).
  - prerequisite: AX.1
    dependent: AX.3
    relation: parallel_safe
    rationale: AX.3 changes no nudge behaviour; the only overlap is additive edits in different regions of crates/atm-storage/src/contract.rs, resolved by AX.3 merging integrate/phase-ax forward before its PR.
---

# AX.1 — Queue template class and default template fixes

Add a queue class to the built-in nudge templates so `atm queue` renders
its own family, retire the two unreachable task-steer kinds, migrate the
override table's kind constraint, and apply the agreed fixes to every
default body and every published copy of it. No new placeholders, no
renderer changes, no new XML attributes.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or
shape-only completion fails the sprint.

- [ ] D1 — `BuiltInNudgeTemplateKind` becomes seven kinds
  (`crates/atm-storage/src/contract.rs`). Code contract C1.
- [ ] D2 — override-table migration
  (`crates/atm-storage-rusqlite/src/shared_db.rs`): new function
  `migrate_template_override_kinds_to_seven(conn)` called from the
  existing schema-ensure path that runs on every database open (CLI and
  daemon alike). It rebuilds `team_nudge_template_overrides` with the
  seven-value `CHECK` in code contract C4, copies every row whose kind is
  one of the seven, and drops each row whose kind is `delivery_task` or
  `delivery_task_ack` with one `tracing::warn!` line naming team and kind
  (retained by REQ-P-OBS-003). Idempotent: a database already on the
  seven-value `CHECK` is left untouched.
- [ ] D3 — kind selection takes the nudge mode. Code contract C2.
  `built_in_nudge_template_kind_from_post_send_event` gains the
  `NudgeKind` parameter (`crates/atm-core/src/boundary/mod.rs`);
  `render_built_in_nudge_for_dispatch` in
  `crates/atm-core/src/send/hook.rs` gains `nudge_kind: NudgeKind` and
  its two call sites inside `build_built_in_dispatch` (tmux and graft
  branches) pass the `kind` that function already derives via
  `nudge_kind_for_mode`. `crates/atm-core/src/nudge_dispatch.rs` is
  unchanged: it already supplies `NudgeMode` to `build_built_in_dispatch`.
  `crates/atm/src/commands/internal_nudge.rs`: the test
  `built_in_template_kind_selection_covers_six_paths` becomes
  `built_in_template_kind_selection_covers_seven_paths` and asserts every
  row of the C2 table; fixtures that name `DeliveryTask` /
  `DeliveryTaskAck` are changed to `Task`.
- [ ] D4 — default bodies (`crates/atm-core/src/send/nudge_template.rs`
  `default_template`). Code contract C3.
- [ ] D5 — CLI: `crates/atm/src/commands/teams.rs` `set-nudge-template`,
  `disable-nudge-template`, `clear-nudge-template` accept `queue`,
  `queue_ack`, `task`; a retired string is rejected with
  `ATM_MESSAGE_VALIDATION_FAILED` and the hint `use "task"`; help text
  lists all seven kinds. `crates/atm-core/src/team_admin.rs` request
  types carry the kind unchanged (verify by test that a `queue_ack`
  override round-trips through `set_nudge_template_override_with_store`).
- [ ] D6 — normative docs lose the six-kind bound:
  `docs/requirements.md` line 1094 ("exactly six named template cases"),
  line 1105 ("those six built-in template bodies"), and line 4496 ("the
  six built-in nudge template bodies"); each must read "seven";
  `docs/architecture.md` lines 2736–2738; `docs/atm/requirements.md`
  lines 143–144. Each states the seven kinds and the rule that
  `NudgeKind` selects the delivery or queue family.
  `docs/adr/ADR-019-direct-post-send-and-claude-json-retirement.md` gains
  a dated amendment section (original body unchanged) recording the
  seven-kind inventory, the retirement, the migration in D2, and the
  `NudgeKind` rule; `docs/adr/INDEX.md` entry text updated if the ADR
  status line changes.
- [ ] D7 — user docs and examples: `docs/user-documents/nudge-templates.md`
  documents the seven kinds (line 23 "exactly six" becomes "seven"), the
  C2 mapping, the migration, and the corrected read action, and records
  the retirement with the sentence "Two former task steer kinds were
  retired in phase AX and are rejected on input; see the ADR-019
  amendment for their names and the migration" (the retired names appear
  only in the ADR-019 amendment, so AC 2's gate holds);
  `docs/user-documents/examples/nudge-templates/`: fix `read atm` in
  `delivery.xml`, `delivery_ack.xml`, `manage-templates.sh`; delete
  `delivery_task.xml` and `delivery_task_ack.xml`; add `queue.xml`,
  `queue_ack.xml`, `task.xml` matching C3.
- [ ] D8 — live nudge-text producers outside the crates:
  `scripts/atm-nudge.py` line 266, `scripts/atm-nudge.sh` line 50,
  `scripts/test_atm_nudge.py` lines 281 and 287, and
  `.claude/skills/restore-team-communications/SKILL.md` line 76 replace
  `read atm --team <team>` / `read atm` with the targeted read action.
- [ ] D9 — task-tagged mail is always deferred (phase plan §2 "tasks are
  always queued"): new `pub(crate) fn nudge_mode_for_request(request:
  &SendRequest, task_id: &Option<TaskId>) -> NudgeMode` beside
  `request_requires_ack` in `crates/atm-core/src/send/mod.rs` returning
  `NudgeMode::Deferred` when `task_id.is_some()`, otherwise
  `request.nudge_mode`; called at both sites that call
  `request_requires_ack` (`prepare_persisted_write` and
  `prepare_persisted_write_async` in `crates/atm-core/src/write/pipeline.rs`).
  Consequence on every backend: a `--task-id` send never steers; tmux and
  graft recipients get the pending-nudge marker exactly as `atm queue`
  does today and are nudged with the Task body when idle. (AX.5 removes
  the marker for Herdr recipients only.) Code contract C5.
- [ ] D10 — tests listed under Required validation.

### Paths to delete

- `docs/user-documents/examples/nudge-templates/delivery_task.xml`
- `docs/user-documents/examples/nudge-templates/delivery_task_ack.xml`

### Paths that must not change

- `docs/plans/phase-aq/evidence/**` (committed CI evidence records).
- `docs/plans/phase-AD/sprint-AD21.md`,
  `reports/smoke/phase-AE-installed-docs-proof.md` (historical records).
- The original decision body of ADR-019 (amended by appended section only).
- `crates/atm-graft/src/nudge_sink.rs`, `crates/atm-graft-python/src/lib.rs`,
  `crates/hermes-atm/tests/test_runtime.py`: their `read atm` strings are
  fixture bodies for their own injectors, not built-in defaults.
- `crates/atm-architecture/tests/pending_nudge_store_boundary.rs`: its
  `read atm-storage types source` strings are unrelated prose.
- `scripts/hooks/test_queue_hooks.py` lines 150 and 154: `read atm` is a
  fixture body for the queue hook under test, not a built-in default.

## Code contracts

### C1 — kind enum

```rust
// crates/atm-storage/src/contract.rs
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BuiltInNudgeTemplateKind {
    Delivery,        // "delivery"
    DeliveryAck,     // "delivery_ack"
    Queue,           // "queue"          (new)
    QueueAck,        // "queue_ack"      (new)
    Task,            // "task"           (new)
    Acknowledge,     // "acknowledge"
    AcknowledgeTask, // "acknowledge_task"
}
// FromStr: "delivery_task" | "delivery_task_ack" => Err(AtmError::validation(
//     "template kind `delivery_task` was retired; use `task`"))
```

The four surviving strings are unchanged. `as_str`, `Display`, `FromStr`
round-trip for all seven.

### C2 — kind selection

```rust
// crates/atm-core/src/boundary/mod.rs
pub fn built_in_nudge_template_kind_from_post_send_event(
    event: &PostSendHookEvent,
    nudge_kind: NudgeKind,
) -> BuiltInNudgeTemplateKind {
    use BuiltInNudgeTemplateKind as K;
    match (event.is_ack, event.task_id.is_some(), event.requires_ack, nudge_kind) {
        (true, true, _, _)                 => K::AcknowledgeTask,
        (true, false, _, _)                => K::Acknowledge,
        (false, true, _, _)                => K::Task,
        (false, false, false, NudgeKind::Steer) => K::Delivery,
        (false, false, true,  NudgeKind::Steer) => K::DeliveryAck,
        (false, false, false, NudgeKind::Queue) => K::Queue,
        (false, false, true,  NudgeKind::Queue) => K::QueueAck,
    }
}

// crates/atm-core/src/send/hook.rs
fn render_built_in_nudge_for_dispatch<R>(
    runtime: &R,
    event: &PostSendHookEvent,
    nudge_kind: NudgeKind,
) -> Option<String>
where
    R: RetainedServiceRuntime + ?Sized;
```

A task-tagged event resolves to `Task` in either mode. `requires_ack` is
already forced for task messages (`send/mod.rs` `request_requires_ack`),
so the `Task` body always carries the ack action.

### C3 — default bodies

Delivery (only the read action changes from today):

```xml
<atm from="{{from}}" message-id="{{message_id}}">
  <action>atm read --message-id {{message_id}}</action>
  <description>{{description}}</description>
  <action>execute the assigned task</action>
  <when idle="immediate" busy="after-current-task"/>
  <console announce="concise" pause="false"/>
</atm>
```

DeliveryAck: Delivery with `<action>ack the message</action>` inserted
after the read action.

Queue: Delivery with the `<when .../>` line removed.
QueueAck: DeliveryAck with the `<when .../>` line removed.

Task:

```xml
<atm from="{{from}}" message-id="{{message_id}}">
  <action>atm read --message-id {{message_id}}</action>
  <action>ack the message</action>
  <task id="{{task_id}}">{{description}}</task>
  <action>execute the assigned task</action>
  <console announce="concise" pause="false"/>
</atm>
```

Acknowledge and AcknowledgeTask: unchanged.

### C4 — override table after migration

```sql
CREATE TABLE IF NOT EXISTS team_nudge_template_overrides (
    team_name TEXT NOT NULL,
    template_kind TEXT NOT NULL
        CHECK(template_kind IN (
            'delivery', 'delivery_ack', 'queue', 'queue_ack', 'task',
            'acknowledge', 'acknowledge_task'
        )),
    -- remaining columns unchanged
);
```

Migration detection: read `sql` from `sqlite_master` for the table; if it
lacks `'queue'`, rebuild (create `_new`, copy accepted rows, drop old,
rename). Retired rows are dropped because the new `CHECK` cannot hold
them.

### C5 — task mail is deferred

```rust
// crates/atm-core/src/send/mod.rs
pub(crate) fn nudge_mode_for_request(request: &SendRequest, task_id: &Option<TaskId>) -> NudgeMode {
    if task_id.is_some() { NudgeMode::Deferred } else { request.nudge_mode }
}
```

The Task body therefore never needs `<when>`: it is only ever delivered
from the queue (marker on tmux and graft; AX.5 pump on Herdr).

### Unchanged surfaces

`nudge_template::render_built_in_nudge` signature; `PostSendHookEvent`;
placeholder set; renderer conditional rejection; `NudgeTemplateOverrideStore`
trait; `crates/atm-core/src/nudge_dispatch.rs`.

## Acceptance criteria

1. On a database created before this sprint holding one `delivery_task`
   override row and one `delivery_ack` row: after open, the
   `delivery_task` row is gone with one warning logged, the `delivery_ack`
   row is intact, and `atm teams set-nudge-template atm-dev queue_ack
   --file x.xml` succeeds; `atm queue --requires-ack` to a member of that
   team renders the override; `atm teams set-nudge-template atm-dev
   delivery_task_ack --file x.xml` exits 3 with the `use "task"` hint.
2. Gates, each must return nothing:
   `grep -rn 'read atm --team' crates/atm-core/src/send/nudge_template.rs docs/user-documents scripts .claude/skills/restore-team-communications`;
   `grep -rln 'delivery_task_ack' docs/requirements.md docs/architecture.md docs/atm/requirements.md docs/user-documents`;
   `grep -rn 'six built-in\|six named template\|exactly one of six\|those six\|exactly six' docs/requirements.md docs/architecture.md docs/atm/requirements.md docs/user-documents`.
   Unit assertion: `default_template(kind)` for all seven kinds contains
   no `read atm `.
4. `atm send tmux-member --task-id t1 --stdin` writes the message, sets
   the pending-nudge marker, emits no steer, and the marker nudge renders
   the Task body; `atm send tmux-member --stdin` (no task) still steers.
5. All Required validation tests pass; `just validate` green, including
   `scripts/check-nudge-taxonomy.py` with an unchanged allowlist (no new
   identifier in this sprint contains `nudge`; the migration function is
   named without it).

## Required validation

- `crates/atm-core/tests/nudge_mode.rs`: for one Herdr-backed and one
  tmux-backed member, `atm send` resolves Delivery / DeliveryAck,
  `atm queue` resolves Queue / QueueAck, and a `--task-id` send resolves
  Task and `NudgeMode::Deferred` whether sent with `atm send` or `atm
  queue` (AC 4; six assertions per backend, plus the marker assertion
  for the tmux member on the sync and async write paths).
- `crates/atm-core/src/send/nudge_template.rs` unit tests: every default
  body renders without error; no default body contains `read atm `;
  Queue, QueueAck, Task bodies lack `<when`; Delivery, DeliveryAck
  contain `<when`; every non-ack body contains `execute the assigned
  task` and `atm read --message-id`.
- `crates/atm/src/commands/internal_nudge.rs`:
  `built_in_template_kind_selection_covers_seven_paths`.
- `crates/atm-storage/src/contract.rs` tests: seven-kind `FromStr` /
  `as_str` round trip; `delivery_task` and `delivery_task_ack` parse to
  the retirement error.
- `crates/atm-storage-rusqlite/src/shared_db.rs` tests: AC 1 migration
  scenario (old six-value `CHECK` database → rebuilt, retired rows
  dropped, others retained, `queue_ack` insert accepted); second open is
  a no-op.
- `crates/atm-storage-rusqlite/src/nudge_template_override_store.rs`
  tests: round trip for `queue`, `queue_ack`, `task`;
  `load_template_override` returns `None` for a retired kind after
  migration.
- `scripts/test_atm_nudge.py` updated assertions pass.
- `just validate` on the sprint branch; quality-mgr Final Quality Report
  posted on the PR before merge; `arch-qa` review of the ADR-019
  amendment.

## Out of scope

Herdr rendering (AX.2); task state (AX.3, AX.4); the Herdr marker
exception (AX.5); any change to the renderer's
placeholder set or conditional handling; watermark or re-nudge behaviour;
a `show-nudge-template` command.
