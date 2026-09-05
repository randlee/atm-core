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
    rationale: AX.2 renders Queue-family templates on the Herdr pump path; the kinds must exist first, and both edit send/hook.rs.
  - prerequisite: AX.1
    dependent: AX.4a
    relation: must_follow
    rationale: both edit crates/atm-storage/src/contract.rs and docs/user-documents; AX.4a's completion message relies on the AX.1 kind mapping.
  - prerequisite: AX.2
    dependent: AX.4a
    relation: parallel_safe
    rationale: AX.2 owns hook.rs, HerdrNudgeTarget, atm-herdr, the bootstrap selector, and the pump; AX.4a owns the write pipeline, ack path, storage task tables, and CLI list/send/ack flags. No shared files, contracts, or artifacts.
  - prerequisite: AX.2
    dependent: AX.4b
    relation: must_follow
    rationale: AX.4b extends the pump tick that AX.2 changes to pass rendered text.
  - prerequisite: AX.4a
    dependent: AX.4b
    relation: must_follow
    rationale: the pump reads the tasks table and the write-path steer suppression AX.4b adds depends on the task rows AX.4a persists.
  - prerequisite: AX.4b
    dependent: AX.3
    relation: must_follow
    rationale: AX.3 is a proof sprint on the merged integrate head.
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
  and **task-state tracking** (AX.4a/AX.4b). The existing message model,
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
  assignee with an open task is nudged at most once per 60 s; every
  tenth nudge notifies the `lead`; doctor warns when a team has no lead.
- The rules are simple state machines: explicit states, explicit events,
  one pure transition function, every transition and every nudge an
  append-only event row. The pump never changes task state.

### 2.1 Decisions to confirm at plan review

Defaults chosen so every sprint is implementable as written. Either
confirmation or an alternative is a wording change, not a re-plan.

| Item | Default in this plan | Alternative |
| --- | --- | --- |
| Who may close a task that the assignee never completes | the **assigner** may also send `--task-complete <id>`; actor recorded in `task_events` | a distinct `--task-cancel` flag |
| Sender identity on the lead notification | reserved sender name `atm-daemon`, never a roster member | notification is a doctor warning and log record only |
| Re-sending an open task id to the same assignee | accepted as a re-send: state unchanged, assignment message id updated, `Resent` event | rejected as a duplicate |

## 3. Sprints

| Sprint | Title | Owns | Doc |
| --- | --- | --- | --- |
| AX.1 | Queue template class and default template fixes | `BuiltInNudgeTemplateKind`, kind selection, default bodies, CLI kind strings, user docs and examples | `sprint-AX.1-queue-template-class.md` |
| AX.2 | Herdr renders the built-in template | `HerdrNudgeTarget`, `send/hook.rs` Herdr branch, `atm-herdr` prompt text, bootstrap selector, pump emit, ADR-058 amendment | `sprint-AX.2-herdr-template-rendering.md` |
| AX.4a | Task state machine and completion | `atm-storage` task types and pure transition, `TaskStore`, rusqlite tables and in-transaction application, ack gate, `--task-complete`, `atm list --tasks`, ADR-061, requirements §15.4 | `sprint-AX.4a-task-state-machine.md` |
| AX.4b | Task nudge cycle, lead notification, doctor | Herdr pump task step, Herdr task-send steer suppression, `atm-daemon` lead message, three doctor codes | `sprint-AX.4b-task-nudge-cycle.md` |
| AX.3 | Live Herdr dogfood evidence | `docs/plans/phase-ax/ax3-live-proof.md` | `sprint-AX.3-herdr-dogfood-evidence.md` |

Dependency graph: AX.1 → {AX.2 ∥ AX.4a} → AX.4b → AX.3. The AX.2/AX.4a
pair is parallel-safe because their file sets, contracts, and artifacts do
not intersect (see frontmatter rationale).

## 4. Acceptance contract for the phase

Each item names the sprint that closes it; the sprint doc carries the
test or evidence gate.

1. A Herdr-backed member receives the same rendered XML a tmux-backed
   member receives for the same send, byte-for-byte apart from transport.
   (AX.2 unit test; AX.3 live evidence.)
2. `atm send` to an idle Herdr member renders Delivery / DeliveryAck;
   `atm queue` renders Queue / QueueAck with no `<when>`; a task-tagged
   send renders Task. (AX.1 tests; AX.2 for the Herdr emitter.)
3. `atm ack` renders the unchanged Acknowledge / AcknowledgeTask
   template. (AX.1.)
4. `atm teams set-nudge-template` accepts `queue`, `queue_ack`, `task`,
   rejects the retired kinds, and the override is honoured on tmux,
   Herdr, and graft. (AX.1; AX.2 for Herdr.)
5. No default template contains `read atm`; every delivery, queue, and
   task read action is `atm read --message-id {{message_id}}`. (AX.1.)
6. `HERDR_WAKE_TEXT` and `prompt_text_is_fixed_and_non_empty` are gone;
   ADR-058 D2/D4 state the prompt text is the rendered template. (AX.2.)
7. Task state is ASSIGNED → ACTIVE → COMPLETE with the transition table
   in AX.4a; `atm ack` of a second task while one is ACTIVE is rejected
   with exit 3; `--task-complete` naming a task not open for the caller
   is rejected with exit 3; `task_events` replays to the `tasks` table.
   (AX.4a.)
8. An idle Herdr assignee with an open task receives one Task nudge
   immediately and then at most one per 60 s; the second task is nudged
   only after the first is COMPLETE; every tenth nudge sends one message
   to the lead; doctor warns `ATM_ROSTER_NO_LEAD` on a team with no lead.
   (AX.4b; AX.3 live evidence.)
9. `just validate` green on the integrate head; `boundary-guard` review of
   `boundaries/atm-storage/task-store.toml`. (AX.4a; phase PR.)

## 5. Out of scope (tracked as follow-ups on #1173)

- Seen watermark advancing past older unread messages on a one-shot read.
- Re-nudge path for non-task immediate-mode sends after the steer is
  consumed.
- Back-to-back Herdr steers coalescing inside Herdr's 300 ms Enter delay.
- `atm doctor` not emitting `ATM_ROSTER_MIXED_LOCAL_BACKEND` for a split
  team.
- Herdr steer and failed tmux hook not recorded in the shared JSONL log.
- New placeholders (unread count) or renderer conditionals.
- Task re-nudge for tmux and graft members (state tracking applies to
  them; only the Herdr pump re-nudges in this phase).
- Cross-team task assignment; task priority, reassignment, expiry,
  configurable intervals.

## 6. Execution notes

- All sprint branches are worktrees off `integrate/phase-ax` via
  `sc-git-worktree`; PRs target `integrate/phase-ax`; merge commits only.
- team-lead dispatches via j2 template over `atm send --stdin`;
  quality-mgr gates each PR with a posted Final Quality Report.
- `must_follow` merge-forward: merge the parent branch into the child
  before every dev or fix round once the parent's development is pushed.
- AX.3 runs on rand-m5 under fenix; the daemon is rebuilt from the
  integrate head into `~/.atm-builds/` (outside `~/Documents`) for the
  live proof.
- `docs/project-plan.md` §55 (added with this plan) tracks sprint status.
