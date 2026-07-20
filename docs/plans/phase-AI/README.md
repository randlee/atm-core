---
title: Phase AI — HTTP daemon and minimal cross-host transport
status: proposed
integration_branch: integrate/phase-AI
---

# Phase AI

Phase AI replaces ATM's custom local frame protocol and abandoned cross-host
subsystem with one HTTP application contract:

```text
CLI / graft ─HTTP over UDS─┐
remote daemon ─HTTPS/TCP───┼─> one REST router -> handlers -> SQLite -> post-write event
test adapter ──────────────┘
```

`/v1/atm/messages`, `/v1/atm/message/{message-id}`, `/v1/atm/doctor`,
`/v1/atm/teams`, and `/v1/atm/team/{team-name}` are the durable resource roots.
An acknowledgement endpoint builds a write with `acknowledges_message_id`; no
separate acknowledgment pipeline exists. The post-write router is the sole
owner of local-versus-remote nudge routing.

The authoritative plan is [plan-phase-AI.md](./plan-phase-AI.md). Sprint
closure is recorded in [readiness.md](./readiness.md).

The checked-in OpenAPI 3.1 description is a durable interface artifact. A
browser UI is intentionally deferred to a later phase and consumes this API as
a client.
