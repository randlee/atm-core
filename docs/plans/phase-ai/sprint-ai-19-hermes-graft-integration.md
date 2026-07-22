---
id: AI.19
title: Hermes Gateway Graft Integration
status: planned
branch: feature/pAI-s19-hermes-graft-integration
worktree: ../atm-core-worktrees/feature/pAI-s19-hermes-graft-integration
target: integrate/phase-AI
---

# Sprint AI.19 — Hermes Gateway Graft Integration

## Goal

Connect the AI.18 graft callback to Hermes’s normal inbound-user-message path.
The callback receives a canonical post-write nudge; Hermes uses its structured
source address to select an isolated `atm:` chat and injects the message body.

## Hard Dependencies

- AI.17 and AI.18 are `PASS`.
- AI.12 is `PASS` and its Phase AI test proves persistence precedes every nudge.

## Parallel Execution

AI.19 may run in parallel with AI.13–AI.16 after AI.12 and AI.18 pass. It must
not modify the post-write router, daemon transport, or `atm-graft` API.

## Deliverables

- One Python bridge implementation using AI.18 that registers one graft
  receiver per Hermes profile. A Rust wrapper is out of scope.
- One typed adapter that maps `source.agent`, optional `source.chat_id`, and
  `source.team` to a Hermes chat key. It consumes structured values, never
  parses a rendered `agent:chat-id@team` string.
- Injection of the nudge body into Hermes’s existing inbound user-message
  path; no ATM write, retry, or alternate routing is performed by the bridge.
- Hermes tests proving: a write is durable before the event is visible; three
  nudges from one qualified source use one chat; two chat IDs remain isolated;
  ATM chats cannot collide with Telegram/Discord; malformed source addresses
  fail closed; duplicate notification delivery does not create a second
  Hermes turn for the same message ID.

## Exact Targets and Contract

- `crates/atm-graft-python/src/hermes.rs` — typed Python-facing Hermes bridge
  adapter over AI.18; no socket or storage dependency.
- `docs/plans/phase-ai/hermes-graft-adapter-contract.md` — checked-in contract
  for the external Hermes implementation.
- `~/.hermes/hermes-agent/gateway/platforms/atm_graft.py` — Hermes adapter
  target. AI.19’s entry record pins the Hermes checkout commit before editing.

```python
def deliver_atm_nudge(nudge: PyNudge) -> None:
    """Map nudge.source to an `atm:` chat and submit nudge.body once."""

def hermes_chat_key(source: PyAgentAddress) -> str:
    return f"atm:{source.agent}{':' + source.chat_id if source.chat_id else ''}@{source.team}"
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

- Focused bridge/Hermes tests and a running daemon proof pass.
- The proof records message ID, persisted-row observation, rendered source
  address, selected Hermes chat key, and nudge receipt order.
- `just lint`, `just test`, and `git diff --check` pass.
