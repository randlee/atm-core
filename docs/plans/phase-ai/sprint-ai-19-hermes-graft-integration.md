---
id: AI.19
title: Hermes Gateway Graft Integration
status: complete
branch: feature/pAI-s19-hermes-graft-integration
worktree: ../atm-core-worktrees/feature/pAI-s19-hermes-graft-integration
target: integrate/phase-AI
---

# Sprint AI.19 — Hermes Gateway Graft Integration

## Goal

Historical AI.19 scope connected the AI.18 graft callback to Hermes’s normal
inbound-user-message path. ADR-043 and AI.36–AI.38 supersede that wake-up
handoff with the configured profile's non-interrupting steer path; this frozen
sprint remains an implementation-history record, not the current requirement.
The callback receives a canonical post-write nudge; Hermes uses its structured
source address to select an isolated `atm:` chat and injects the message body.

## Hard Dependencies

- AI.17 and AI.18 are `PASS`.
- AI.12 is `PASS` and its Phase AI test proves persistence precedes every nudge.

## Parallel Execution

AI.19 may run in parallel with AI.13–AI.16 after AI.12 and AI.18 pass. It must
not modify the post-write router, daemon transport, or `atm-graft` API.

## Deliverables

- `crates/atm-graft-python/python/atm_graft_hermes_bridge.py` — one Python
  bridge implementation using AI.18 that registers one graft receiver per
  Hermes profile. New Rust wrapper code is out of scope except the narrow
  `PyNudge(message_id, source, body)` value constructor required to exercise
  the existing typed callback payload in reference-adapter tests.
- The bridge uses `PyAgentAddress.__str__()` as the canonical conversation
  identity and adds only the Hermes-local `atm:` namespace. It performs no
  segment validation, address rendering, or local identity helper logic.
- Injection of the nudge body into Hermes’s existing inbound user-message
  path; no ATM write, retry, or alternate routing is performed by the bridge.
- `crates/atm-graft-python/tests/test_hermes_bridge.py` — reference-adapter
  tests proving: a write is durable before the event is visible; three nudges
  from one qualified source use one chat; two chat IDs remain isolated; ATM
  chats cannot collide with Telegram/Discord; malformed source addresses fail
  closed; duplicate notification delivery does not create a second Hermes turn
  for the same message ID.
- `docs/plans/phase-ai/hermes-graft-adapter-contract.md` — checked-in Hermes
  handoff contract specifying the typed callback, the `atm:` namespace, and
  the downstream repository/test/review handoff; it authorizes no external
  checkout edit in this sprint.

## Exact Targets and Contract

- `crates/atm-graft-python/python/atm_graft_hermes_bridge.py` — typed Python
  reference adapter over AI.18; no socket or storage dependency.
- `crates/atm-graft-python/tests/test_hermes_bridge.py` — tests for every
  reference-adapter assertion listed in Deliverables.
- `.just/run_hermes_graft_bridge_tests.py` and the
  `just test-hermes-graft-bridge` recipe — build the Maturin package in an
  isolated venv, then execute `python -m unittest discover -s
  crates/atm-graft-python/tests -p test_hermes_bridge.py`. This uses the
  Python standard library; no unwired pytest dependency is introduced.
- `docs/plans/phase-ai/hermes-graft-adapter-contract.md` — checked-in contract
  for Hermes maintainers, including the downstream test/merge handoff.

AI.19 does not modify an external Hermes checkout. Hermes maintainers consume
the checked-in reference adapter and contract through their own repository,
branch, test, review, and merge process; that downstream work is not an
atm-core deliverable or closure gate.

```python
def deliver_atm_nudge(nudge: PyNudge) -> None:
    chat_key = f"atm:{nudge.source}"
    # Submit nudge.body to Hermes's normal inbound-user-message path once.
```

The bridge keeps a bounded in-memory set of recently injected **message IDs**
solely to suppress duplicate callback delivery. It does not retry delivery,
persist delivery state, or create any ATM message. Hermes’s normal inbound
message API is the sole injection target.

## Boundary and Non-Goals

AI.19 does not add `X-Session-ID`, custom session headers, a webhook-specific
address grammar, a separate idempotency key, polling, or a second send/ack
path. The normal Hermes webhook behavior for unrelated routes remains
unchanged.

## Closure

- `just test-hermes-graft-bridge` passes, proving every reference-adapter
  assertion named in Deliverables; `just test` alone is insufficient for this
  target.
- The checked-in `hermes-graft-adapter-contract.md` contains the typed
  callback, namespace, and downstream handoff contract described in
  Deliverables.
- A running daemon proof passes.
- The proof records message ID, persisted-row observation, rendered source
  address, selected source `chat_id`, and nudge receipt order.
- `just lint`, `just test`, and `git diff --check` pass.
- Before AI.20 drafting starts, AI.19 records `FROZEN` in
  `readiness-ai17-21-hermes-graft.md`, with the exact commit SHA and the
  bridge module, configuration keys, and readiness probe names. If any of
  those artifacts changes before AI.19 `PASS`, AI.19 replaces the frozen
  record and AI.20 rebases its draft before it resumes.
