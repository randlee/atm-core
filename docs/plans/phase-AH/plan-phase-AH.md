---
title: Phase AH Plan
status: planned
branch: develop
worktree: ../atm-core
---

# Phase AH Plan

## Goal

Integrate `atm-graft` into Hermes Agent as a Python-callable nudge-delivery
bridge so that launchd-backed Hermes gateway daemons can receive ATM
notifications as in-process events into persistent named sessions — closing
the gap between the tmux-send-keys nudge surface (Claude Code / Codex) and
the Hermes daemon (non-tmux, non-stdin) model.

ATM 1.3.1 defines the graft nudge surface for Rust-embedded host agents.
Hermes hosts the agent loop in Python; it cannot link `atm-graft` directly
without a Python extension binding. Phase AH adds those bindings, wires the
graft session into the Hermes gateway event loop, and adds a `session_id`
field to the ATM durable message model so multi-turn conversations can land
in the same Hermes session on both sides — without callers ever managing a
`--session` flag manually.

Phase AH therefore becomes:

- session-id protocol and query surface on the ATM durable message model
  (`AH.1`)
- Python extension bindings for `atm-graft` via PyO3 + Maturin (`AH.2`)
- Hermes gateway graft integration — graft session activation, nudge
  injection into Hermes session context, `X-Session-ID` routing (`AH.3`)
- Launchd deployment for the per-profile bridge processes (`AH.4`)
- end-to-end validation — closure of the four motivating user stories
  (`AH.5`)

## Historical Input and Namespace Rule

This phase is a new namespace — it does not supersede or modify any existing
phase.

Input dependency on:

- Phase AF's accepted same-host reliability line (merged to `develop` at
  `98a4e66c`) — the atm-graft Rust crate API is the stable integration
  surface Phase AH must not change
- Phase AG's cross-host product surface — orthogonal, AH does not depend on
  AG closure
- `docs/atm-graft/architecture.md` — embedded crate boundary rules
- `docs/atm-graft/boundaries.md` — owner-of-what between atm-graft and host
- `crates/atm-graft/src/lib.rs` — `HostNudgeInjector` trait and
  `GraftSessionOptions` entry points Phase AH must bind verbatim

## Release Framing

Phase AH is post-1.3.1 extension work. The atm-graft crate is already shipped
in 1.3.1 as a Rust-embed-only library. Phase AH adds Python consumability
without breaking the existing Rust consumer contract.

Entry-gate prerequisites:

- atm-core 1.3.1+ is the working baseline with stable atm-graft API
- `HostNudgeInjector`, `GraftSession`, `GraftSessionOptions`,
  `GraftObservability` are the stable integration points; no atm-graft Rust
  API change is permitted during AH
- Hermes Agent 0.17.0+ (or a Hermes version with the webhook platform
  adapter) runs each profile
- The `hermes` team is already registered in ATM with members `hendrix`,
  `grecon`, `alpha-prime`, `skillrx`
- The Hermes session key env is already exposed per process
  (`HERMES_SESSION_KEY`)

Release claim this phase must validate:

- Hermes gateways receive ATM nudges within 1 second of `atm send`
- multi-turn ATM conversations can execute against a single persistent
  Hermes session, with no cross-session pollution
- no regression in existing Hermes gateway behavior (Telegram, Discord,
  webhook)
- no regression in existing Rust consumers of atm-graft (atm-daemon, atm
  CLI)
- `atm read` default query mode is session-scoped; `--agent` searches
  across sessions

Release claim this phase must not make without evidence:

- that atm-graft Python bindings work on Windows (Linux and macOS are the
  only targets for AH)
- that Hermes sessions can safely share context with interactive Telegram
  sessions (ATM sessions are isolated per counterparty)
- that cross-host atm-graft delivery works (same-host only in AH scope)

## Branch Framing

Phase AH uses a single planning branch on
`plan/phase-ah-hermes-graft-integration` for the planning phase itself.
Each implementation sprint uses a feature branch off develop. The plan branch
is the source of truth until execution starts, at which point the sprint docs
become authoritative in their own branches.

## Scope

Phase AH may:

- add `session_id` as a first-class field on the ATM durable message model
- add session-scoped and agent-scoped `atm read` query modes
- add PyO3/Maturin build configuration, producing a new Python extension
  crate (separate from the existing atm-graft cdylib; never modifies the
  existing cdylib target or the Rust API)
- add a Hermes gateway graft-adapter that activates one graft session per
  profile on startup and routes nudges into named Hermes sessions
- extend the Hermes webhook adapter to honor `X-Session-ID` for
  persistent-session routing
- add launchd plists for the per-profile bridge processes
- add an ADR for the Hermes + atm-graft boundary
- add requirements and architecture updates in `docs/atm-graft/` reflecting
  the Python host binding surface
- add operational runbook and validation packages

Phase AH must not:

- change the `atm-graft` Rust API surface (`HostNudgeInjector`,
  `GraftSession`, `GraftSessionOptions`, `GraftObservability`)
- merge the Hermes ATM session namespace with the Hermes Telegram or Discord
  session namespace
- introduce polling as the primary nudge delivery mechanism (polling is the
  known fallback only; push is the requirement)
- treat Windows as a supported host target in this phase
- add or modify CLI `atm send` / `atm read` semantics beyond the session_id
  field addition (the new query modes are additive, not a rewrite of
  existing CLI surface)
- conflate Hermes gateway lifecycle with atm-daemon lifecycle (they remain
  separate processes)
- require agents to pass `--session` explicitly on `atm send`; the binding
  auto-populates from `HERMES_SESSION_KEY`

## Session-ID Protocol Design

The session_id is a first-class field on the ATM durable message model:

- senders MAY set session_id; when they do not, the daemon auto-assigns one
- the session_id is durable — stored in the mailbox row, carried through
  delivery, surfaced on `atm read` output
- senders NEVER need to pass `--session` explicitly; idiomatic `atm send`
  picks up the session_id from the active transport-session context
- receivers route by `session_id` to find an existing Hermes session; a
  missing session is created
- both sides maintain parallel session state; the session_id is the shared
  coordinate

Display form when a message arrives on the receiver side:

```
hendrix:telegram:8991600178@hermes
```

Where the parts are:

| Segment | Source |
|---|---|
| `hendrix` | `ATM_IDENTITY` of the sender |
| `telegram:8991600178` | sender's `HERMES_SESSION_KEY`, stripped of its
  `agent:main:` prefix and namespace-quoted |
| `@hermes` | the `ATM_TEAM` of the sender |

This becomes the receiver's Hermes chat_id:
`atm:{sender_agent}:{sender_transport_session_id}`

The receiver's Hermes creates this on first message, finds it by session_id
on subsequent messages, and routes replies back using the receiver's own
`HERMES_SESSION_KEY` (auto-attached by the Python binding on outbound send).

## Query Surface

`atm read` defaults to session-scoped:

```bash
# When invoked from a Hermes session: returns only messages in the current
# session_id scope
atm read
```

Agent-scoped search:

```bash
# Returns messages with a specific peer across all sessions
atm read --agent arch-ctm
```

Session-id-scoped query:

```bash
atm read --session-id <id>
```

These are additive to existing query modes. Existing `atm read --team NAME`
and `atm read --as IDENTITY` modes remain unchanged and compose with
`--agent` / `--session-id`.

## Ownership Model

Phase AH separates execution from verification:

- `team-lead` (Hendrix)
  - owns dispatch, sequencing, branch routing, and merge authorization
- `arch-ctm` (or Hermes-side architect)
  - owns plan edits, implementation-side code + doc updates, sprint review
- `quality-mgr`
  - owns independent review and PASS/FAIL verdicts on each sprint
- `hermes-operator`
  - owns Hermes profile configuration, launchd plist management, and
    gateway restart coordination

Findings-ledger `owner` field uses one of:

- `team-lead`
- `arch-ctm`
- `quality-mgr`
- `hermes-operator`

## Validation Lanes

### Lane A — Session-ID Protocol Closure

Purpose:

- prove the durable `session_id` field on the ATM message model is
  representable, queryable, and transport-preserving across send/receive

Required shape:

- schema change is merged
- `atm read` default mode is session-scoped
- `atm read --agent <name>` returns peer-scoped messages across sessions
- unit tests cover: send with session_id set, send without, query by
  session_id, query by agent

### Lane B — Python Bindings + Hermes Integration (macOS)

Purpose:

- prove the atm-graft Python extension loads, activates a graft session,
  and routes nudges into a Hermes gateway in-process

Required shape:

- one Hermes profile (default) started with graft-adapter enabled
- atm-daemon running at 1.3.1+
- Hermes webhook platform enabled on loopback only (`host: 127.0.0.1`)
- webhook adapter honors `X-Session-ID`
- no reads or writes against live ATM state beyond the `hermes` team mailbox

Every validation row must capture:

- sender identity and target profile
- exact ATM command transcript
- Hermes session state before and after
- graft-bridge log snapshot when nudge delivery was exercised
- finding ID if the row fails
- whether the failure was BINDING, SESSION, DELIVERY, or CONFIG-GAP

### Lane C — Cross-Profile Validation

Purpose:

- prove each of the four Hermes profiles receives nudges independently
- without session crosstalk

Entry condition:

- Lane B is green end to end

Required shape:

- each profile runs its own graft-adapter against the same atm-daemon
- each profile has a distinct Hermes ATM session namespace
- nudge delivery to one profile does not affect another profile's session

### Lane D — Four-Story Closure Validation

Purpose:

- prove each of the four motivating user stories works end to end

Entry condition:

- Lanes B and C are both green

Required shape:

- each story is executed as a complete narrative
- evidence is retained for each story's success criteria
- multi-turn ATM conversations are captured with full context persistence
- blocking requests complete within documented latency bounds (minutes for
  story 3, seconds for 1/4)

## Failure Classification

The sole authoritative `classification` enum for Phase AH findings:

- `BINDING` — PyO3/Maturin binding failed, module won't load, or crashes the
  host
- `SESSION` — Hermes session routing failed, context was lost, or nudges were
  delivered to the wrong session
- `DELIVERY` — atm-daemon → bridge → Hermes nudge delivery failed at a
  known layer
- `CONFIG-GAP` — launchd / webhook / atm-graft session config gap
- `PRODUCT-BUG` — atm-graft or Hermes webhook adapter defect
- `PROTOCOL-BUG` — session_id field contract was violated or under-specified
- `EXTERNAL-BLOCKER` — Hermes upstream dependency not yet supporting AH
  surface

## Evidence Contract

Every validation row must capture:

- Hermes profile and identity involved
- exact atm-daemon and Hermes gateway versions (commit SHA or release tag)
- exact graft-bridge process startup command and resulting PID
- exact `atm send` command transcript (including any `--requires-ack`)
- Hermes session state (session ID, message count before/after)
- graft-bridge log snapshot showing nudge receipt and delivery
- finding ID if the row fails
- failure classification from the enum above

## Sprint Sequence

### AH.1 atm-core Session-ID Protocol + Query Surface

Primary objective:

- add `session_id` as a first-class field on the ATM durable message model
- expose session-scoped `atm read` default mode
- expose peer-scoped `atm read --agent <name>` query mode

Outputs:

- schema change (nullable `session_id` column on the mailbox table; backfill
  for existing rows assigns a fresh idempotency-style id)
- `Message::session_id` field surfaced on the Rust delivery surface
- CLI `--session-id` on send (optional)
- CLI default `atm read` picks up ambient session via `HERMES_SESSION_KEY` when set
- CLI `atm read --agent <name>` query mode
- unit + integration tests covering session-scoped and agent-scoped reads;
  round-trip send/receive preserves session_id

Entry gate: none; this is the foundational sprint

Execution owner: `arch-ctm`
Verification owner: `quality-mgr`

### AH.2 atm-graft Python Bindings

Primary objective:

- produce a loadable Python extension module from `atm-graft` via Maturin
  that exposes `GraftSessionOptions`, `GraftSession` activation, and the
  nudge-receiver callback seam to Python

Outputs:

- new crate `crates/atm-graft-python/` wrapping `atm-graft`
- `pyproject.toml` for Maturin build
- Python `AtmGraftSession` class that wraps the Rust session
- `set_nudge_callback()` accepting a Python callable as the `HostNudgeInjector`
- unit tests covering:
  - module import
  - session lifecycle (activate/deactivate)
  - nudge callback invocation with synthetic nudge
  - error propagation across FFI boundary
- documentation of the Python API surface

Entry gate: `AH.1` is `PASS`

Execution owner: `arch-ctm`
Verification owner: `quality-mgr`

### AH.3 Hermes Gateway Graft Integration + X-Session-ID

Primary objective:

- activate a graft session in each Hermes gateway process on startup so the
  Hermes event loop can receive ATM nudges as injected messages inside a
  named persistent session
- add `X-Session-ID` header support to the Hermes webhook adapter so nudges
  always route into a named session (chat_id = `atm:{from_agent}:{session_id}`)

Outputs:

- Hermes gateway graft-adapter module that activates the graft session per
  profile
- `HostNudgeInjector` Python implementation that routes nudge into the
  Hermes session queue for the named ATM session
- `GraftObservability` Python implementation emitting Hermes-compatible
  tracing
- Webhook adapter change honoring `X-Session-ID`
- Idempotency cache scoped per session (not per delivery)
- Session routing key derivation:
  `chat_id = f"atm:{from_agent}:{session_id}"` (from
  `hendrix:telegram:8991600178@hermes` display form)

Entry gate: `AH.2` is `PASS`

Execution owner: `arch-ctm` (with `hermes-operator` for Hermes-side config)
Verification owner: `quality-mgr`

### AH.4 Hermes Launchd Bridge Processes

Primary objective:

- add per-profile launchd plists so each Hermes profile's graft-adapter
  starts alongside its gateway and is supervised by launchd

Outputs:

- launchd plist per profile (`ai.hermes.bridge-{profile}`)
- Hermes gateway config to auto-enable the webhook platform on loopback at
  startup when graft-adapter is configured
- atm-graft session config per profile (`team`, `agent`, workspace path)
- operational runbook for starting/stopping/inspecting bridge processes
- unit/acceptance tests:
  - bridge starts with launchd
  - bridge registers with atm-daemon after gateway is up
  - bridge shuts down cleanly when gateway stops
  - atm-daemon routes nudges to the correct bridge

Entry gate: Lanes B and C of `AH.3` are `PASS`

Execution owner: `hermes-operator`
Verification owner: `quality-mgr`

### AH.5 Four-Story Closure Validation

Primary objective:

- prove each of the four motivating user stories works end to end

Stories:

1. `team-lead@atm-dev` multi-turn question via ATM → answer lands in
   Telegram
2. Nightly ATM cron → publish daily report
3. PR approval via ATM nudge → Hermes reviews and approves in minutes
4. ATM agent asks Hendrix for design info → Hendrix answers via ATM

Outputs:

- four story-acceptance evidence packages, one per story
- latency measurements from story 3 (must be within minutes)
- multi-turn context retention evidence from story 1
- final phase `readiness.md` with story closure verdicts

Entry gate: Lanes B and C of `AH.3` and `AH.4` are both `PASS`

Execution owner: `team-lead` with `hermes-operator`
Verification owner: `quality-mgr`

## Required Interface Matrix

Phase AH must validate all of the following:

- atm-graft Python module imports cleanly on macOS (Python 3.11+, 3.12+)
- graft session activation does not block Hermes gateway startup
- atm-daemon delivers a nudge to the bridge process via Unix socket
- bridge translates nudge to HTTP POST with `X-Session-ID` header to
  Hermes webhook port
- Hermes webhook adapter accepts POST and routes to named session using
  `atm:{from_agent}:{session_id}` chat_id key
- session state persists across 3+ consecutive nudges in same session
- Hermes Telegram session is not affected by ATM nudge delivery to the ATM
  session
- launchd bridge process tracks Hermes gateway lifecycle
- `atm read` default mode is session-scoped
- `atm read --agent <name>` returns peer-scoped messages across sessions
- story 1: multi-turn ATM → Hermes → Telegram round-trip with context
  intact
- story 2: nightly cron completes without manual intervention
- story 3: PR-approval nudge reaches Hermes in <60 seconds, Hermes acts
  on it, ack returns to sender within a minute
- story 4: design-question nudge reaches Hermes and reply lands in ATM
  inbox with correct session_id so the peer can continue

## Required Document Updates

This phase must update these docs as work progresses:

- `docs/atm-graft/requirements.md` — add Python host binding surface
- `docs/atm-graft/architecture.md` — add Python extension crate boundary
- `docs/plans/phase-AH/readiness.md` — sprint closure table
- `docs/plans/phase-AH/hermes-integration-runbook.md` — operational runbook
  (AH.4)
- `docs/plans/phase-AH/four-story-validation.md` — closure evidence index
  (AH.5)
- ADR for the Hermes + atm-graft boundary

## Four-Story Validation Package

Phase AH's closure criterion is that all four motivating user stories work
end to end. The `four-story-validation.md` artifact (produced in AH.5)
catalogs:

- the four stories
- the session_id flow for each (who picks it, where it lives, how it
  round-trips)
- the evidence collected
- the per-story verdict (`PASS`, `FAIL`, `PARTIAL`)
- the latency observation from story 3 (must be within minutes)
- the context-persistence observation from story 1 (must span 3+ turns)
