---
title: Phase AH Plan
status: planned
branch: develop
worktree: ../atm-core
---

# Phase AH

Phase `AH` owns the integration of `atm-graft` (the Rust embeddable ATM client
crate) into Python host agents — specifically the Hermes Agent gateway daemon —
so that Hermes can receive ATM nudges as in-process events via persistent named
sessions, with no polling, no tmux-send, no Python-side polling loop, and no
new transport path.

Phase AH extends the Phase AF/AG release line (1.3.1+) which already ships
`atm-graft` as a first-class Rust-embeddable crate. Phase AH adds:

- a `session_id` field to ATM's durable message model, so receivers can maintain
  conversation context across multiple nudges
- a Python (PyO3 + Maturin) binding for `atm-graft` so non-Rust host agents can
  embed the crate
- a Hermes webhook-adapter extension honoring `X-Session-ID` for persistent
  named session routing
- per-profile launchd plists for the Hermes graft-bridge process so each
  gateway daemon starts alongside its session-scoped graft receiver

## Historical Input and Namespace Rule

This phase is a new namespace. It does not supersede or modify any existing
phase.

Input dependency on:

- `Phase AF` — accepted same-host reliability line (merged to `develop` at
  `98a4e66c`); the `atm-graft` Rust crate API is the stable integration surface
  Phase AH must not change
- `Phase AG` — cross-host product surface (orthogonal; AH does not depend on
  AG closure)
- `docs/atm-graft/architecture.md` — embedded crate boundary rules
- `docs/atm-graft/boundaries.md` — owner-of-what between atm-graft and host
- `crates/atm-graft/src/lib.rs` — `HostNudgeInjector` trait and
  `GraftSessionOptions` entry points Phase AH must bind verbatim

Phase AH does NOT change the atm-graft Rust API. It extends the protocol
message model (adds `session_id`) and adds a new Python-embeddable binding
crate.

## Phase Goals

1. ATM's message model carries a durable `session_id` field visible to agents
   via query but invisible to them via the idiomatic `atm send` / `atm read`
   surface — agents never pass `--session` manually
2. A Python extension (PyO3 + Maturin) wraps atm-graft so Python host agents
   (Hermes) can activate a graft receiver and receive nudges in-process
3. The Hermes gateway activates a graft session per profile on startup and
   routes received nudges into persistent named sessions (one namespace per
   sender, keyed on `atm:{from}:{session_id}`)
4. Hermes's "from-agent-@-transport-session" display form is the identity
   source for the session_id on outbound ATM sends — the sender never has to
   remember a session reference
5. `atm read` defaults to session-scoped output (current session only) but
   accepts `--agent <name>` to search across all sessions with that peer
6. Phase-level end-to-end validation proves the four motivating user stories
   work with sub-60-second latency and persistent multi-turn context

## Sprint Sequence

| Sprint | Title | Owner | Status |
|--------|-------|-------|--------|
| AH.1 | atm-core Session-ID Protocol + Query Surface | `arch-ctm` | `PENDING` |
| AH.2 | atm-graft PyO3 Python Bindings | `arch-ctm` | `PENDING` |
| AH.3 | Hermes Gateway Graft Integration + X-Session-ID | `arch-ctm` (Hermes side) | `PENDING` |
| AH.4 | Hermes Launchd Bridge Processes | `hermes-operator` | `PENDING` |
| AH.5 | Four-Story Closure Validation | `team-lead` + all operators | `PENDING` |

## Planning Artifacts

- `plan-phase-AH.md` — this file, the phase-level source of truth
- `readiness.md` — sprint closure table and gate criteria
- `sprint-AH1.md`
- `sprint-AH2.md`
- `sprint-AH3.md`
- `sprint-AH4.md`
- `sprint-AH5.md`
- `four-story-validation.md` — closure evidence index for the four user stories
- `hermes-integration-runbook.md` — operational runbook for Phase AH.4

## Scope

Phase AH may:

- add `session_id` as a first-class field on the ATM durable message model
- expose session-scoped and agent-scoped `atm read` query modes
- add a Python extension crate (`atm-graft-python` or equivalent)
- add Hermes gateway plumbing that activates a graft session per profile
- add Hermes side session routing based on `atm:{from_agent}:{session_id}`
- add launchd plists for per-profile bridge processes
- add planning, sprint, validation, and runbook docs
- add an ADR for the Hermes + atm-graft boundary

Phase AH must not:

- change the atm-graft Rust API (`HostNudgeInjector`, `GraftSession`,
  `GraftSessionOptions`, `GraftObservability`)
- change the atm-daemon's Unix-socket nudge protocol (only extends the durable
  message schema)
- merge Hermes ATM sessions with Hermes Telegram sessions
- support Windows hosts (Linux and macOS only in this phase)
- support cross-host ATM nudges via Python bindings (same-host only;
  cross-host is a Phase AG concern not opened by AH)
- conflate Hermes gateway lifecycle with atm-daemon lifecycle
