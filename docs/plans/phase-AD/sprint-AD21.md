---
id: AD.21
title: Built-In Post-Send Nudge And Six-Template Override Surface
status: planned
branch: feature/pAD-s21-built-in-post-send-nudge-and-template-overrides
worktree: ../atm-core-worktrees/feature/pAD-s21-built-in-post-send-nudge-and-template-overrides
target: integrate/phase-AD
---

# Sprint AD.21 — Built-In Post-Send Nudge And Six-Template Override Surface

## Goal

- make post-send nudge work in a normal installed ATM binary with no repo-local
  Python or shell dependency while preserving a full local command override
  path and a bounded team-tunable built-in template surface

## Hard Dependencies

- `AD.20` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/violation-inventory.md`

## Exact Targets

- `crates/atm-core/src/send/hook.rs`
- `crates/atm-core/src/config/mod.rs`
- `crates/atm-core/src/config/types.rs`
- `crates/atm-core/src/config/discovery.rs`
- `crates/atm-core/src/team_admin.rs`
- `crates/atm/src/commands/mod.rs`
- `crates/atm/src/commands/teams.rs`
- `crates/atm/src/commands/internal_nudge.rs`
- `crates/atm/src/main.rs`
- `crates/atm-core/tests/mailbox_locking.rs`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm-graft/requirements.md`
- `docs/atm-graft/architecture.md`
- `docs/adr/ADR-019-direct-post-send-and-claude-json-retirement.md`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/sprint-AD21.md`

## Interfaces To Add Or Modify

The built-in nudge renderer is case-based, not logic-programmable:

```rust
pub enum BuiltInNudgeTemplateKind {
    Delivery,
    DeliveryAck,
    DeliveryTask,
    DeliveryTaskAck,
    Acknowledge,
    AcknowledgeTask,
}

pub struct BuiltInNudgeEvent {
    pub from: String,
    pub team: TeamName,
    pub message_id: AtmMessageId,
    pub description: String,
    pub task_id: String,
}

pub struct BuiltInNudgeTemplateSet {
    pub delivery: String,
    pub delivery_ack: String,
    pub delivery_task: String,
    pub delivery_task_ack: String,
    pub acknowledge: String,
    pub acknowledge_task: String,
}

pub struct TeamNudgeTemplateOverrideRow {
    pub team_name: TeamName,
    pub kind: BuiltInNudgeTemplateKind,
    pub template_body: String,
}
```

The accepted built-in rendering contract after this sprint is:

- ATM chooses exactly one of the six template kinds above
- built-in rendering performs direct placeholder substitution only
- no conditional language, no Jinja evaluation, and no template-side branching
- supported placeholders are exactly:
  - `{{from}}`
  - `{{team}}`
  - `{{message_id}}`
  - `{{description}}`
  - `{{task_id}}`
- `{{task_id}}` and `{{description}}` are always available; one or both may be
  empty strings depending on the message family
- the shipped built-in path is the hidden/internal `atm internal-nudge`
  subcommand rather than a repo-local Python or shell script
- `atm internal-nudge` must dispatch to exactly one concrete sink:
  - `TmuxNudgeSink` for local tmux-backed recipients
  - `GraftNudgeSink` for graft-backed recipients
- `TmuxNudgeSink` must preserve the current operational tmux-injection pattern:
  paste the rendered nudge text, send `Enter`, wait about `250ms` to `300ms`,
  then send a second `Enter`; the exact delay stays implementation-tunable but
  the accepted design must record that this timing-sensitive double-enter path
  exists and needs verification

The accepted default XML templates are:

```xml
<!-- delivery -->
<atm from="{{from}}" message-id="{{message_id}}">
  <action>read atm --team {{team}}</action>
  <description>{{description}}</description>
  <action>execute the assigned task</action>
  <when idle="immediate" busy="after-current-task"/>
  <console announce="concise" pause="false"/>
</atm>

<!-- delivery_ack -->
<atm from="{{from}}" message-id="{{message_id}}">
  <action>read atm --team {{team}}</action>
  <action>ack the message</action>
  <description>{{description}}</description>
  <action>execute the assigned task</action>
  <when idle="immediate" busy="after-current-task"/>
  <console announce="concise" pause="false"/>
</atm>

<!-- delivery_task -->
<atm from="{{from}}" message-id="{{message_id}}">
  <action>read atm --team {{team}}</action>
  <task id="{{task_id}}">{{description}}</task>
  <action>execute the assigned task</action>
  <when idle="immediate" busy="after-current-task"/>
  <console announce="concise" pause="false"/>
</atm>

<!-- delivery_task_ack -->
<atm from="{{from}}" message-id="{{message_id}}">
  <action>read atm --team {{team}}</action>
  <action>ack the message</action>
  <task id="{{task_id}}">{{description}}</task>
  <action>execute the assigned task</action>
  <when idle="immediate" busy="after-current-task"/>
  <console announce="concise" pause="false"/>
</atm>

<!-- acknowledge -->
<atm kind="ack" from="{{from}}" message-id="{{message_id}}"/>

<!-- acknowledge_task -->
<atm kind="ack" from="{{from}}" message-id="{{message_id}}" task-id="{{task_id}}"/>
```

The accepted precedence order after this sprint is:

1. matching external `[[atm.post_send_hooks]]` command
2. host-scoped SQLite-backed built-in template override row for the active team
   and selected template kind
3. built-in product default template for that template kind

## Paths To Delete

- any assumption that a shipped ATM install requires `scripts/atm-nudge.py`,
  `scripts/atm-nudge.sh`, Python, or shell scripts for its default nudge path
- any new design that introduces conditional logic or Jinja execution into the
  built-in template renderer
- any post-send fallback that silently skips built-in nudge when no matching
  external hook rule is configured

## Deliverables

- the installed `atm` binary exposes a built-in internal nudge path usable as
  the default post-send implementation with no repo-local script dependency
- `[[atm.post_send_hooks]]` remains the full command/script override path and
  still wins when a matching rule is configured
- the built-in path selects one of the six accepted template kinds and renders
  it through fixed placeholder substitution only
- host-scoped, team-keyed SQLite-backed template override rows can replace any
  subset of the six built-in templates without requiring repo-local Python
  script distribution
- the built-in path resolves exactly one concrete sink after rendering:
  `TmuxNudgeSink` or `GraftNudgeSink`
- the accepted docs describe the exact six-template contract, the placeholder
  inventory, and the override-precedence rule above
- built-in `acknowledge` and `acknowledge_task` defaults are intentionally
  minimal and do not repeat delivery-only context such as description, extra
  action text, console hints, or delivery-oriented body text

## This Sprint Does Not Close

- removal of git-tracked tmux pane ids from `.atm.toml`
- migration of current dogfooding repo config away from committed pane routing
- retirement or deletion of the repo-local nudge scripts used only for current
  dogfood compatibility

## Acceptance Criteria

- targeted regression coverage proves the built-in renderer selects the correct
  one of the six template kinds for:
  - `atm send`
  - `atm send --requires-ack`
  - `atm send --task ...`
  - `atm send --task ... --requires-ack`
  - `atm ack` without task context
  - `atm ack` with task context
- targeted regression coverage proves placeholder substitution is direct and
  bounded:
  - no unknown placeholder silently disappears
  - no conditional or Jinja-style syntax is evaluated
  - empty `task_id` never renders the task templates
- targeted precedence coverage proves:
  - matching external `[[atm.post_send_hooks]]` rules still override the
    built-in path
  - host-scoped, team-keyed SQLite-backed built-in template override rows
    replace only the addressed template kinds
  - any unset template kind falls back to the product default body for that
    kind
- targeted rendering coverage proves the default built-in acknowledge templates
  stay intentionally smaller than delivery templates:
  - `acknowledge` renders exactly
    `<atm kind="ack" from="{{from}}" message-id="{{message_id}}"/>`
  - `acknowledge_task` renders exactly
    `<atm kind="ack" from="{{from}}" message-id="{{message_id}}" task-id="{{task_id}}"/>`
- targeted sink-selection coverage proves:
  - local tmux-backed recipients route through `TmuxNudgeSink`
  - graft-backed recipients route through `GraftNudgeSink`
  - tmux sink regression coverage verifies the documented paste + `Enter` +
    short sleep + second `Enter` behavior
- docs state explicitly that the default installed nudge path no longer depends
  on repo-local scripts or external interpreters
- docs enumerate the exact six template names and exact placeholder inventory

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted built-in template-kind selection regression coverage
- targeted template precedence regression coverage
- `git diff --check`
