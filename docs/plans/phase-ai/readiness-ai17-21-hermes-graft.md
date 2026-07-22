# Phase AI.17–AI.21 Readiness Record

The authoritative AI.17–AI.21 sequence, dependencies, and parallel-execution
rules are in [plan-ai17-21-hermes-graft.md](plan-ai17-21-hermes-graft.md).
This file records only entry baselines, readiness states, approvals, and
closure evidence.

## Per-Sprint Closure

| Sprint | Result | Required closure |
|---|---|---|
| AI.17 | `PENDING` | Ambient `ATM_CHAT_ID` resolution feeds existing Phase AI `chat_id`; Hermes is the first client; no new schema or CLI grammar |
| AI.18 | `PENDING` | Python binding preserves typed canonical address and graft behavior |
| AI.19 | `PENDING` | persisted write produces one Hermes nudge and canonical address maps to isolated chat |
| AI.20 | `PENDING` | each bridge is launchd-supervised with a reproducible runbook |
| AI.21 | `PENDING` | four production stories have complete evidence and explicit verdicts |

Allowed values are `PENDING`, `FROZEN`, `PASS`, `FAIL`, `BLOCKED`, and
`PARTIAL`. `FROZEN` is an AI.19-only pre-`PASS` record that must list its
commit SHA plus bridge module, configuration keys, and readiness probe names;
it authorizes AI.20 drafting only. `PARTIAL` and `FROZEN` are not closure.

## Required records

- Every sprint records its exact `integrate/phase-AI` dependency baseline,
  release/version, and green `just lint` / `just test` output.
- Before AI.19 starts, the owner records approval of the expanded scope named
  in the authoritative plan.
- If an AI.19 frozen surface changes before `PASS`, replace the `FROZEN`
  record and record AI.20's rebase to the replacement SHA.
- A nudge test must prove persistence precedes the Hermes-visible event.
- Every message used as evidence renders the expected `agent:chat-id@team`
  address when a chat ID is present.

## Phase Closure

Phase AI’s Hermes/graft extension is ready only when all five sprints are `PASS`, every evidence row is
owned and retained, Hermes ATM chats are demonstrably isolated from
Telegram/Discord chats, and no AH component bypasses the Phase AI API,
canonical write, or post-write router.
