---
title: "Phase AX — Nudge templates on every backend and task-state tracking"
phase: AX
branch: integrate/phase-ax
status: draft
owner: fenix (plan author); team-lead (dispatch); arch-ctm / Cipher-311d (implementation)
base_revision: 5b1baacef (develop)
integration_branch: integrate/phase-ax
issue: https://github.com/randlee/atm-core/issues/1173
dependency_relations:
  - prerequisite: AX.1
    dependent: AX.2
    relation: must_follow
    rationale: AX.2 renders Queue-family templates on the Herdr pump path and both edit send/hook.rs (AX.1 changes render_built_in_nudge_for_dispatch, AX.2 changes the Herdr branch of build_built_in_dispatch).
  - prerequisite: AX.1
    dependent: AX.3
    relation: parallel_safe
    rationale: AX.3 changes no nudge behaviour and its completion message renders with either six or seven kinds; the only overlap is additive edits in different regions of crates/atm-storage/src/contract.rs, resolved by merge-forward before the AX.3 PR.
  - prerequisite: AX.2
    dependent: AX.3
    relation: parallel_safe
    rationale: no functional dependency; both add re-exports to crates/atm-core/src/boundary/mod.rs (AX.2 HerdrNudgeTarget field, AX.3 TaskStore types) in different lines, resolved by merge-forward before the AX.3 PR.
  - prerequisite: AX.2
    dependent: AX.4
    relation: must_follow
    rationale: the pump task step emits through the rendered-text path AX.2 introduces (HerdrNudgeTarget.rendered_nudge, prompt(text)).
  - prerequisite: AX.3
    dependent: AX.4
    relation: must_follow
    rationale: the pump task step reads the tasks table and TaskStore delivered by AX.3, and the write-path marker suppression relies on the task rows existing.
  - prerequisite: AX.4
    dependent: AX.5
    relation: must_follow
    rationale: the lead notification is triggered from the reminder counter AX.4 maintains inside the same pump function, and doctor ATM_TASK_STALLED reads it.
  - prerequisite: AX.5
    dependent: AX.6
    relation: must_follow
    rationale: AX.6 is a proof sprint on the merged integrate head.
execution_tracks:
  - track: A
    sprints: [AX.1, AX.2]
    stack: gh-stack A rooted on integrate/phase-ax
  - track: B
    sprints: [AX.3]
    stack: gh-stack B rooted on integrate/phase-ax
    parallel_with: A
  - track: C
    sprints: [AX.4, AX.5]
    stack: gh-stack C rooted on integrate/phase-ax after A and B merge
  - track: D
    sprints: [AX.6]
    stack: none (proof on the integrate head)
---

# Phase AX — Nudge templates on every backend and task-state tracking

> Evidence base: fenix's Herdr dogfood session on rand-m5, 2026-09-04
> (atm 1.4.13 from develop 9d49ce38d, herdr 0.8.2, team `atm-dev` fully
> migrated to `--backend herdr`). Findings recorded on issue #1173 and its
> review comment.

## 1. Problem

The built-in nudge templates are backend-agnostic by design: they define
what every agent receives on a nudge, whichever local receiver (tmux,
Herdr, graft) delivers it. Three defects break that today.

1. **Herdr bypasses the template layer.** `send/hook.rs` renders the
   database-resolved template for the tmux and graft sinks, but the Herdr
   branch builds `HerdrNudgeTarget { session }` with no rendered text, and
   `atm-herdr` hard-codes `HERDR_WAKE_TEXT` ("You have unread ATM messages.
   Run: atm read") as the `herdr agent prompt` argument. A Herdr-backed
   agent never sees the sender, message id, description, or ack
   instruction. ADR-058 D2/D4 document the fixed text as the contract.
2. **No queue template class.** `built_in_nudge_template_kind_from_post_send_event`
   derives the kind from `(is_ack, task_id, requires_ack)` only. The
   `NudgeKind::{Steer, Queue}` value is computed in the hook and carried on
   the dispatch but is not an input to template selection, so `atm queue`
   reuses the Delivery templates, including the `<when idle="immediate"
   busy="after-current-task"/>` element that only makes sense for a steer.
3. **No task tracking.** A task-tagged message is ordinary mail with a
   forced ack requirement. Nothing records whether the assignee accepted
   it, nothing stops a second task from being acked while the first is
   in progress, and an idle assignee with an open task is never reminded.

Minor defects in the default template bodies found in the same review:

- Every delivery template's read action says `read atm --team {{team}}`;
  the verb is `atm read`.
- The read action does not target the message. Newest-first selection plus
  the seen watermark stranded a requires-ack message (dogfood test 7a)
  behind a newer one.

## 2. Decisions (Rand, 2026-09-04 and 2026-09-05, binding)

- No architecture change beyond **adding a queue template class** (AX.1)
  and **task-state tracking** (AX.3–AX.5). The existing message model,
  placeholders (`from`, `team`, `message_id`, `description`, `task_id`),
  renderer, and override store are reused.
- Tasks are always queued and always require ack, so the template surface
  is seven kinds: `delivery`, `delivery_ack`, `queue`, `queue_ack`,
  `task`, `acknowledge`, `acknowledge_task`. `delivery_task` and
  `delivery_task_ack` are retired.
- The `kind` of a nudge indicates **when** it is delivered, not what it
  says. No `kind="steer"` or `kind="queue"` attribute is added. The
  existing `kind="ack"` on the two ack templates stays.
- Every delivery, queue, and task template keeps a **call to action**
  (`execute the assigned task`).
- The read action is a **targeted read**: `atm read --message-id
  {{message_id}}`, no `--team`.
- The queue template is the delivery template **without `<when>`**.
- Ack templates are unchanged.
- Committed CI evidence records under `docs/plans/phase-aq/evidence/`
  that contain the old `read atm` text are **not** edited.
- Task tracking: one ACTIVE task per assignee; the next ASSIGNED task is
  surfaced only after `--task-complete` on the current one; an idle
  assignee with an open task is reminded at most once per 60 s; every
  tenth reminder notifies the `lead`; doctor warns when a team has no lead.
- The rules are simple state machines: explicit states, explicit events,
  one pure transition function, every transition and every reminder an
  append-only event row. The pump never changes task state.

Plan-author decisions recorded in the same list (2026-09-05, from the
cycle-1 hardening findings; binding unless Rand objects at plan review):

- The `team_nudge_template_overrides` table has a six-value `CHECK` on
  `template_kind`; SQLite cannot alter it, so AX.1 rebuilds the table
  with the seven-value `CHECK`. Rows holding a retired kind cannot be
  copied into the rebuilt table and are dropped by the migration with one
  retained-log warning each. The migration runs on every database open.
- The unused `atm_storage::contract::TaskState(String)` newtype (no
  callers in the workspace beyond three `pub use` lines) is retired in
  AX.3; the name is taken by the new state enum.
- New task identifiers avoid the `nudge` word family (`reminder`,
  `Reminded`, `last_reminded_at`) so the frozen inventory in
  `scripts/check-nudge-taxonomy.py` does not grow.
- Task-tagged mail to a Herdr-backed recipient carries **no** pending-nudge
  marker and no immediate steer (AX.4); the durable `tasks` row is its
  delivery record and the pump task step is its only nudge source. This is
  a recorded exception to ADR-054's marker contract (dated ADR-054
  amendment plus ADR-061). Daemon-down behaviour is identical to queued
  mail today: nothing is nudged until the daemon pump runs.
- `TaskStore` is the seventh optional storage capability trait under
  ADR-018 §3 as re-counted by ADR-054; ADR-061 is the follow-up ADR that
  rule requires.
- Doctor gains three warning codes: `ATM_ROSTER_NO_LEAD` (Rand),
  `ATM_ROSTER_MULTIPLE_LEADS`, and `ATM_TASK_STALLED` (open task with ten
  or more reminders).
- The reserved sender name `atm-daemon` is used for the lead notification
  and is rejected by `add-member` / `update-member`.

### 2.1 Binding defaults (alternatives recorded for the record)

The sprint docs implement the default column. The alternatives are
follow-up options on #1173, not open questions.

| Item | Binding default | Alternative recorded |
| --- | --- | --- |
| Who may close a task the assignee never completes | the **assigner** may send `atm send <assignee> --task-complete <id>`; actor recorded in `task_events` | a distinct `--task-cancel` flag |
| Sender identity on the lead notification | reserved sender name `atm-daemon`, never a roster member | doctor warning and log record only |
| Re-sending an open task id to the same assignee | accepted: state unchanged, `assignment_message_id` and `description` updated, one `Resent` event row | rejected as a duplicate |
| Completing a task that was never acked | accepted; the assignment message is marked acknowledged in the same transaction so it does not stay pending-ack forever | reject and require an ack first |

## 3. Sprints and execution tracks

| Sprint | Track | Execute | Title | Owns | Doc |
| --- | --- | --- | --- | --- | --- |
| AX.1 | A | **parallel with AX.3** | Queue template class and default template fixes | `BuiltInNudgeTemplateKind`, override-table migration, kind selection, default bodies, CLI kind strings, six-kind statements in docs and ADR-019 amendment, nudge scripts | `sprint-AX.1-queue-template-class.md` |
| AX.2 | A | after AX.1; **parallel with AX.3** | Herdr renders the built-in template | `HerdrNudgeTarget`, `send/hook.rs` Herdr branch, `HerdrProcessAdapter::prompt` and its three impls, bootstrap selector call site, ADR-058 amendment, Herdr boundary record | `sprint-AX.2-herdr-template-rendering.md` |
| AX.3 | B | **parallel with AX.1 and AX.2** | Task state machine and completion | `atm-storage` task types and pure transition, `TaskStore`, rusqlite tables and in-transaction application, ack gate, `--task-complete`, `atm list --tasks`, ADR-061, ADR-054 amendment, requirements §6.5/§7/§15.4 | `sprint-AX.3-task-state-machine.md` |
| AX.4 | C | after AX.2 and AX.3 merge | Task reminder cycle in the Herdr pump | pump task step and idle-set widening, task-row reminder rendering, Herdr task-mail steer and marker suppression, bootstrap composition | `sprint-AX.4-task-reminder-cycle.md` |
| AX.5 | C | after AX.4 | Lead notification and doctor | `atm-daemon` reserved sender, lead message, four doctor codes with catalog guidance | `sprint-AX.5-lead-notification-doctor.md` |
| AX.6 | D | after AX.5 merges | Live Herdr dogfood evidence | `docs/plans/phase-ax/ax6-live-proof.md` | `sprint-AX.6-herdr-dogfood-evidence.md` |

Dependency graph:

```
integrate/phase-ax
 ├── track A: AX.1 ──► AX.2 ──┐
 │                            ├──► track C: AX.4 ──► AX.5 ──► track D: AX.6
 └── track B: AX.3 ───────────┘
```

Tracks A and B **execute in parallel** from day one. `must_follow` edges:
AX.1→AX.2, AX.2→AX.4, AX.3→AX.4, AX.4→AX.5, AX.5→AX.6. `parallel_safe`
pairs: AX.1∥AX.3, AX.2∥AX.3 (frontmatter carries the rationale). The
price of the parallel pair is one merge-forward of `integrate/phase-ax`
into the AX.3 branch after track A merges, with a trivial conflict in
`crates/atm-core/src/boundary/mod.rs` where both add lines.

## 4. Acceptance contract for the phase

Each item names the sprint that closes it; the sprint doc carries the
test or evidence gate.

1. A Herdr-backed member receives the same rendered XML a tmux-backed
   member receives for the same send, byte-for-byte apart from transport.
   (AX.2 unit test; AX.6 live evidence.)
2. `atm send` to an idle Herdr member renders Delivery / DeliveryAck;
   `atm queue` renders Queue / QueueAck with no `<when>`; a task-tagged
   send renders Task. (AX.1 tests; AX.2 for the Herdr emitter.)
3. `atm ack` renders the unchanged Acknowledge / AcknowledgeTask
   template. (AX.1.)
4. `atm teams set-nudge-template` accepts `queue`, `queue_ack`, `task`,
   rejects the retired kinds, the override table accepts the new kinds on
   an upgraded database, and the override is honoured on tmux, Herdr, and
   graft. (AX.1; AX.2 for Herdr.)
5. No default template body and no published example contains
   `read atm --team`; every delivery, queue, and task read action is
   `atm read --message-id {{message_id}}`; no normative doc states a
   six-kind bound. (AX.1, gates in its AC 2.)
6. `HERDR_WAKE_TEXT` and `prompt_text_is_fixed_and_non_empty` are gone;
   `herdr agent prompt` argv keeps exactly four elements with the rendered
   template as the fourth; ADR-058 D2/D4 state the prompt text is the
   rendered template. (AX.2.)
7. Task state is ASSIGNED → ACTIVE → COMPLETE with the transition table
   in AX.3; `atm ack` of a second task while one is ACTIVE is rejected
   with exit 3; `--task-complete` that resolves to no open row for the
   caller is rejected with exit 3; `task_events` replays to the `tasks`
   table per (team, task_id, assignee). (AX.3.)
8. An idle Herdr assignee with an open task receives one Task reminder on
   the first idle tick after assignment and then at most one per 60 s;
   the second task is reminded only after the first is COMPLETE; at most
   one Herdr prompt per member per tick. (AX.4; AX.6 live evidence.)
9. Every tenth reminder sends one queued message from `atm-daemon` to the
   lead; doctor warns `ATM_ROSTER_NO_LEAD` on a team with no lead and
   `ATM_TASK_STALLED` on an open task with ten or more reminders. (AX.5
   tests; AX.6 case C14.)
10. `just validate` green on the integrate head, including
    `scripts/check-nudge-taxonomy.py` with an unchanged allowlist;
    `boundary-guard` review of `boundaries/atm-storage/task-store.toml`
    and `boundaries/atm-herdr/herdr-process-adapter.toml`. (AX.3, AX.2;
    phase PR.)

## 5. Out of scope (tracked as follow-ups on #1173)

- Seen watermark advancing past older unread messages on a one-shot read.
- Re-nudge path for non-task immediate-mode sends after the steer is
  consumed.
- Back-to-back Herdr steers coalescing inside Herdr's 300 ms Enter delay
  (AX.4 avoids triggering it by serialising one prompt per member per
  tick; the underlying Herdr behaviour is unchanged).
- `atm doctor` not emitting `ATM_ROSTER_MIXED_LOCAL_BACKEND` for a split
  team.
- Herdr steer and failed tmux hook not recorded in the shared JSONL log.
- New placeholders (unread count) or renderer conditionals.
- Task reminders for tmux and graft members (state tracking applies to
  them; only the Herdr pump reminds in this phase).
- Cross-host and cross-team task assignment: task transitions are applied
  only to locally originated writes; peer receipts never create or reject
  task state.
- Task priority, reassignment, expiry, configurable intervals.

## 6. Execution notes

- Development happens in worktrees created by `sc-git-worktree`; PR
  bases and bottom-to-top merging are managed by `gh stack`. `gh stack`
  navigation, `init`, `checkout`, `rebase`, and `sync` are **not used**:
  they switch or rewrite branches in one checkout, which conflicts with
  the per-branch worktrees and the merge-commit policy. Merge-forward
  replaces rebase.
- PRs target `integrate/phase-ax` (bottom of each stack) or the branch
  below them in the stack; merge commits only (`--merge`); the phase PR
  `integrate/phase-ax → develop` is opened after AX.6.
- team-lead dispatches via j2 template over `atm send --stdin`;
  quality-mgr gates each PR with a posted Final Quality Report. `gh
  stack merge` checks only open/not-draft, so it is run only after every
  PR in the stack has its report posted.
- Sequence (team-lead, from the main repo unless noted):

```bash
# Day one: tracks A and B in parallel
/sc-git-worktree --create feature/ax1-queue-template-class integrate/phase-ax
/sc-git-worktree --create feature/ax3-task-state-machine integrate/phase-ax
git push -u origin feature/ax1-queue-template-class feature/ax3-task-state-machine
gh stack link feature/ax1-queue-template-class                    # stack A
gh stack link feature/ax3-task-state-machine                      # stack B
# dispatch AX.1 (Cipher-311d) and AX.3 (arch-ctm) in the same batch

# When AX.1 dev is pushed: AX.2 stacks on AX.1
/sc-git-worktree --create feature/ax2-herdr-template-rendering feature/ax1-queue-template-class
gh stack link feature/ax1-queue-template-class feature/ax2-herdr-template-rendering
gh stack submit --auto --open --remote origin                     # AX.1 → integrate, AX.2 → AX.1
# in the AX.2 worktree before every fix round:
#   git merge origin/feature/ax1-queue-template-class

# Track A merge (both QA reports posted)
gh stack merge <AX.2 PR#> --yes --merge

# Track B merge: AX.3 merges integrate forward first (boundary/mod.rs overlap), then
gh stack submit --auto --open --remote origin                     # from the AX.3 worktree
gh stack merge <AX.3 PR#> --yes --merge

# Track C: after A and B are in integrate
/sc-git-worktree --create feature/ax4-task-reminder-cycle integrate/phase-ax
# when AX.4 dev is pushed:
/sc-git-worktree --create feature/ax5-lead-notification-doctor feature/ax4-task-reminder-cycle
gh stack link feature/ax4-task-reminder-cycle feature/ax5-lead-notification-doctor
gh stack submit --auto --open --remote origin
gh stack merge <AX.5 PR#> --yes --merge

# Track D: AX.6 on the integrate head, then the phase PR to develop
```

- `must_follow` merge-forward inside a stack: merge the branch below into
  the branch above before every dev or fix round once the lower branch's
  development is pushed; the lower PR merges first (`gh stack merge`
  does this bottom to top).
- AX.6 runs on rand-m5 under fenix; the daemon is rebuilt from the
  integrate head into `~/.atm-builds/` (outside `~/Documents`) for the
  live proof.
- `docs/project-plan.md` §55 (added with this plan) tracks sprint status.
