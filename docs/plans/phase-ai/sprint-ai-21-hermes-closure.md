---
id: AI.21
title: Four-Story Closure Validation
status: planned
branch: feature/pAI-s21-hermes-closure
worktree: ../atm-core-worktrees/feature/pAI-s21-hermes-closure
target: integrate/phase-AI
---

# Sprint AI.21 — Four-Story Closure Validation

## Goal

Produce evidence that the deployed AI.17–AI.20 line supports the intended
Hermes workflows. This is evidence-only: a failed row is a finding, not an
authorization to add unplanned code.

## Hard Dependencies

- AI.16 and AI.17 through AI.20 are `PASS`.
- The running daemon and CLI are the recorded Phase AI-derived release.

## Stories and Required Evidence

1. **Multi-turn ATM conversation:** three or more turns between qualified
   Hermes identities stay in one `atm:` chat per side.
2. **Cron continuation:** a Hermes cron-originated qualified identity receives
   an ATM reply and resumes in its original Hermes chat.
3. **PR review nudge:** a durable ATM write causes a Hermes in-process nudge,
   followed by a reply/ack using the same canonical write path.
4. **Hermes-to-Hermes design question:** a no-ack conversation and follow-up
   preserve the distinct source chat identity and remain queryable through
   Phase AI’s existing `--chat-id` / `--as` semantics.

For every story retain: exact commands, release/commit, message IDs, rendered
source and destination addresses, persisted-before-nudge observation, source
chat IDs, bridge logs, and `PASS`/`FAIL` verdict. Measure the PR-review
latency; do not invent an unverified target.

## Evidence Matrix

| Story | Required assertion |
|---|---|
| 1 | Three turns retain the same qualified source address and one Hermes `atm:` chat on each side. |
| 2 | A cron-originated `agent:chat-id` reply is read through the same caller identity and resumes that chat. |
| 3 | A normal write is readable before its Hermes nudge; the Hermes reply/ack is a canonical write. |
| 4 | `--chat-id` and equivalent `--as` forms select the same context; two different chat IDs stay isolated. |

## Non-Closure

AI.21 does not create query flags, schema fields, custom headers, new transport
logic, or launchd configuration. It only records evidence against AI.17–AI.20.

## Parallel Execution

AI.21 is not parallelizable: it is final evidence and begins only after its
AI.16 and AI.17–AI.20 dependencies are `PASS`.

## Closure

- All four stories pass with complete evidence.
- ATM chats are demonstrated isolated from non-ATM Hermes chats.
- `readiness-ai17-21-hermes-graft.md` is updated with per-story results.
- `git diff --check` passes and the evidence package receives independent
  review.
