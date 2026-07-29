---
title: Phase AI.17–AI.21 — Hermes + atm-graft Integration
status: planned
branch: plan/phase-ah-hermes-graft-integration
execution_base: integrate/phase-AI at the documented dependency baseline
---

# Phase AI.17–AI.21 Plan — Hermes + atm-graft Integration

## Goal

Integrate `atm-graft` into Hermes as a Python-callable, in-process nudge
receiver. Hermes sends and reads through the Phase AI application interface;
it receives a nudge only after the canonical Phase AI write has persisted the
message. This phase supplies host integration, not a new daemon protocol.

## Entry Gate and Execution Base

Each sprint starts from the exact `integrate/phase-AI` commit that satisfies
its stated dependencies and passes `just lint` and `just test`. The entry
record names that commit and release/version. AI.17–AI.21 merge forward
strictly within their own chain; merging an AI.11–AI.16 change forward is
required only when that change alters a consumed contract.

The following Phase AI contracts are hard dependencies:

- ADR-033 HTTP endpoint contract and sealed `DaemonApiClient` client boundary.
- ADR-035 canonical write ingress: one message write path, persist before
  post-write effects.
- ADR-037 chat address identity: separate optional `chat_id` fields with
  agent-facing `agent:chat-id@team` rendering.
- ADR-039 Python graft host binding: the sealed graft client is exposed to
  Python without a second daemon, storage, or routing boundary.
- `docs/requirements.md` caller-context contract for `--as` and `--chat-id`.

## Design

### Identity mapping

`ATM_CHAT_ID` is a client-neutral ambient non-empty, validated `chat_id`; it
is not a transport session, mailbox key, or new durable ATM field. Hermes is
the first client to consume it, but any session/chat-based agent may build the
same normal Phase AI caller address. Precedence is `--as`, then `--chat-id`,
then `ATM_CHAT_ID`, then qualified `ATM_IDENTITY`, then no chat-id:

```text
ATM_IDENTITY=omega-prime
ATM_CHAT_ID=1234
→ caller omega-prime:1234@<ATM_TEAM>
```

This is the same identity as either command below:

```bash
atm send <to> --chat-id 1234 <message>
atm send <to> --as omega-prime:1234 <message>
```

On receipt, Hermes uses the complete canonical source address, including its
single optional `chat_id`, to select its isolated local conversation. The
mapping is deterministic and never changes the persisted message address.

### Nudge adapter

The graft callback receives the canonical post-write nudge. The bridge passes
the message ID and binding-rendered canonical source address into Hermes. It
must not use a custom session-routing header, locally parse or re-render that
address, or issue another ATM write. This historical AI.17–AI.21 plan used an
incoming-chat handoff; ADR-043 and AI.36–AI.38 supersede that handoff with the
configured profile's non-interrupting steer path. The source remains
attribution/reply identity, not a Hermes session selector.

### Boundaries

- Phase AI owns parsing, validation, persistence, API routing, local versus
  remote routing, and post-write ordering.
- `atm-graft` owns connection to the sealed `DaemonApiClient` boundary and
  delivery of a nudge to its host callback.
- The Python binding only translates typed graft values to Python values.
- The Hermes adapter only maps the canonical source address to a Hermes chat
  and injects the body into Hermes.
- Launchd owns process supervision; it does not determine message routing.

No AI.17–AI.21 component may access SQLite directly, open a daemon socket directly,
or implement a second send/ack/read path.

## Scope

### Expanded scope acknowledgement

This track is not only PyO3/Maturin bindings. It also includes an in-repository
Hermes reference bridge and downstream handoff contract, parameterized
per-profile deployment templates and runbook, and retained production evidence.
It does not authorize edits to an external Hermes checkout. The owner must
record approval of this expanded scope in the readiness record before AI.19
starts.

### In scope

- Client-neutral ambient `ATM_CHAT_ID` resolution, first consumed by Hermes.
- PyO3/Maturin bindings for the full supported graft host API: client
  operations, session lifecycle/snapshot, and canonical nudge callback.
- Graft-to-Hermes nudge injection, per-profile launchd deployment, runbook,
  and end-to-end validation.

### Out of scope

- A `session_id` schema field, `--session`/`--session-id`, auto-generated
  conversation IDs, or custom session headers.
- Changes to Phase AI’s HTTP resources, CLI grammar, address parser, storage
  schema, canonical write path, or post-write router.
- Windows host support, cross-host transport changes, polling as primary
  delivery, and combining Hermes ATM chats with Telegram/Discord chats.

## Validation Model

AI.17 proves the identity mapping against Phase AI’s existing CLI/API behavior.
AI.18 proves typed Python binding parity, including the existing graft receiver
callback. AI.19 proves a durable write produces one Hermes in-process nudge and
maps distinct chat IDs to distinct Hermes chats. AI.20 proves supervised
per-profile operation. AI.21 records the four end-to-end stories with command
transcripts, message IDs, rendered addresses, chat IDs, and explicit verdicts.

Each sprint must pass `just lint`, `just test`, its focused tests, and
`git diff --check`; an AI.17–AI.21 sprint may not close by deferring an unmet
deliverable to the next sprint.

## Parallel Execution Rule

| Sprint | May run alongside AI.11–AI.16? | Preconditions and isolation |
| --- | --- | --- |
| AI.17 | Yes | AI.5 chat-address contract is accepted. It adds only a Hermes mapping adapter; it does not modify `atm-graft`, the daemon, HTTP, or storage. |
| AI.18 | Yes, after AI.17 | AI.17 is `PASS` and the sealed `DaemonApiClient` signature used by the binding is unchanged. It adds `atm-graft-python`; it does not modify `atm-graft`. |
| AI.19 | Yes, after AI.12 and AI.18 | AI.12 is `PASS` because the bridge consumes the canonical post-write nudge. It may run alongside AI.13–AI.16, but not before AI.12. |
| AI.20 | Draft only | AI.19 records a `FROZEN` readiness entry containing its exact SHA and stabilized bridge module, configuration keys, and readiness probe. AI.20 rebases on a replacement frozen SHA; deployment validation waits for AI.19 `PASS`. |
| AI.21 | No | It is final evidence and starts only after AI.16, AI.19, and AI.20 are `PASS`. |

AI.17→AI.18→AI.19 remains a strict merge-forward chain. A parallel branch
must touch only its named sprint targets; a changed Phase AI shared contract
invalidates its entry baseline and requires rebase and review before merge.
