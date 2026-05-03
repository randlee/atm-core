# ATM-Graft Crate Architecture

## 1. Purpose

This document defines the `atm-graft` crate architectural boundary.

It complements the product architecture in
[`../architecture.md`](../architecture.md) and owns only embedded Rust
host-agent integration decisions.

This crate is introduced by the Phase Q implementation line. It is not part of
the pre-Phase-Q workspace.

## 2. Architectural Rules

- `atm-graft` is the embedded crate linked into a Rust host-agent executable.
- `atm-graft` depends on `atm-core` semantic types, request/result contracts,
  config semantics, and error vocabulary.
- `atm-graft` must not depend on `atm-daemon` as a Rust crate; it talks to the
  daemon over the documented same-host socket protocol only.
- `atm-graft` must not depend on `atm-rusqlite`; direct store access is outside
  its boundary.
- `atm-graft` owns graft-side structured observability and may depend directly
  on `sc-observability` for its own runtime events.
- `atm-graft` must remain runtime-neutral at its core. Host executables supply
  execution/spawn integration; optional adapters such as `tokio` may be
  provided as additive conveniences.
- `atm-graft` must not own host-specific tool-loop surgery. It exposes a host
  injection queue/bridge; the embedding executable decides when to drain and
  surface queued nudges.

## 2.1 Boundary Model

Phase Q uses this split for embedded host-agent integration:

- `atm-core` owns the semantic client protocol contract
- `atm-daemon` owns request handling, post-send-event generation, and nudge
  delivery
- `atm-graft` owns the concrete same-host daemon client, graft-session
  lifecycle, and host bridge

Architectural rule:
- first-party Rust host agents must not invent a parallel transport or
  alternate daemon contract outside the `atm-core` client models consumed by
  `atm-graft`

## 2.2 Activation And Config Boundary

`atm-graft` is active only inside an ATM-configured project.

Architectural rules:
- `.atm.toml` discovery gates whether graft mode is active at all
- if `.atm.toml` is absent, `atm-graft` remains inert
- runtime identity comes from `ATM_IDENTITY`; graft mode does not add a second
  identity-resolution scheme
- optional graft-specific config remains ATM-owned config semantics rather than
  host-private settings
- the initial graft config surface is intentionally small:
  - graft mode defaults on when active
  - an explicit disable switch is allowed
  - daemon endpoint overrides may be added later without widening the semantic
    contract

## 2.3 Graft Session

The active runtime object is `GraftSession`.

Responsibilities:
- connect to the same-host daemon API
- register the current host-agent identity and process context
- receive daemon-originated nudge events
- expose queued nudges to the embedding host executable
- shut down cleanly and unregister when appropriate

Architectural rules:
- `GraftSession` registration is automatic by default when graft mode is active
- disconnect / reconnect behavior belongs to `atm-graft`, not to the host
  executable's business logic
- session lifecycle failures remain typed and observable; they must not collapse
  into silent disabled behavior after activation succeeded

## 2.4 Nudge Delivery Model

Nudges originate from the daemon, not from local shell hooks.

Architectural rules:
- the daemon emits one internal `post-send-event` after authoritative message
  commit
- daemon-owned notifier logic may transform that event into one or more nudge
  payloads for registered graft sessions
- `post-send-event` is an internal runtime event and is distinct from the
  `.atm.toml` `post-send hook` subprocess mechanism
- the host-facing payload is structured and contains at least:
  - `from`
  - `message`
- the host-facing queue is FIFO from the perspective of the embedding agent
  loop; the queue exists to support cooperative insertion between tool calls
- nudges are advisory delivery signals, not durable mail truth; authoritative
  message state remains behind daemon-backed `read` calls

## 2.5 Client API Boundary

`atm-graft` should expose a deliberately small public surface.

Required public capability groups:
- graft-session lifecycle
- same-host daemon client calls for:
  - `send`
  - `read`
  - `ack`
- host-facing nudge queue access

Architectural rules:
- `read` uses the daemon API rather than direct SQLite access
- `send` and `ack` use the same daemon-backed semantic contract as `atm`
- any optional runtime heartbeat or activity reporting must also use the daemon
  API instead of side channels

## 2.6 Observability Boundary

`atm-graft` owns its own runtime/client observability.

Architectural rules:
- graft-side events emit through `sc-observability`
- graft-side events remain separate from daemon-owned runtime and transport
  events
- host-agent embedding must not require `atm-core` to depend directly on
  `sc-observability`

## 3. ADR Namespace

The `atm-graft` crate uses the `ADR-GRAFT-*` namespace.

Initial use cases:

- activation and inert-mode decisions
- runtime adapter design
- graft-session registration lifecycle
- host queue / injection bridge behavior
