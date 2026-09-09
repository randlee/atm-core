---
phase: AZ
title: "Phase AZ: bounded task-nudge metadata contract"
canonical_path: docs/plans/phase-az/phase-az-plan.md
planning_branch: plan/phase-az-task-nudge-contract
integration_branch: develop
status: proposed
owner: solar
authored: 2026-09-09
baseline: develop@0ca0878cf4045546fb7f4b4f14dc6c473995158c
authority:
  - arch-ctm@atm-dev message 01M23DRDJ0CXCZH49NNAZ8EA31
  - team-lead@atm-dev message 01M23E8CGE4ENY59CY2VT079YX
---

# Phase AZ: bounded task-nudge metadata contract

## Why this phase exists

ATM nudges are wake-up metadata, not message delivery. The immutable message
body belongs in the mailbox and is retrieved only with
`atm read --message-id <id>`. Current code violates that boundary in three
places: task admission copies the rendered body into `tasks.description`, the
task-reminder builder inserts that value into a Task nudge, and the immediate
and rebuilt-queue event builders fall back from a missing summary to raw
`envelope.text`. A long ordinary message or rendered J2 template can therefore
be repeated in a nudge.

Phase AZ repairs that notification contract in one surgical sprint. Every
Steer, Queue, rebuilt Queue, Task, and task-reminder path uses persisted
message title/summary metadata. No nudge builder reads, renders, copies, or
falls back to the immutable body.

## Binding outcome

The only message-derived values permitted in a nudge are:

- `message.id`;
- `message.title`, sourced only from persisted `MessageEnvelope.summary`;
- optional `message.task-id`.

Routing identity and fixed control instructions remain nudge-envelope
metadata; they are not message content. The current XML `<description>` or
`<task>` display position may render the message title, but it must never
receive message description/body text. `atm read --message-id` is the sole
body/render path.

A missing or blank persisted title becomes the empty title. It never causes a
read of `MessageEnvelope.text`, `TaskRow.description`, template source, or
rendered J2 output. The nudge is still emitted with its message id and optional
task id.

## Scope

Phase AZ contains exactly one sprint:

| Sprint | Status | Branch | Authoritative plan |
| --- | --- | --- | --- |
| `AZ.1` | `planned` | `feature/az1-task-nudge-contract` | [`sprint-AZ.1-task-nudge-contract.md`](./sprint-AZ.1-task-nudge-contract.md) |

AZ.1 owns the one authoritative deliverables list, affected-path list,
acceptance criteria, and validation matrix. There is no second Phase AZ
sprint and no live evidence gate.

## Dependency relation

`AZ.1 must_follow develop@0ca0878cf4045546fb7f4b4f14dc6c473995158c`.
That baseline contains the Phase AX seven-template/task-reminder behavior and
the Phase AY replacement-runtime path that this repair constrains. AZ.1 is a
standalone PR to `develop`; it has no parallel Phase AZ branch and must merge
current `develop` forward before implementation and before each QA round.

## Explicit non-closure

Phase AZ does not remove or migrate `tasks.description`, redesign task-list
ordering/table output, deduplicate resends, or solve completed-task pending
nudge invalidation.

Verified current behavior is narrower: `atm send <assignee> --task-complete
<task-id> --stdin` transitions the task row to `complete`, and periodic task
reminder selection excludes complete rows, so the 60-second task-reminder loop
stops. Completion does **not** invalidate older independent pending-nudge rows
keyed by `(team, agent, message_key)`; those messages can still be claimed or
requeued.

That completed-task defect remains a separate follow-up. Its required
discovery question is: **How should task-aware pending-nudge
invalidation/supersession cover every message associated with a completed
task, without coupling that lifecycle repair to Phase AZ's notification-body
repair?**

## Architecture boundary

All implementation stays in shared core, retained storage contracts, graft
projections, and the Tokio/Axum `atm-http-runtime` composition. The frozen
legacy synchronous daemon is not modified. Validation uses unit/integration
fixtures and temporary stores only; no live daemon, test daemon, tag, release,
publish, or installation operation belongs to this phase.
