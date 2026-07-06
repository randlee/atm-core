# ATM-Graft Crate Architecture

## 1. Purpose

This document defines the architectural boundary for `atm-graft`.

`atm-graft` is the thin embedded Rust client used by a host agent that wants
automatic ATM nudges without shell-hook or tmux automation. It complements the
product architecture in [`../architecture.md`](../architecture.md) and owns
only graft-local client/runtime behavior.

## 2. Core Boundary

- `atm-core` owns semantic request/response types, config loading, shared error
  vocabulary, and the host-facing `PostSendHookEvent`.
- `atm-daemon-client` owns same-host endpoint resolution, daemon binary
  resolution, probe, and supervised daemon bootstrap convenience.
- `atm-graft` owns the concrete thin client, the receiver-owned poll loop, and
  the host injection seam.
- `atm-graft` must not depend on `atm-daemon` as a Rust crate.
- `atm-graft` must not depend on storage crates or read SQLite directly.
- `atm-graft` must not reintroduce graft-private protocol families when the
  shared unary ATM protocol is sufficient.

## 3. Activation Rules

- `atm-graft` is active only inside an ATM-configured project.
- `.atm.toml` discovery gates whether graft mode is active at all.
- if `.atm.toml` is absent, `atm-graft` remains inert.
- runtime identity comes from ATM-owned identity resolution; `atm-graft` does
  not invent a second identity-resolution path.
- `[atm.graft].enabled = false` disables the receiver runtime while preserving
  access to the thin client surface.
- `GraftClient::connect()` may use the standard daemon bootstrap helper seam
  once ATM launch conditions are met.

## 4. Public Surface

The retained public surface is intentionally small:

- `GraftClient`
- `GraftSession`
- `GraftSessionOptions`
- `GraftSessionState`
- `HostNudgeInjector`
- `GraftObservability`
- `SessionSnapshot`

Rules:

- `GraftClient` routes `send`, `read`, `ack`, and `list` over the shared unary
  ATM request/response protocol.
- `HostNudgeInjector` consumes `PostSendHookEvent`, not a graft-private nudge
  DTO.
- `GraftObservability` reports graft-local lifecycle and delivery outcomes, but
  the embedding host owns the actual observability backend.
- no public API may expose daemon-owned advisory registration, fetch/drain, or
  stream/session concepts.

## 5. Runtime Model

`GraftSession` owns one receiver-local background thread while the session is
active.

That thread:

- periodically issues a unary `list` request for unread messages
- issues an exact unary `read` for each unread message id not yet injected
- projects the durable message into `PostSendHookEvent`
- calls the injected `HostNudgeInjector`
- tracks only the minimal in-memory delivered-id set needed to avoid
  reinjecting the same still-unread message

The session does not:

- register with the daemon
- hold a dedicated advisory stream open
- rely on daemon-owned graft queues
- expose graft-private session ids
- persist host-local delivery state

Lifecycle states are:

- `Inactive`
- `Polling`
- `Degraded`
- `Closed`
- `CloseFailed`

## 6. Nudge Contract

`atm-graft` delivers the shared `PostSendHookEvent` to the embedding host.

Required event fields are:

- sender
- sender team
- recipient
- recipient team
- message id
- message text
- ack flag
- ack/reply linkage metadata when present

Architectural rules:

- the durable ATM message remains the source of truth
- `atm-graft` does not own mailbox semantics; it only reads through the shared
  daemon API
- a host-specific projection may exist inside the embedding executable, but it
  must stay outside `atm-graft`
- external terminal automation such as `tmux send-keys` remains out of scope
  for embedded graft delivery

## 7. Compatibility Rules

- compatibility is defined by the shared same-host RPC surface, not by lockstep
  version equality between `atm-graft` and the primary `atm` install
- transport correlation ids and ATM message ids must remain distinct semantic
  types
- semantic payloads must stay typed; do not replace them with ad hoc JSON blobs
- any future alternative storage backend remains behind the daemon boundary and
  does not change `atm-graft`’s contract
