# Phase AI.17–AI.21 Readiness Record

The authoritative AI.17–AI.21 sequence, dependencies, and parallel-execution
rules are in [plan-ai17-21-hermes-graft.md](plan-ai17-21-hermes-graft.md).
This file records only entry baselines, readiness states, approvals, and
closure evidence.

## Per-Sprint Closure

| Sprint | Result | Required closure |
|---|---|---|
| AI.17 | `PARTIAL` | Ambient `ATM_CHAT_ID` resolution feeds existing Phase AI `chat_id`; Hermes is the first client; no new schema or CLI grammar |
| AI.18 | `PENDING` | Python binding preserves the full supported graft host surface: typed canonical address, client operations, session lifecycle/snapshot, and canonical nudge callback |
| AI.19 | `FROZEN` | `5947858a406fbfc8b8f07487880fa13ff53bbb1e` freezes `crates/atm-graft-python/python/atm_graft_hermes_bridge.py`; reviewed/approved readiness record: `bedc1bf1` (docs-only, with no bridge source changes); configuration inputs are `ATM_IDENTITY`, `ATM_TEAM`, and optional `ATM_CHAT_ID` when constructing the typed caller, plus per-profile `PyGraftSessionOptions`; readiness probes are `just test-hermes-graft-bridge` and `PyGraftSession.snapshot()` |
| AI.20 | `PENDING` | each bridge is launchd-supervised with a reproducible runbook |
| AI.21 | `PENDING` | four production stories have complete evidence and explicit verdicts |

Allowed values are `PENDING`, `FROZEN`, `PASS`, `FAIL`, `BLOCKED`, and
`PARTIAL`. `FROZEN` is an AI.19-only pre-`PASS` record that must list its
commit SHA plus bridge module, configuration keys, and readiness probe names;
it authorizes AI.20 drafting only. `PARTIAL` and `FROZEN` are not closure.

### AI.17 PARTIAL evidence (verified against 85a6fcf6)

5 of 6 AI17-QA1 findings confirmed fixed independently: sprint-doc frontmatter
status, `--as`/`ATM_IDENTITY` OR-semantics regression, `ATM_CHAT_ID` doc
placement, error-code consistency, and env-var boundary lint coverage. One
item remains open: `IMPORTANT-01-MISSING-TEST-CATEGORIES` still lacks the
fully-absent-inputs test (all overrides and env vars unset ->
`IdentityUnavailable`). AI.17 moves to `PASS` once that single test lands and
re-verification confirms it; see
`.triage/phase-AI/findings/AI17-QA1-IMPORTANT-01-MISSING-TEST-CATEGORIES.ttl`.

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
