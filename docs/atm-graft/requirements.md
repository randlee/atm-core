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
- host-facing queue / injection bridge for between-tool-call insertion
- graft-mode activation rules based on discovered `.atm.toml`
- graft-side structured observability through `sc-observability`

`atm-graft` does not own:

- daemon business logic
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
- `REQ-GRAFT-NOTIFY-001` `atm-graft` owns the host-facing nudge queue and
  structured payload contract used for between-tool-call injection. Satisfies:
  `REQ-P-GRAFT-001`.
- `REQ-GRAFT-OBS-001` `atm-graft` owns graft-side structured observability
  emission for activation, connectivity, registration, and nudge-queue
  behavior. Satisfies:
  `REQ-P-OBS-001`, `REQ-P-GRAFT-001`.

## 4. Required References

The `atm-graft` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
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
- the host-facing nudge payload is structured and must contain at least:
  - `from`
  - `message`
- the host executable owns the final insertion point between tool calls;
  `atm-graft` owns only the queue / bridge surface that makes those nudges
  available
- `atm-graft` must expose a small public surface rather than mirroring the full
  CLI:
  - daemon client operations for `send`, `read`, and `ack`
  - graft-session lifecycle entrypoints
  - host-facing nudge queue access
- `atm-graft` must emit structured observability for:
  - active / inactive graft mode
  - daemon connect / reconnect
  - registration success / failure
  - nudge received
  - nudge queued / dropped
