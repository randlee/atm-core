# ATM-Graft Crate Requirements

## 1. Purpose

This document defines the `atm-graft` crate requirements.

The `atm-graft` crate owns the embedded Rust host-agent integration surface for
Phase Q. Product behavior remains defined in
[`../requirements.md`](../requirements.md). `atm-graft` must satisfy those
product requirements without re-owning `atm-core` service semantics or
`atm-daemon` runtime behavior.

## 2. Ownership

`atm-graft` owns:

- same-host daemon-client integration for linked Rust host-agent executables
- graft-session registration and lifecycle
- automatic daemon-originated nudge subscription when graft mode is active
- host-facing nudge fetch / injection bridge for between-tool-call insertion
- graft-mode activation rules based on discovered `.atm.toml`
- graft-side observability through an ATM-owned injected boundary supplied by
  the embedding host

`atm-graft` does not own:

- daemon business logic
- daemon-owned pending-nudge queue state
- direct SQLite access
- direct inbox JSONL parsing or writes
- direct ownership of ATM semantic types that already belong to `atm-core`
- forced interruption of a running tool call inside the host executable

## 3. Requirement Namespace

The `atm-graft` crate uses the `REQ-GRAFT-*` namespace.

Initial allocation:

- `REQ-GRAFT-CONFIG-*`
- `REQ-GRAFT-RUNTIME-*`
- `REQ-GRAFT-CLIENT-*`
- `REQ-GRAFT-NOTIFY-*`
- `REQ-GRAFT-OBS-*`

Initial crate requirement IDs:

- `REQ-GRAFT-CONFIG-001` `atm-graft` owns graft-mode activation and embedded
  config-loading behavior. Satisfies:
  `REQ-P-GRAFT-001`, `REQ-P-IDENTITY-001`.
- `REQ-GRAFT-RUNTIME-001` `atm-graft` owns the runtime-neutral graft-session
  lifecycle used by linked Rust host agents. Satisfies:
  `REQ-P-GRAFT-001`, `REQ-P-TEST-001`.
- `REQ-GRAFT-CLIENT-001` `atm-graft` owns the embedded same-host daemon client
  surface for first-party Rust host agents. Satisfies:
  `REQ-P-GRAFT-001`, `REQ-CORE-COMPAT-002`,
  `REQ-CORE-TRANSPORT-001`.
- `REQ-GRAFT-NOTIFY-001` `atm-graft` owns the host-facing nudge fetch/drain
  contract and structured payload rendering used for between-tool-call
  injection. Satisfies:
  `REQ-P-GRAFT-001`.
- `REQ-GRAFT-OBS-001` `atm-graft` owns graft-side structured observability
  emission for activation, connectivity, registration, and nudge-queue
  behavior. Satisfies:
  `REQ-P-OBS-001`, `REQ-P-GRAFT-001`.

## 4. Required References

The `atm-graft` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../plan-atm-graft.md`](../plan-atm-graft.md)
- [`../arch-review-phase-Q.md`](../arch-review-phase-Q.md)
- [`../project-plan.md`](../project-plan.md)
- [`../plan-phase-Q.md`](../plan-phase-Q.md)
- [`../documentation-guidelines.md`](../documentation-guidelines.md)
- [`../atm-error-codes.md`](../atm-error-codes.md)
- [`../atm-core/requirements.md`](../atm-core/requirements.md)
- [`../atm-core/architecture.md`](../atm-core/architecture.md)
- [`../atm-daemon/requirements.md`](../atm-daemon/requirements.md)
- [`../atm-daemon/architecture.md`](../atm-daemon/architecture.md)
- [`../team-member-state.md`](../team-member-state.md)

## 5. Phase Q Embedded-Graft Rules

Requirement IDs:
- `REQ-GRAFT-CONFIG-001`
- `REQ-GRAFT-RUNTIME-001`
- `REQ-GRAFT-CLIENT-001`
- `REQ-GRAFT-NOTIFY-001`
- `REQ-GRAFT-OBS-001`

Required rules:
- if no `.atm.toml` is discovered, `atm-graft` remains inactive and performs no
  daemon registration or nudge work
- if graft mode is active, runtime identity comes from `ATM_IDENTITY`; graft
  mode must not invent a separate identity source
- graft mode is enabled by default when active and may be disabled only by
  explicit config or runtime opt-out
- `atm-graft` must use the same-host daemon API for:
  - `send`
  - `read`
  - `ack`
  - graft-session registration / unregistration
  - daemon-originated nudge receipt
  - optional runtime heartbeat / activity reporting when the host enables it
- `atm-graft` must not bypass the daemon by talking directly to SQLite or inbox
  JSONL
- pending nudge state must remain daemon-owned so embedded and CLI/hook-based
  consumers observe one queue
- the host-facing nudge payload is structured and must contain at least:
  - `from`
  - `message`
- the host executable owns the final insertion point between tool calls;
  `atm-graft` owns only the fetch / bridge surface that makes those nudges
  available
- `atm-graft` must expose a small library surface rather than mirroring the
  full CLI:
  - daemon client operations for `send`, `read`, and `ack`
  - graft-session lifecycle entrypoints
  - host-facing nudge fetch/drain access
- any hook-facing command that renders insertion-ready nudge text belongs on
  the `atm` CLI surface and must call the same daemon API used by `atm-graft`
- `atm-graft` must emit structured observability for:
  - active / inactive graft mode
  - daemon connect / reconnect
  - registration success / failure
  - nudge received / fetched
  - daemon-reported nudge drop / backpressure signals when surfaced through the
    shared API
  - the observability boundary must be injected by the host binary; `atm-graft`
    must not require a direct public dependency on `sc-observability`

## 5.1 Q.5 Alignment Notes

The architectural target above remains correct, but the current Phase Q
implementation does not satisfy it yet.

Primary review target:
- `/Users/randlee/Documents/github/atm-core-worktrees/feature/pQ-s5-lock-retirement`

Current implementation notes that shape `atm-graft` planning:
- Q.5 currently exposes daemon-backed `read`, `clear`, `doctor`, and
  `heartbeat`, but `send` and `ack` still run through direct `RusqliteStore`
  call paths in the `atm` crate
- `atm_core::dispatcher` currently uses `serde_json::Value` payloads rather
  than typed semantic request / response / event structs
- same-host client control-state and wire-envelope types currently live in
  `atm-daemon`, not `atm-core`
- daemon framing is still newline-delimited and does not yet use the versioned
  binary header required for Q.6 completion
- Q.5 has no daemon API for graft-session registration, unregistration, or
  daemon-originated nudge delivery
- Q.5 has no daemon-owned pending-nudge drain API for hook-based consumers
- Q.5 has no `[atm.graft]` config surface in `atm-core`

Planning rule:
- `REQ-GRAFT-CONFIG-001`, `REQ-GRAFT-CLIENT-001`, and `REQ-GRAFT-NOTIFY-001`
  are prerequisite-driven requirements; `atm-graft` implementation starts only
  after the needed `atm-core` and `atm-daemon` surfaces exist

Scope-simplification rule for the first implementation pass:
- `atm-graft` v1 should keep its public API to `send`, `read`, `ack`,
  `GraftSession`, and host-facing nudge fetch/drain access
- runtime heartbeat / activity reporting is explicitly deferred unless the host
  integration proves it is needed in the same sprint
