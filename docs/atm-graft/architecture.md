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
  injection fetch/bridge; the embedding executable decides when to drain and
  surface daemon-owned nudges.

## 2.1 Implementation Target Snapshot

The current implementation target for this design is:
- `develop` after `Q.RULES-DOC-1` merge
- Phase Q implementation review target:
  `/Users/randlee/Documents/github/atm-core-worktrees/feature/pQ-s5-lock-retirement`

Current Q.5 realities that this architecture must target:
- `atm-core` already has strong durable store models for mail, task, and roster
  state
- `atm-daemon` already supports same-host `read`, `clear`, `doctor`, and
  `heartbeat` requests
- the daemon protocol is still unary request/response and does not yet support
  graft registration or server-push nudge delivery
- the current CLI still uses mixed transport modes:
  - `read`, `clear`, and `doctor` go through `atm-daemon`
  - `send` and `ack` still call SQLite-backed services directly
- workflow sidecar state is still transitional compatibility machinery during
  Q.5 and must not be treated as the future graft-facing truth surface

Architectural consequence:
- `atm-graft` planning must target the follow-on extraction and completion work
  required by Q.5, not the older published crate surface

## 2.2 Boundary Model

Phase Q uses this split for embedded host-agent integration:

- `atm-core` owns the semantic client protocol contract
- `atm-daemon` owns request handling, post-send-event generation, and nudge
  delivery
- `atm-graft` owns the concrete same-host daemon client, graft-session
  lifecycle, and host bridge

Architectural rules:
- first-party Rust host agents must not invent a parallel transport or
  alternate daemon contract outside the `atm-core` client models consumed by
  `atm-graft`
- all structs, enums, and traits needed by `atm-graft` must live in
  `atm-core`, even when the daemon is the concrete runtime peer
- concrete socket/runtime code may remain outside `atm-core`, but it must bind
  only to `atm-core` protocol and control-state models

## 2.3 Required `atm-core` Surfaces

The current Q.5 `dispatcher` module is only a partial precursor.

Required follow-on ownership in `atm-core`:
- typed request structs for:
  - `send`
  - `read`
  - `ack`
  - `clear`
  - `doctor`
  - `heartbeat`
  - graft registration / unregistration
- typed response structs for the same request families
- typed daemon-originated event structs for at least:
  - `NudgeEvent`
  - registration rejection / shutdown notifications if the protocol uses them
- typed control-state and same-host endpoint models needed by socket clients
- versioned binary frame-header models with transport-scoped correlation ids
- typed wire envelope / framing helpers that do not require a dependency on
  `atm-daemon`
- small client-facing traits for request execution and graft-session event
  streams

Rust boundary rules:
- semantic request / response / event types must not remain raw
  `serde_json::Value` payloads; this is a direct `RBP-004` newtype/typed-model
  requirement for a published crate boundary
- transport correlation ids must use a distinct semantic wrapper type rather
  than reusing ATM mail `message_id` semantics
- any new public client or stream traits must make an explicit sealed/open
  decision up front; default posture is sealed (`RBP-003`)
- stream-oriented traits intended for dynamic dispatch must remain object-safe
  (`RBP-008`)

## 2.4 Activation And Config Boundary

`atm-graft` is active only inside an ATM-configured project.

Architectural rules:
- `.atm.toml` discovery gates whether graft mode is active at all
- if `.atm.toml` is absent, `atm-graft` remains inert
- runtime identity comes from `ATM_IDENTITY`; graft mode does not add a second
  identity-resolution scheme
- optional graft-specific config remains ATM-owned config semantics rather than
  host-private settings
- the initial graft config surface must stay small:
  - `[atm.graft].enabled = true|false`
  - endpoint override only if Q.5 follow-on work proves it necessary

Architectural consequence:
- because Q.5 has no `[atm.graft]` model yet, `atm-core` config loading must be
  extended before `atm-graft` activation logic can be implemented

## 2.5 Graft Session

The active runtime object is `GraftSession`.

Responsibilities:
- connect to the same-host daemon API
- register the current host-agent identity and process context
- receive daemon-originated nudge events
- expose fetched daemon-owned nudges to the embedding host executable
- shut down cleanly and unregister when appropriate

Architectural rules:
- `GraftSession` registration is automatic by default when graft mode is active
- disconnect / reconnect behavior belongs to `atm-graft`, not to the host
  executable's business logic
- session lifecycle failures remain typed and observable; they must not collapse
  into silent disabled behavior after activation succeeded

Queue-ownership rule:
- bounded pending-nudge state belongs in the daemon
- `atm-graft` may keep only transient fetched state needed to hand nudges to
  the embedding host
- any daemon-queue overflow/backpressure behavior must emit structured
  observability and must not affect durable ATM mail truth

State-model rule:
- the runtime must keep the lifecycle explicit at least across:
  - inactive
  - connecting
  - registered
  - disconnected / retrying
  - closed
- whether this becomes a full typestate API is deferred, but the documented
  state machine must remain clear (`RBP-002`)

## 2.6 Nudge Delivery Model

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
- fetched nudge drain order must preserve daemon queue order from the
  perspective of the embedding agent loop
- nudges are advisory delivery signals, not durable mail truth; authoritative
  message state remains behind daemon-backed `read` calls

Alternate integration rule:
- the same pending nudge state must also be accessible through a daemon poll /
  drain request so hook-driven hosts can fetch insertion text without embedding
  `atm-graft`
- this hook-driven path belongs to the `atm` CLI surface, not to a separate
  `atm-graft` executable

Implementation-target rule:
- because Q.5 currently has only unary request/response transport, graft
  delivery requires a new long-lived registration/session protocol rather than
  reuse of the current one-shot request path
- every daemon-originated event frame still uses the shared binary header and
  its transport-scoped `WireMessageId`

## 2.7 Client API Boundary

`atm-graft` should expose a deliberately small public surface.

Required public capability groups:
- graft-session lifecycle
- same-host daemon client calls for:
  - `send`
  - `read`
  - `ack`
- host-facing nudge fetch / drain access

Boundary rule:
- a hook-facing command that prints insertion-ready nudge text is an `atm`
  command built on the same daemon API, not a CLI owned by the `atm-graft`
  crate itself

Architectural rules:
- `read` uses the daemon API rather than direct SQLite access
- `send` and `ack` must eventually use the same daemon-backed semantic
  contract as `atm`
- any optional runtime heartbeat or activity reporting must also use the daemon
  API instead of side channels

Implementation-target note:
- Q.5 does not yet satisfy the `send` / `ack` daemon-path rule, so the `atm`
  crate must converge on the same client protocol before `atm-graft` can share
  that boundary cleanly

## 2.8 Observability Boundary

`atm-graft` owns its own runtime/client observability.

Architectural rules:
- graft-side events emit through `sc-observability`
- graft-side events remain separate from daemon-owned runtime and transport
  events
- host-agent embedding must not require `atm-core` to depend directly on
  `sc-observability`
- registration, reconnect, queue-overflow, and daemon-unavailable paths must
  keep typed error identity with recovery guidance (`RBP-001`)

## 3. ADR Namespace

The `atm-graft` crate uses the `ADR-GRAFT-*` namespace.

Initial use cases:

- activation and inert-mode decisions
- runtime adapter design
- graft-session registration lifecycle
- host queue / injection bridge behavior
