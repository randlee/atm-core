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
dependency_relations:
  - related: AX.2
    relation: must_follow
    rationale: AX.2 renders Queue-family templates on the Herdr pump path and edits send/hook.rs; the kinds and corrected defaults must exist first.
  - related: AX.4a
    relation: must_follow
    rationale: both edit crates/atm-storage/src/contract.rs and docs/user-documents; AX.4a's completion message relies on the kind mapping delivered here.
---

# AX.1 — Queue template class and default template fixes

Add a queue class to the built-in nudge templates so `atm queue` renders
its own family, retire the two unreachable task-steer kinds, and apply
the agreed fixes to every default body. No new placeholders, no renderer
changes, no new XML attributes. Backend-agnostic: the change is in kind
selection and bodies only.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or
shape-only completion fails the sprint.

- [ ] D1 — `BuiltInNudgeTemplateKind` becomes seven kinds
  (`crates/atm-storage/src/contract.rs`). Code contract C1. Stored
  override rows whose `template_kind` is `delivery_task` or
  `delivery_task_ack` are deleted at daemon startup in
  `atm-storage-rusqlite` with one info log line each naming the team and
  kind.
- [ ] D2 — kind selection takes the nudge mode
  (`crates/atm-core/src/boundary/mod.rs`
  `built_in_nudge_template_kind_from_post_send_event`). Code contract C2.
  Callers in `crates/atm-core/src/send/hook.rs` pass the `NudgeKind`
  they already compute via `nudge_kind_for_mode`;
  `crates/atm-core/src/nudge_dispatch.rs` `rebuild_received_hook_dispatch`
  passes its `NudgeKind` argument through.
- [ ] D3 — default bodies (`crates/atm-core/src/send/nudge_template.rs`
  `default_template`). Code contract C3.
- [ ] D4 — CLI: `crates/atm/src/commands/teams.rs` `set-nudge-template`,
  `disable-nudge-template`, `clear-nudge-template` accept `queue`,
  `queue_ack`, `task`; a retired string is rejected with
  `ATM_MESSAGE_VALIDATION_FAILED` and the hint `use "task"`; help text
  lists all seven kinds.
- [ ] D5 — docs: `docs/user-documents/nudge-templates.md` documents the
  seven kinds, the C2 mapping, the retirement, and the corrected read
  action; `docs/user-documents/examples/nudge-templates/`: fix `read atm`
  in `delivery.xml`, `delivery_ack.xml`, `manage-templates.sh`; delete
  `delivery_task.xml` and `delivery_task_ack.xml`; add `queue.xml`,
  `queue_ack.xml`, `task.xml` matching C3; `docs/requirements.md`:
  replace the two "exactly six named template cases" / "six built-in
  nudge template bodies" statements (post-send hook block near line 1093
  and the emitter seam block near line 4496) with the seven kinds and the
  rule that `NudgeKind` selects the delivery/queue family.
- [ ] D6 — tests listed under Required validation.

### Paths to delete

- `docs/user-documents/examples/nudge-templates/delivery_task.xml`
- `docs/user-documents/examples/nudge-templates/delivery_task_ack.xml`

### Paths that must not change

- `docs/plans/phase-aq/evidence/**` (committed CI evidence records that
  contain `read atm`; never edited).
- `docs/plans/phase-AD/sprint-AD21.md` (historical).

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

### Unchanged surfaces

`nudge_template::render_built_in_nudge` signature; `PostSendHookEvent`;
placeholder set; renderer conditional rejection; `NudgeTemplateOverrideStore`
trait.

## Acceptance criteria

1. `atm teams set-nudge-template atm-dev queue_ack --file x.xml` succeeds
   and `atm queue --requires-ack` to a member of that team renders the
   override; `atm teams set-nudge-template atm-dev delivery_task_ack
   --file x.xml` exits 3 with the `use "task"` hint.
2. `grep -rn 'read atm' crates docs/user-documents` returns nothing.
3. A daemon started against a database holding a `delivery_task`
   override row deletes it and logs one info line; the row is absent on
   the next `atm teams show-nudge-template` (or equivalent list).
4. All Required validation tests pass; `just validate` green.

## Required validation

- `crates/atm-core/tests/nudge_mode.rs`: for one Herdr-backed and one
  tmux-backed member, `atm send` resolves Delivery / DeliveryAck,
  `atm queue` resolves Queue / QueueAck, and a `--task-id` send resolves
  Task in both modes (six assertions per backend).
- `crates/atm-core/src/send/nudge_template.rs` unit tests: every default
  body renders without error; no default body contains `read atm`;
  Queue, QueueAck, Task bodies lack `<when`; Delivery, DeliveryAck
  contain `<when`; every non-ack body contains `execute the assigned
  task`; every non-ack body contains `atm read --message-id`.
- `crates/atm-storage/src/contract.rs` tests: seven-kind `FromStr` /
  `as_str` round trip; `delivery_task` and `delivery_task_ack` parse to
  the retirement error.
- `crates/atm-storage-rusqlite/src/nudge_template_override_store.rs`
  tests: round trip for `queue`, `queue_ack`, `task`; startup deletes
  retired-kind rows and leaves the other kinds untouched.
- `just validate` on the sprint branch; quality-mgr Final Quality Report
  posted on the PR before merge.

## Out of scope

Herdr rendering (AX.2); task state (AX.4a); any change to the renderer's
placeholder set or conditional handling; watermark or re-nudge behaviour.
