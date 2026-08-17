---
title: Phase AI — HTTP daemon and minimal cross-host transport
status: proposed
integration_branch: integrate/phase-AI
---

# Phase AI

Phase AI replaces ATM's custom local frame protocol and abandoned cross-host
subsystem with one HTTP application contract:

```text
Unix CLI / graft ─HTTP over UDS or loopback TCP─┐
Windows CLI / graft ─HTTP over loopback TCP─────┤
remote daemon ─HTTPS/TCP───┼─> one REST router -> handlers -> SQLite -> post-write event
test adapter ──────────────┘
```

`/v1/atm/messages`, `/v1/atm/message/{message-id}`,
`/v1/atm/message/{message-id}/read`, and `/v1/atm/doctor` are the durable
initial resource roots.
An acknowledgement endpoint builds a write with `acknowledges_message_id`; no
separate acknowledgment pipeline exists. The post-write router is the sole
owner of local-versus-remote nudge routing.

An optional chat-id is part of a sender/recipient address, not a daemon session
or message-thread field: `hendrix:12345@hermes`. Storage preserves it in its
own nullable columns; every agent-facing `from`, `to`, nudge, reply, and ack
uses the same rendered address. `atm read --agent hendrix` spans chats and
`--agent hendrix --chat 12345` narrows to one context.

The authoritative plan is [plan-phase-ai.md](./plan-phase-ai.md). Sprint
closure is recorded in [readiness.md](./readiness.md).

AI.17–AI.21 extend this same contract to Hermes through a Python binding and
graft nudge adapter; they are specified in
[ai17-21-hermes-graft-overview.md](./ai17-21-hermes-graft-overview.md). They
add no message schema, CLI grammar, HTTP resource, or alternate write path.

AI.36–AI.38 plan the first closure of identified graft wake-up reliability
gaps without adding mail storage or a conversation manager:
[AI.36](sprint-ai-36-graft-receiver-ownership.md)
makes receiver ownership safe, [AI.37](sprint-ai-37-hermes-recovery-summary.md)
adds one durable-mail-derived restart summary, and
[AI.38](sprint-ai-38-hermes-steer-nudge-delivery.md) routes live/recovery
wake-ups through Hermes's non-interrupting steer path. A live steer failure
while the receiver remains connected is an explicitly retained, logged
residual; ADR-043 forbids masking it with an unbounded graft retry queue.

The checked-in OpenAPI 3.1 description is a durable interface artifact. A
browser UI is intentionally deferred to a later phase and consumes this API as
a client.

Cross-host smoke-gap closure is specified by
[AI.21-pre–AI.30](plan-phase-ai-crosshost-smoke-gaps.md): AI.21-pre establishes
the supported Python/XHTML smoke harness and explicit plaintext-test diagnostic
profile; AI.22 fixes the
host-qualified self-send guard; AI.23 proves one shared HTTP write endpoint;
AI.24 proves host-qualified ACK receipt/nudge through the advertised-IP TCP
path; AI.25 establishes stable hostname/pin peer authority; AI.26 establishes
one end-to-end peer-write deadline; AI.27 makes
persisted/confirmed/unconfirmed outcomes truthful; AI.28 adds bounded recovery
of recent canonical writes; AI.29 supplies receiver-proven physical smoke
evidence; and AI.30 separates compatible schema/API admission from
product-release labels. Each remaining sprint begins with matching
`1.3.2-beta-<sprint-number>` CLI/daemon release metadata.

## Physical peer smoke

Before peer smoke, use the
[daemon-switch skill](../../../.claude/skills/daemon-switch/SKILL.md) to select
one matching CLI/daemon candidate pair, restart its one managed daemon, and
verify native `atm doctor --json`. After smoke completes or aborts, restore the
latest installed pair with the same skill, restart once, and verify doctor
again. Do not leave a worktree daemon selected for other teams.
