# ATM-Graft Crate Requirements

## 1. Purpose

This document defines the `atm-graft` crate requirements.

The `atm-graft` crate owns the embedded Rust host-agent integration surface for
the post-Phase-S daemon/runtime line. Product behavior remains defined in
[`../requirements.md`](../requirements.md). `atm-graft` must satisfy those
product requirements without re-owning `atm-core` service semantics,
`atm-daemon` runtime behavior, or `atm-rusqlite` durability.

## 2. Ownership

`atm-graft` owns:

- same-host daemon-client integration for linked Rust host-agent executables
- graft-session registration and lifecycle
- automatic daemon-originated nudge subscription when graft mode is active
- automatic between-tool-call nudge injection bridge for the embedding host
- one persistent receive thread plus open daemon connection for advisory nudges
- host wake/event signaling when a new nudge arrives while the host is idle
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
  `REQ-P-GRAFT-001`, `REQ-CORE-TRANSPORT-001`.
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
- [`../plan-phase-U.md`](../plan-phase-U.md)
- [`../plan-atm-graft.md`](../plan-atm-graft.md)
- [`../project-plan.md`](../project-plan.md)
- [`../documentation-guidelines.md`](../documentation-guidelines.md)
- [`../atm-error-codes.md`](../atm-error-codes.md)
- [`../atm-core/requirements.md`](../atm-core/requirements.md)
- [`../atm-core/architecture.md`](../atm-core/architecture.md)
- [`../atm-daemon/requirements.md`](../atm-daemon/requirements.md)
- [`../atm-daemon/architecture.md`](../atm-daemon/architecture.md)
- [`../atm-daemon/protocol-icd.md`](../atm-daemon/protocol-icd.md)

## 5. Phase U Embedded-Graft Rules

Requirement IDs:
- `REQ-GRAFT-CONFIG-001`
- `REQ-GRAFT-RUNTIME-001`
- `REQ-GRAFT-CLIENT-001`
- `REQ-GRAFT-NOTIFY-001`
- `REQ-GRAFT-OBS-001`

Required rules:
- if no `.atm.toml` is discovered, `atm-graft` remains inactive and performs no
  daemon registration or nudge work
- the `atm-graft` crate must rely on ATM-owned config loading via
  `atm_core::load_atm_config`; it must not privately reparse `.atm.toml`
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
- in embedded mode, `atm-graft` must automatically surface daemon-originated
  nudges into the host's between-tool-call context flow; manual polling is not
  sufficient for `atm-graft` acceptance
- embedded mode must keep one persistent receive task/thread and one live
  daemon connection dedicated to advisory nudges while the session is active
- if the host is idle when a nudge arrives, `atm-graft` must enqueue the
  received nudge until host consumption and fire a host wake/event signal so
  the host takes follow-on action promptly
- production `atm-graft` acceptance must not depend on `tmux send-keys`,
  shell-hook polling, or any equivalent external terminal-injection mechanism
- the intended production integration is a custom host CLI with `atm-graft`
  linked in-process so context injection happens without terminal automation
- the host executable owns the final insertion point between tool calls, but
  `atm-graft` must drive that path automatically through its session/runtime
  bridge rather than exposing only a passive fetch API
- `atm-graft` must expose a small library surface rather than mirroring the
  full CLI:
  - daemon client operations for `send`, `read`, and `ack`
  - graft-session lifecycle entrypoints
  - host-facing automatic nudge-delivery integration points
- the concrete `U.9` public surface must include:
  - `GraftClient`
  - `GraftSession`
  - `HostNudgeInjector`
  - `GraftObservability`
- the shared `atm-core` public contract for that surface is:
  - existing shared transport and protocol DTOs for unary `send` / `read` /
    `ack`
  - typed shared DTOs for registration, unregistration, and nudge delivery
  - no graft-specific public trait family unless the shared boundary proves
    insufficient
- any hook-facing command that renders insertion-ready nudge text belongs on
  the `atm` CLI surface and must call the same daemon API used by `atm-graft`,
  but it is not a production substitute for embedded-mode automatic injection
- `atm-graft` must emit structured observability for:
  - active / inactive graft mode
  - daemon connect / reconnect
  - registration success / failure
  - nudge received / fetched
  - daemon-reported nudge drop / backpressure signals when surfaced through the
    shared API
  - the observability boundary must be injected by the host binary; `atm-graft`
    must not require a direct public dependency on `sc-observability`

## 5.1 Current Baseline

The current daemon/runtime line already satisfies more of the embedded-client
baseline than the earlier Phase Q planning assumed.

Current planning baseline:
- `integrate/phase-T @ 75d341b`

Baseline assumptions for `atm-graft` planning:
- same-host daemon singleton/runtime ownership already exists
- same-host request/response transport already exists
- retained CLI operations already route through the daemon/runtime line for the
  transport-backed paths
- Windows runtime parity, SQLite writer-lane hardening, and remaining daemon
  shutdown/state hardening still complete first as `T.2`-`T.5`

Remaining prerequisites specific to `atm-graft`:
- a small embeddable client surface owned by `atm-core`
- graft-session registration and unregistration
- one persistent receive loop for embedded sessions plus daemon-owned bounded
  nudge queue/drain or advisory-stream access
- minimal `[atm.graft]` config support in ATM-owned config loading

Scope-simplification rule for the first implementation pass:
- `atm-graft` v1 should keep its public API to `send`, `read`, `ack`,
  `GraftSession`, and automatic host-facing nudge injection integration
- runtime heartbeat / activity reporting is explicitly deferred unless host
  integration proves it is needed in the same sprint

## 5.2 Req-QA Verification Anchors

`req-qa` should treat these as fail-closed presence checks:

- `REQ-GRAFT-CONFIG-001`
  - `[atm.graft].enabled` exists in ATM-owned config models and config loading
  - `atm_core::load_atm_config` is the public loader consumed by `atm-graft`
  - `atm-graft` remains inert when `.atm.toml` is absent
- `REQ-GRAFT-RUNTIME-001`
  - daemon-owned registration, unregistration, and typed queue fetch/drain
    surfaces exist before the `atm-graft` crate tries to consume them
  - the concrete `GraftSession` lifecycle type exists once `U.9` lands
  - embedded mode includes one persistent receive task/thread plus one live
    daemon connection per active `GraftSession` once `U.9` lands
  - the client runtime queues nudges until host consumption and triggers a host
    wake/event signal when new nudges arrive while the host is idle
  - registration plus clean shutdown/unregistration are test-covered at the
    daemon/runtime and crate-consumer layers
- `REQ-GRAFT-CLIENT-001`
  - the public embedded client surface supports `send`, `read`, and `ack`
  - the public session-facing surface supports graft registration,
    unregistration, and typed nudge fetch/drain requests
  - the concrete exported types include `GraftClient` and `GraftSession`
  - `atm-graft` does not take a Rust dependency on `atm-daemon`
- `REQ-GRAFT-NOTIFY-001`
  - daemon-originated nudge receipt is automatic in embedded mode
  - daemon-owned drain/fetch or subscription surfaces exist for the
    session/runtime bridge
  - the host-facing nudge payload exposes at least `from` and `message`
  - the client runtime queues nudges until host consumption and emits a host
    wake/event signal on arrival
  - no acceptance path relies on `tmux send-keys` or external terminal key
    injection as the delivery mechanism
- `REQ-GRAFT-OBS-001`
  - graft activation/connectivity/registration/nudge paths emit through an
    injected ATM-owned observability boundary
  - the concrete exported observability/injection seams include
    `GraftObservability` and `HostNudgeInjector`
