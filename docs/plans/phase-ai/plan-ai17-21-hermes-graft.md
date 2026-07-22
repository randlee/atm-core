---
title: Phase AI.17–AI.21 — Hermes + atm-graft Integration
status: planned
branch: plan/phase-ah-hermes-graft-integration
execution_base: integrate/phase-AI after AI.16 closure
---

# Phase AI.17–AI.21 Plan — Hermes + atm-graft Integration

## Goal

Integrate `atm-graft` into Hermes as a Python-callable, in-process nudge
receiver. Hermes sends and reads through the Phase AI application interface;
it receives a nudge only after the canonical Phase AI write has persisted the
message. This phase supplies host integration, not a new daemon protocol.

## Entry Gate and Execution Base

AI.17 may start only when AI.16 is accepted on `integrate/phase-AI` and the
resulting integration baseline passes `just lint` and `just test`. Every
AI.17–AI.21 implementation branch is created from that exact integration
commit and merges forward strictly into its successor. The entry record must
name the commit and release/version used for the run.

The following Phase AI contracts are hard dependencies:

- ADR-033 HTTP endpoint contract and sealed `DaemonApiClient` client boundary.
- ADR-035 canonical write ingress: one message write path, persist before
  post-write effects.
- ADR-037 chat address identity: separate optional `chat_id` fields with
  agent-facing `agent:chat-id@team` rendering.
- `docs/requirements.md` caller-context contract for `--as` and `--chat-id`.

## Design

### Identity mapping

`HERMES_SESSION_KEY` is a Hermes-local source for a non-empty, validated
`chat_id`; it is not a transport session, mailbox key, or new durable ATM
field. The Python binding builds the normal Phase AI caller address:

```text
ATM_IDENTITY=omega-prime
HERMES_SESSION_KEY=1234
→ caller omega-prime:1234@<ATM_TEAM>
```

This is the same identity as either command below:

```bash
atm send <to> --chat-id 1234 <message>
atm send <to> --as omega-prime:1234 <message>
```

On receipt, Hermes derives its isolated local chat key from the complete
canonical source address, for example `atm:omega-prime:1234@atm-dev`. The
mapping is deterministic and never changes the persisted message address.

### Nudge adapter

The graft callback receives the canonical post-write nudge. The bridge passes
the message ID and structured source/destination address into Hermes. It must
not use a custom session-routing header, reconstruct an address from display
text, or issue another ATM write. Hermes resolves the incoming chat from the
canonical `source.agent` plus optional `source.chat_id`, then submits the body
to its ordinary inbound-user-message path.

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

### In scope

- Hermes mapping of an ambient session key to Phase AI `chat_id`.
- PyO3/Maturin bindings for the existing graft API.
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
AI.18 proves typed Python binding parity. AI.19 proves a durable write produces
one Hermes in-process nudge and maps distinct chat IDs to distinct Hermes
chats. AI.20 proves supervised per-profile operation. AI.21 records the four
end-to-end stories with command transcripts, message IDs, rendered addresses,
Hermes chat keys, and explicit verdicts.

Each sprint must pass `just lint`, `just test`, its focused tests, and
`git diff --check`; an AI.17–AI.21 sprint may not close by deferring an unmet
deliverable to the next sprint.

## Parallel Execution Rule

AI.17→AI.18→AI.19 is a strict merge-forward chain. AI.20 documentation and
plist-template drafting may begin after AI.19's bridge invocation/configuration
contract is committed, on its own branch based on that commit; its deployment
and closure work waits for AI.19 `PASS`. AI.21 is never parallel with a live
AI.19 or AI.20 implementation because it is phase evidence, not a fix sprint.
