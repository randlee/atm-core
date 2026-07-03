---
title: Phase AD Plan
status: active
branch: plan/post-send-hook-fix
worktree: /Users/randlee/Documents/github/atm-core-worktrees/plan/post-send-hook-fix
---

# Phase AD Plan

## Goal

Restore the original ATM runtime model for identity, send, read, and post-send
nudge behavior:

- `atm send` persists the message to the database
- if the recipient exposes a post-send hook capability, ATM fires it
- post-send emission failure is logged and surfaced as a sender-visible warning
- `atm read` reads from the database

Phase `AD` exists because the current accepted line has drifted away from that
model in several release-blocking ways:

- bare ATM commands in an `arch-ctm` session can resolve as `team-lead`
- `.atm.toml` still configures `[[atm.post_send_hooks]]`, but the live daemon
  path can complete a send with no nudge and no warning
- the current post-send path is obscured by generic delivery/notification
  machinery instead of one direct post-commit emission path
- `ReconcileRuntime` and its file-watch/import lane remain in the daemon even
  though Claude Code no longer uses that subsystem
- daemon notification delivery is currently a separate queue/worker subsystem
  whose practical job is appending JSONL events to disk
- obsolete `[atm].identity` config still trips `ATM_WARNING_IDENTITY_DRIFT`
- roster and pane truth still drift away from the accepted SQLite-owned model

## Validated Breakage On Entry

- `ATM_IDENTITY=arch-ctm` was present in the active shell, but bare ATM
  commands still resolved as `team-lead` until `--as arch-ctm` was forced
- `.atm.toml` currently contains a `team-lead` post-send hook rule, but a live
  send produced neither the expected nudge nor a sender-visible warning
- `atm doctor --team atm-dev` currently reports
  `ATM_WARNING_IDENTITY_DRIFT`
- current doctor output shows blank `tmux_pane_id` values in roster state for
  `team-lead` and `arch-ctm`

## Design Rules

Phase `AD` is corrective simplification, not a feature-expansion line.

The governing rules are:

- caller identity is mandatory for every caller-owned ATM command before any
  daemon dispatch occurs
- the only accepted caller-identity sources at the CLI boundary are an explicit
  command-line override or `ATM_IDENTITY` from the invoking shell
- if caller identity is unresolved at the CLI boundary, the CLI must fail the
  command and must not contact the daemon
- every downstream request DTO for caller-owned commands must carry resolved
  caller identity as a required field, never an optional field
- the daemon must execute caller-owned commands against declared request
  identity only and must never consult daemon ambient `ATM_IDENTITY` to fill a
  missing caller identity
- message persistence is the send success boundary
- post-send behavior is a post-commit side effect only
- post-send behavior is event-driven; it is not planned through a generic
  delivery-plan abstraction
- ATM owns post-send emission, emission logging, and sender warnings on
  emission failure
- ATM does not own receiver-side consumption after successful emission
- `atm read` is a database read path only
- active `tmux_pane_id` already exists in SQLite roster state and must remain
  authoritative there rather than drifting back to repo config assumptions
- pane metadata must be settable and repairable from the CLI
- `ReconcileRuntime` and the watched-source daemon lane are not part of the
  accepted Claude Code runtime and must be removed
- daemon notification delivery is not a protected subsystem; if notification
  logging remains, ATM should append directly at the event site instead of
  routing through a retained queue/worker service
- Claude Code no longer uses the Claude backend, so `atm-storage-claude` and
  Claude inbox-append runtime behavior must be removed from the accepted line
- post-send nudge emission must not depend on Claude inbox append success
- durable message delivery remains allowed, but post-send nudge ownership must
  be narrowed back to one direct seam
- post-send hook capability resolution must not depend on the caller's current
  working directory or on whichever repo-local `.atm.toml` happens to be found
- directory and roster cleanup in this phase must prefer deletion and direct
  field ownership over adding new coordinator structs, planner structs, or
  compatibility-only state machines
- shared backend interoperability remains mandatory through the `atm-storage`
  contract; SQLite is one backend and future SQL backend support remains a
  requirement after the Claude backend is retired
- backend interoperability does not require multiple live concrete backends on
  the accepted line; it requires that the shared contract stays future-backend
  ready without another architectural rewrite
- any new sealed `atm-core` boundary trait introduced in this phase must land
  with a matching `boundaries/atm-core/*.toml` governance record and a
  `docs/atm-core/boundaries.md` inventory entry before implementation
  dependents close

## Scope Rules

Phase `AD` may:

- fix caller identity ownership on daemon-backed ATM commands
- make caller identity transport explicit and required for daemon-backed
  caller-owned commands
- simplify the post-send nudge path to one post-commit emission seam
- add or tighten trait contracts for local tmux-backed and graft-backed
  post-send emission
- remove `ReconcileRuntime` and the daemon watch/import subsystem
- remove daemon notification queue/worker delivery and replace any retained
  notification logging with direct append logic if the log still has value
- remove the retired Claude backend and obsolete Claude inbox-append runtime
  assumptions while preserving the shared backend contract and future SQL
  backend requirement
- repair roster drift detection and operator guidance
- finish the CLI-managed repair/update path for the existing SQLite-owned pane
  and member-home metadata and remove lingering `.atm.toml` assumptions
- simplify directory metadata ownership so only durable `home_dir`, runtime
  `live_cwd`, and log-only startup `launch_cwd` remain
- add smoke and doctor coverage required to keep these regressions closed

Phase `AD` must not:

- redesign durable message delivery semantics
- make send success depend on downstream nudge consumption
- treat receiver-side consumption as an ATM send failure after successful
  emission
- preserve stale post-send behavior just because it is already threaded through
  `DeliveryPlan`, `NotificationSink`, or advisory queue code
- keep Claude inbox append as a hidden runtime fallback
- collapse the architecture into a permanent SQLite-only contract
- preserve dead daemon subsystems just because they already exist

## Baseline

- planning branch: `plan/post-send-hook-fix`
- execution integration branch: `integrate/phase-AD`
- prerequisite accepted line:
  - current `develop` as merged through the accepted `1.2.3` baseline
- active blocking issues:
  - `#421` daemon-mediated identity ownership regression
  - `#440` configured post-send notification can fail silently

## Explicit Contract Sample

The implementation target for post-send emission must stay this direct:

```rust
pub struct PostSendHookEvent {
    pub sender: AgentName,
    pub sender_team: TeamName,
    pub recipient: AgentName,
    pub recipient_team: TeamName,
    pub message_id: AtmMessageId,
    pub requires_ack: bool,
    pub is_ack: bool,
    pub task_id: Option<TaskId>,
    pub recipient_pane_id: Option<PaneId>,
}

pub trait PostSendHookEmitter: sealed::Sealed {
    fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError>;
}
```

Required runtime meaning:

- caller-owned commands:
  - CLI resolves caller identity from explicit override or invoking-shell
    `ATM_IDENTITY`
  - CLI fails locally if caller identity is unavailable
  - daemon receives caller identity as required request data
  - daemon never substitutes its own ambient identity
- send:
  - persist
  - if recipient has post-send hook capability, call `emit(...)`
  - if `emit(...)` fails, log it and append a sender-visible warning
- read:
  - load from durable state only

If notification logging survives, it should stay equally direct:

```rust
persist_message(...)?;
if recipient_has_post_send_hook {
    match post_send_hook_emitter.emit(&event) {
        Ok(()) => append_notification_log(&event)?,
        Err(error) => {
            log_post_send_failure(&error);
            append_sender_warning(render_post_send_warning(&error));
        }
    }
}
```

No accepted `Phase AD` design should require:

- `DeliveryPlan` construction for post-send emission
- `NotificationSink` for post-send emission
- daemon queue/worker orchestration just to append one notification event
- watched-file reconcile/import machinery to make send or read work

## Claude Harness Constraint

Phase `AD` must freeze and enforce this rule:

- Claude inbox JSON append is not part of the accepted Claude Code
  delivery/post-send runtime
- `atm-storage-claude` is retired from the accepted line because Claude Code no
  longer uses that backend
- the shared `atm-storage` contract remains the governing backend seam after
  Claude backend retirement
- any surviving code, tests, docs, or diagrams that still treat inbox append as
  mailbox delivery, nudge delivery, or context injection must be removed or
  rewritten

## Execution Order

Phase `AD` executes the deletion line first so new emitter work does not get
implemented on top of retired Claude JSON, reconcile, or notification
infrastructure.

Phase `AD` orchestration rule:

- `Phase AD` is a strict merge-forward line
- each implementation sprint branch/worktree must be created from the current
  accepted tip of the immediately preceding `AD` sprint
- before development starts on a sprint, that sprint worktree must merge the
  immediately preceding accepted `AD` sprint
- before a sprint starts any QA-findings fix pass, that sprint worktree must
  merge the latest tip of the immediately preceding accepted `AD` sprint again
- sprint branches must merge forward numerically:
  - `AD.1 -> AD.2 -> AD.3 -> AD.4 -> AD.5 -> AD.6 -> AD.7 -> AD.8 -> AD.9 -> AD.10 -> AD.11`
- accepted execution for this phase is immediate-predecessor merge-forward
  only; do not run pairwise cross-merges between unrelated `AD` sprint
  branches

1. [AD.1 Caller Identity Ownership Restore](./sprint-AD1.md)
2. [AD.2 Obsolete Config Identity Removal And Doctor Contract Repair](./sprint-AD2.md)
3. [AD.3 Claude Backend And Inbox Nudge Retirement](./sprint-AD3.md)
4. [AD.4 Reconcile Runtime Removal](./sprint-AD4.md)
5. [AD.5 Notification Runtime Removal And Post-Send `NotificationSink` Detachment](./sprint-AD5.md)
6. [AD.6 Post-Send Nudge Contract Simplification](./sprint-AD6.md)
7. [AD.7 Local Tmux Post-Send Emitter](./sprint-AD7.md)
8. [AD.8 Graft Post-Send Emitter](./sprint-AD8.md)
9. [AD.9 Update-Member CLI And Roster Repair Path](./sprint-AD9.md)
10. [AD.10 Directory Metadata And Doctor Contract Cleanup](./sprint-AD10.md)
11. [AD.11 Smoke And Readiness Closeout](./sprint-AD11.md)

## Phase Exit Criteria

Phase `AD` closes only when:

- bare daemon-backed ATM commands honor the invoking shell identity correctly
- caller-owned commands fail before daemon dispatch when neither explicit
  override nor invoking-shell `ATM_IDENTITY` is present
- daemon-backed caller-owned request shapes carry caller identity as required
  data rather than relying on daemon ambient identity
- repo config no longer carries obsolete `[atm].identity`
- post-send configured recipients either receive an emitted nudge or return a
  sender-visible warning
- `PostSendHookEmitter` has a machine-readable boundary TOML plus a matching
  `docs/atm-core/boundaries.md` inventory entry, and `AD.11` readiness/lint
  checks fail closed if either record is missing
- `ReconcileRuntime`, watched-file import, and daemon reconcile notification
  behavior are removed from the accepted line
- daemon notification queue/worker delivery is removed; any retained
  notification log append is direct
- local tmux-backed members use the approved local emitter
- graft-backed recipients use the approved graft emitter
- `atm-storage-claude` is removed from the accepted line
- the shared backend contract remains intact and documented as future-SQL-ready
- no approved runtime path still treats Claude inbox append as mailbox
  delivery, nudge delivery, or context injection
- active pane-id authority and repair flow runs through the existing SQLite
  roster state plus CLI, not `.atm.toml`
- the validated-on-entry roster drift for `team-lead` and `arch-ctm` is
  repaired on the accepted line
- any remaining drift category not validated on entry is surfaced with accurate
  diagnostics
- smoke and doctor coverage prove the repaired behavior on the accepted line
