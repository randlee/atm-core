---
id: AH.5
title: Four-Story Closure Validation
status: planned
branch: feature/pAH-s5-four-story-closure
worktree: ../atm-core-worktrees/feature/pAH-s5-four-story-closure
target: develop
---

# Sprint AH.5 — Four-Story Closure Validation

```yaml
plan_type: sprint_plan
phase: AH
sprint: AH.5
worktree: ../atm-core-worktrees/feature/pAH-s5-four-story-closure
branch: feature/pAH-s5-four-story-closure
status: planned
estimated_scope: medium
```

## Goal

Prove each of the four motivating user stories works end to end on the
production baseline produced by AH.1–AH.4.

This sprint is evidence-only. It does not land new code; it lands the
validation evidence package that proves Phase AH is release-ready for its
scope.

## Hard Dependencies

- AH.1, AH.2, AH.3, AH.4 are all `PASS`
- all four Hermes profiles are running with bridge processes up
- atm-daemon 1.3.1+ is running on the host

## Exact Targets

- `docs/plans/phase-AH/four-story-validation.md` — the authoritative closure-evidence
  index (produced as part of this sprint)
- retained evidence packages per story:
  - transcript of each `atm send` / `atm read` / Hermes-side action
  - Hermes session ID and message count before/after
  - bridge-process log snapshot
  - latency measurement (for story 3)
  - multi-turn context retention evidence (for stories 1 and 4)

## Stories

### Story 1 — Multi-Turn ATM Conversation (Telegram)

User asks team-lead@atm-dev a question via `atm send` with `--requires-ack`.
team-lead reads the question from their ATM inbox (in the Hermes session
keyed by `atm:{user}:{session_id}`), answers via `atm send` back to the
user. Hermes wakes up, processes the answer in the same context as the
original question, and continues the conversation.

Success criteria:

- at least 3 round-trip turns of conversation persist in the same Hermes
  session on both sides
- the session_id round-trips transparently (no `--session` flag passed by
  either side)
- the user's Hermes display form `hendrix:telegram:8991600178@hermes` is
  visible to team-lead in every inbound message from the conversation
- `atm read` on the relevant session_id scope returns all 3+ turns

### Story 2 — Nightly Cron → ATM Response → Cron Continues

Cron triggers `atm send` to a Hermes agent (e.g., hendrix) with
`--requires-ack`. Hermes wakes up, processes the cron-initiated question,
sends response via `atm send` back to the cron session. Cron session
maintains context and continues working.

Success criteria:

- at least 2 round-trip turns of conversation persist in the cron session
- latency of Hermes response is <60 seconds
- the cron session can act on the Hermes response without losing prior
  context

### Story 3 — PR Approval via ATM Nudge in Minutes

External event (e.g., a Hermes cron that monitors a PR or a direct
`atm send` from a teammate requesting review) lands on a Hermes agent as
an ATM nudge. Hermes reviews the PR, approves it (or comments), and the
sender receives a durable ack.

Success criteria:

- end-to-end latency from `atm send` requiring review to Hermes reply is
  <5 minutes (measured; reported in the evidence package)
- Hermes side can invoke the `gh` CLI to view and act on the PR
- sender receives a durable ack from Hermes
- no manual intervention required between send and response

### Story 4 — Design Question Between ATM Agents

One Hermes agent asks another for design info via `atm send` (no
`--session`, no `--requires-ack`). Receiving agent wakes, reads the
question in its Hermes session, answers via `atm send` back. Sender can
continue a follow-up in the same session.

Success criteria:

- round-trip completes with session-scoped `atm read` returning both
  question and answer in the same session scope on the receiving side
- sender's `atm read` default mode returns both turns in the same session
  scope
- `atm read --agent <peer>` returns the conversation on both sides

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims.

- `docs/plans/phase-AH/four-story-validation.md` — closure evidence index
  covering all four stories with the structure documented below
- per-story evidence packages as listed above
- phase `readiness.md` update marking AH.5 `PASS` and recording the
  per-story verdicts
- the four-story-validation.md artifact lists:
  - story 1–4 identifiers
  - the Hermes session key for each side of each story
  - the session_id that was round-tripped
  - the transcript, Hermes session state, and bridge log snapshots
  - the latency measurement for story 3
  - the per-story verdict: `PASS`, `FAIL`, or `PARTIAL`

## Required Work

### Story 1 execution

Run story 1 against the production Hermes install:

- `atm send arch-ctm "design question 1" --requires-ack`
- arch-ctm's Hermes session wakes, processes, replies
- repeat for 3+ turns
- capture transcripts + Hermes session state + bridge logs
- verify session_id round-trips (no `--session` flag used)
- verify `atm read` session-scope returns all turns on both sides

### Story 2 execution

Trigger a Hermes cron that sends an ATM question to a Hermes profile,
receives the response, and continues processing.

- schedule a one-shot cron job with `atm send hermes "cron question"`
- capture Hermes session state before and after response
- verify session continuity in the cron session

### Story 3 execution

Trigger a PR review via ATM and measure end-to-end latency.

- `atm send hendrix "review PR #X" --requires-ack`
- Hermes wakes, runs `gh pr view`, runs `gh pr review --approve`, sends
  ack back
- measure latency from `atm send` to the ack-receipt

### Story 4 execution

Run a multi-turn design question between two Hermes agents.

- `atm send alpha-prime "how does X work?"` (no `--session`, no
  `--requires-ack`)
- alpha-prime's Hermes wakes, answers
- send follow-up on the same session
- verify `atm read --agent <peer>` returns the full conversation on both
  sides

### Evidence assembly

After all four stories complete, assemble:

- `docs/plans/phase-AH/four-story-validation.md` with all evidence indexed
- per-story sub-files (as needed) containing full transcripts, state
  dumps, bridge logs
- final verdict table in `readiness.md`

## Non-Closure

This sprint does not:

- introduce new code
- add new query modes
- change Hermes channel behavior
- add new launchd plists

This sprint reports on the state delivered by AH.1–AH.4.

## Acceptance Criteria

- all four stories are evidenced with full transcripts and Hermes session
  state
- story 1, 4 have ≥3 turns captured in the same session scope
- story 3 latency is under 5 minutes
- story 2 shows session continuity in the cron session
- `atm read` session-scoped returns the right turns on both sides for
  stories 1 and 4
- `atm read --agent <peer>` returns the conversation across sessions for
  story 4
- the four-story-validation.md artifact lists every story with a clear
  verdict

## Required Validation

- every evidence artifact passes a `quality-mgr` review for completeness
  (transcripts present, session state present, verdicts explicit)
- `git diff --check` on the evidence commit
