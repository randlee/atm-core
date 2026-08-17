---
title: AI.38 Hermes steer nudge delivery
status: complete
branch: feature/pAI-s38-hermes-steer-nudge-delivery
worktree: feature/pAI-s38-hermes-steer-nudge-delivery
target: integrate/phase-ai-31-33
depends_on: AI.37
---

# AI.38 — Hermes steer nudge delivery

```yaml
plan_type: sprint_plan
phase: AI
sprint: AI.38
worktree: feature/pAI-s38-hermes-steer-nudge-delivery
branch: feature/pAI-s38-hermes-steer-nudge-delivery
status: complete
estimated_scope: focused new Python host adapter plus checked-in reference evidence
```

## Goal

Deliver live graft nudges and AI.37's one-shot recovery summary through the
configured Hermes profile's non-interrupting steer path so an active agent sees
ATM work at its next safe API/tool-loop boundary without normal user-message
interruption.

## Scope Summary

This sprint creates the currently absent
`crates/atm-graft-python/python/atm_graft_hermes_adapter.py` and the narrow
`HermesSteerPort` dispatch it consumes. The existing checked-in
`atm_graft_hermes_bridge.py` remains a generic typed callback bridge. The
`MessageEvent` normal-ingress pattern is an external Hermes pattern that this
new adapter must not adopt. This sprint does not create a Hermes conversation
manager, a multi-channel registry, another ATM transport, or an ATM message
read/ack workflow.

## Governing Requirements

- `REQ-GRAFT-HERMES-002`
- `REQ-GRAFT-HERMES-003`
- `REQ-GRAFT-NOTIFY-002`
- `REQ-CORE-GRAFT-001`

## Governing ADRs

- ADR-037 — configured `ChatId` remains the host session binding
- ADR-039 — Python binding stays a translation over the graft client
- ADR-043 — wake-ups use steer, not normal user-message ingress

## Governing Boundaries

- `atm_graft_hermes_adapter.py` owns the Hermes-specific adapter.
- `atm_graft_hermes_bridge.py` owns typed nudge de-duplication and callbacks.
- Hermes owns the final steer API and its agent scheduling semantics.
- `atm-graft` remains host-neutral and owns neither Telegram nor Hermes state.

## Prerequisites

- AI.36 and AI.37 are merged.
- Before coding, record the exact supported Hermes steer callable and its
  result/error contract from the installed Hermes source; do not infer it from
  a normal gateway message handler.

## Hard Dependencies

- This sprint is the closure point for non-interrupting Hermes wake-up
  evidence.

## Non-Goals

- No multiple-channel/multiple-chat routing policy for one ATM agent.
- No synthetic Telegram user event, `MessageEvent`, or normal inbound
  authorization path for ATM nudges.
- No direct ATM read/send/ack performed by the adapter after receiving a
  nudge; the agent's existing ATM skills decide the follow-up work.

## Sub-Tasks

### 1. Introduce a narrow steer boundary

Development work:

1. First commit sets all releasable assemblies to `1.4.0-beta-ai.38`.
2. Create `atm_graft_hermes_adapter.py` with one injected async steer port
   tied to the configured profile `ChatId`; do not add a normal-message
   adapter first.
3. Bind that port to Hermes's documented non-interrupting steer API. The
   adapter must await/report its result but must never fall back to normal
   user-message dispatch.

Required shape:

```python
class HermesSteerPort(Protocol):
    async def steer(self, *, chat_id: str, text: str) -> None: ...

async def _inject_steer(self, text: str) -> None:
    await self._steer_port.steer(chat_id=self._chat_id, text=text)
```

4. Use `_inject_steer` for both the typed live nudge body and AI.37 recovery
   summary. The sender address/chat-id is retained in bounded attribution
   metadata/logging; it does not become a second Hermes session key.

Required tests:

- fake steer port records configured `ATM_CHAT_ID` and exact text for a live
  nudge and recovery notice;
- a sentinel normal-message handler fails the test if called;
- no fallback occurs when steer fails; the failure is structured and visible.

### 2. Preserve one-profile identity without premature multi-channel work

Development work:

1. Validate the configured `ATM_CHAT_ID` once at adapter connection. Resolve
   it through the host's Hermes registration map to the opaque runtime session
   id returned by Hermes, and pass only that runtime id to `session.steer`.
   Never use the platform chat id as a TUI session id or silently fall back to
   it when a binding is missing.
2. Preserve `PyNudge.source` (including its chat-id) for attribution and any
   ordinary ATM reply address; do not parse/rebuild it as a Hermes session.
3. Make duplicate ULID suppression remain bounded and bridge-local. It only
   prevents duplicated live pushes; it is not durable recovery state.

Required tests:

- sources `sender:telegram-chat@team` and `sender:future-chat@team` retain
  different structured addresses but both steer only the current profile's
  configured session;
- a duplicate ULID generates one steer; distinct ULIDs generate two;
- reconnect constructs one profile binding, not a session registry.

### 3. Reproducible active-profile reference proof

Development work:

1. Create `crates/atm-graft-python/tests/hermes_steer_fixture.py`,
   `scripts/phase-ai/run-hermes-steer-smoke.py`, and its unit test
   `scripts/phase-ai/test_run_hermes_steer_smoke.py`. The script must use the
   checked-in `HermesSteerFixture`: an ATM-controlled reference profile that
   implements the documented steer-port contract and has a controlled active
   tool turn. It is not a downstream production Hermes checkout.
2. Capture evidence that the steer is accepted for the configured session and
   becomes visible after the current safe tool boundary without interrupting
   or replacing that task.
3. Repeat for the AI.37 delayed recovery summary with durable unread and
   pending-ack fixtures. The agent uses its normal ATM skill to read the mail;
   the harness verifies the summary itself did not mutate mail state.

Required evidence fields:

```json
{
  "profile": "agent@team",
  "chat_id": "configured-host-session",
  "wake_kind": "live_nudge|recovery_summary",
  "steer_accepted": true,
  "normal_message_handler_called": false,
  "current_task_interrupted": false,
  "mailbox_mutated_by_wake": false
}
```

## Split Recommendation

Do not combine a later Discord/multi-channel capability into AI.38. It needs
an explicit identity/ownership revision after the single configured session is
reliable in production.

## Acceptance Criteria

1. Every live and recovery ATM wake-up uses the configured Hermes steer path.
2. No ATM wake-up invokes normal inbound-user-message dispatch.
3. The checked-in active-profile reference fixture receives the wake-up at a
   safe boundary without interruption, while durable ATM mail remains
   unchanged until normal ATM skills act. A separately retained downstream
   Hermes smoke run is useful operational evidence but is not a merge gate.
4. One configured profile/session works independently of other profiles.

## Required Validation

```text
python3 -m unittest crates/atm-graft-python/tests/test_hermes_adapter.py  # new in AI.38
python3 -m unittest crates/atm-graft-python/tests/test_hermes_bridge.py
python3 scripts/phase-ai/run-hermes-steer-smoke.py --fixture
just test-hermes-graft-bridge
just lint
just test
```

`run-hermes-steer-smoke.py --fixture` writes the Sub-Task 3 JSON evidence
schema for one live nudge and one recovery summary. A real downstream Hermes
run may be retained as operational evidence but cannot replace the checked-in
fixture proof or block this sprint on an external merge.

## Required Document Updates

- `docs/atm-graft/requirements.md`
- `docs/plans/phase-ai/hermes-graft-adapter-contract.md`
- `docs/plans/phase-ai/hermes-graft-runbook.md`
- ADR-039 and ADR-043 evidence/status notes

## Risks And Watchouts

- `/steer` semantics must be verified from the installed Hermes contract, not
  approximated by `MessageEvent(internal=False)`.
- A steer failure is observable but must never trigger a normal-message
  fallback or a graft retry queue.
- Keep the adapter's scope to one configured host session; future multi-chat
  routing is an explicit new feature.

## Completion Evidence

The reference adapter records the installed Hermes `session.steer` contract:
it resolves `ATM_CHAT_ID` to a runtime session id before sending nonblank
`text`, accepts only `result.status == "queued"`, and converts
missing-binding, rejection, and RPC failures to a visible
`HermesSteerFailure`. `run-hermes-steer-smoke.py --fixture` emits one
`live_nudge` and one `recovery_summary` row proving safe-boundary visibility,
no normal-message dispatch, no current-task interruption, and no wake-induced
mailbox mutation.
