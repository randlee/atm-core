# Phase AI.17–AI.21 Readiness Record

## Entry Gate

Each sprint records the exact `integrate/phase-AI` dependency baseline and
release/version with `just lint` / `just test` green. AI.17–AI.21 consume, but
do not redefine, ADR-033, ADR-035, and ADR-037. AI.17 may run with AI.11–AI.16
after AI.5's chat-address contract; AI.19 waits specifically for AI.12's
post-write contract; AI.21 waits for AI.16 as final evidence.

## Per-Sprint Closure

| Sprint | Result | Required closure |
|---|---|---|
| AI.17 | `PENDING` | Hermes key maps to existing Phase AI `chat_id`; no new schema or CLI grammar |
| AI.18 | `PENDING` | Python binding preserves typed canonical address and graft behavior |
| AI.19 | `PENDING` | persisted write produces one Hermes nudge and canonical address maps to isolated chat |
| AI.20 | `PENDING` | each bridge is launchd-supervised with a reproducible runbook |
| AI.21 | `PENDING` | four production stories have complete evidence and explicit verdicts |

Allowed values are `PENDING`, `PASS`, `FAIL`, `BLOCKED`, and `PARTIAL`.
`PARTIAL` is not closure.

## Ordering

- AI.17 may run alongside AI.11–AI.16 after AI.5, and closes before AI.18.
- AI.18 closes before AI.19.
- AI.19 waits for AI.12, may run alongside AI.13–AI.16, and closes before AI.20 and AI.21.
- AI.20 closes before AI.21.
- AI.20 template/runbook drafting may run after AI.19's bridge contract is
  frozen, but AI.20 deployment validation and `PASS` require AI.19 `PASS`.
- A nudge test must prove persistence precedes the Hermes-visible event.
- Every message used as evidence renders the expected `agent:chat-id@team`
  address when a chat ID is present.

## Phase Closure

Phase AI’s Hermes/graft extension is ready only when all five sprints are `PASS`, every evidence row is
owned and retained, Hermes ATM chats are demonstrably isolated from
Telegram/Discord chats, and no AH component bypasses the Phase AI API,
canonical write, or post-write router.
