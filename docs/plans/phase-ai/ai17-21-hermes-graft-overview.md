---
title: Phase AI.17–AI.21 — Hermes + atm-graft Integration
status: planned
branch: plan/phase-ah-hermes-graft-integration
execution_base: integrate/phase-AI at the documented dependency baseline
---

# Phase AI.17–AI.21 — Hermes + atm-graft Integration

AI.17–AI.21 integrate the established ATM application contract into Python
host agents, specifically the Hermes gateway. They do not create a second
message, identity, storage, CLI, or HTTP contract. Their concurrency with
AI.11–AI.16 is explicitly constrained below.

## Phase AI Contract Consumed by AI.17–AI.21

AI.17–AI.21 consume these Phase AI decisions unchanged:

- `AgentAddress { agent, chat_id: Option<ChatId>, team }` from ADR-037.
  Agent-facing rendering is `agent:chat-id@team` when a chat ID is present.
- Durable `source_chat_id` and `destination_chat_id` message columns. AH adds
  no `session_id` column and no daemon-generated conversation identifier.
- `atm send <to> --chat-id <id>` and `atm send <to> --as <agent>:<id>` are
  equivalent caller-context forms; the corresponding `atm read` forms are
  equivalent. AI.17–AI.21 do not add `--session` or `--session-id`.
- The Phase AI HTTP API and sealed `DaemonApiClient` are the single local
  client boundary. `atm-graft` and its Python binding use that boundary; they
  must not open a parallel daemon protocol or bypass its canonical write path.
- The canonical message write persists first, then the Phase AI post-write
  router performs nudge and any eligible transport work. Hermes receives a
  nudge only after the message is durable.

## Goals

1. Provide a supported PyO3/Maturin binding for `atm-graft` without changing
   its Rust public API.
2. Map the Hermes ambient `HERMES_SESSION_KEY` to a Phase AI `chat_id` when a
   Hermes process sends. A receiver sees the source as `agent:chat-id@team`
   and routes it to the corresponding isolated Hermes chat.
3. Deliver graft nudges to Hermes as in-process events, using the canonical
   message identity in the payload rather than a custom `X-Session-ID` header.
4. Deploy one supervised bridge per Hermes profile and prove the documented
   multi-turn user stories.

## Scope Boundary

AI.17–AI.21 may add Python bindings, a Hermes adapter, launchd configuration, Hermes
mapping/tests, and operational evidence. It must not modify Phase AI address
grammar, message schema, CLI semantics, HTTP resources, daemon routing, or
the Rust `atm-graft` API. Hermes ATM chats remain in an `atm:` namespace and
never share Telegram or Discord conversation state.

## Sprint Sequence

| Sprint | Purpose | Depends on |
|---|---|---|
| AI.17 | Verify and specify Hermes-to-Phase-AI chat identity mapping | AI.5 chat-address contract |
| AI.18 | PyO3/Maturin binding over the existing graft client boundary | AI.17 |
| AI.19 | Hermes gateway nudge adapter and canonical chat routing | AI.18 |
| AI.20 | Per-profile launchd deployment and runbook | AI.19 |
| AI.21 | Four-story end-to-end closure evidence | AI.20 |

## Parallelism

| Work | Parallel with AI.11–AI.16 | Constraint |
| --- | --- | --- |
| AI.17 | Yes | Uses only AI.5's accepted chat-address contract; no `atm-graft` change. |
| AI.18 | Yes, after AI.17 | New binding crate only; it consumes, but does not modify, `atm-graft`. |
| AI.19 | Yes only after AI.12 | Requires AI.12's accepted post-write nudge contract; may overlap AI.13–AI.16. |
| AI.20 | Draft only after AI.19 contract freeze | Runtime validation waits for AI.19 `PASS`. |
| AI.21 | No | Final evidence waits for AI.16, AI.19, and AI.20 `PASS`. |

AI.17, AI.18, and AI.19 are strictly sequential within this track. A parallel
branch must rebase if an upstream Phase AI contract it consumes changes.

See [plan-ai17-21-hermes-graft.md](plan-ai17-21-hermes-graft.md) for the
implementation contract and [readiness-ai17-21-hermes-graft.md](readiness-ai17-21-hermes-graft.md)
for closure gates.
