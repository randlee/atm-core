# ATM-Graft Crate Requirements

## 1. Purpose

This document defines the `atm-graft` crate requirements.

The `atm-graft` crate owns the embedded Rust host-agent integration surface.
Product behavior remains defined in
[`../requirements.md`](../requirements.md). `atm-graft` must satisfy those
product requirements without re-owning `atm-core` service semantics,
`atm-daemon` runtime behavior, or `atm-rusqlite` durability.

Phase AI replaces the legacy same-host framed client with ADR-033's HTTP/UDS
`DaemonApiClient`. Graft is one client of that API; it must not preserve a
graft-specific protocol, frame codec, write path, or chat-address parser.

## 2. Ownership

`atm-graft` owns:

- same-host daemon-client integration for linked Rust host-agent executables
- graft lifecycle activation and lifecycle
- automatic graft-backed post-send receipt when graft mode is active
- automatic between-tool-call nudge injection bridge for the embedding host
- host wake/event signaling when a new nudge arrives while the host is idle
- graft-mode activation rules based on discovered `.atm.toml`
- graft-side observability through an ATM-owned injected boundary supplied by
  the embedding host

`atm-graft` does not own:

- daemon business logic
- direct SQLite access
- direct inbox JSONL parsing or writes
- direct ownership of ATM semantic types that already belong to `atm-core`
- forced interruption of a running tool call inside the host executable
- runtime/storage composition ownership
- shared daemon packet families for receiver-private lifecycle or stream behavior

## 3. Requirement Namespace

The `atm-graft` crate uses the `REQ-GRAFT-*` namespace.

Initial allocation:

- `REQ-GRAFT-CONFIG-*`
- `REQ-GRAFT-RUNTIME-*`
- `REQ-GRAFT-CLIENT-*`
- `REQ-GRAFT-NOTIFY-*`
- `REQ-GRAFT-OBS-*`
- `REQ-GRAFT-PYTHON-*`
- `REQ-GRAFT-HERMES-*`

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
- `REQ-GRAFT-NOTIFY-001` `atm-graft` owns the host-facing post-send nudge
  delivery
  contract and structured payload rendering used for between-tool-call
  injection. Satisfies:
  `REQ-P-GRAFT-001`.
- `REQ-GRAFT-OBS-001` `atm-graft` owns graft-side structured observability
  emission for activation, connectivity, receiver handoff, and
  receiver-local buffering
  behavior. Satisfies:
  `REQ-P-OBS-001`, `REQ-P-GRAFT-001`.
- `REQ-GRAFT-PYTHON-001` AI.18 may expose the existing typed graft API through
  PyO3/Maturin. The binding is a translation layer over `DaemonApiClient`; it
  must not open a socket, access storage, serialize a graft-private message,
  or add another send/read/ack path.
- `REQ-GRAFT-HERMES-001` AI.17–AI.21 may map a Hermes ambient key to ADR-037
  `ChatId` and inject a canonical post-write nudge into Hermes. The adapter
  must use structured `AgentAddress`, preserve the immutable message ID, and
  must not create a session header, daemon session, or transport-specific
  routing policy.
- `REQ-GRAFT-RUNTIME-002` A graft receiver has exactly one active owner for a
  `(canonical_graft_root, team, agent)` endpoint record. Activation must either
  acquire that ownership or fail with a typed conflict; it must never silently
  replace a live receiver. Close may remove a record only when it still owns
  the published generation. A crashed owner must be reclaimable without an
  operator deleting a stale file.
- `REQ-GRAFT-NOTIFY-002` Graft nudges are bounded wake-up signals only. They
  are never mail storage, a durable retry queue, a conversation manager, or a
  second mailbox. The daemon mailbox remains the source of truth.
- `REQ-GRAFT-HERMES-002` One Hermes profile binds one ATM identity and its
  configured ADR-037 `ChatId` host session. Live and recovery wake-ups must
  enter that host session through its non-interrupting steer path, not through
  normal inbound-user-message dispatch. The full source address, including a
  present chat-id, remains available for attribution and reply routing.
- `REQ-GRAFT-HERMES-003` After a Hermes profile receiver becomes listening,
  its Python adapter waits exactly ten seconds once, reads durable mailbox
  bucket counts through the ordinary daemon API, and emits at most one concise
  steer summary when unread or pending-ack counts are non-zero. The summary is
  advisory; it neither reads, acknowledges, persists, nor replays mail.

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
- [`../atm-daemon/http-api.md`](../atm-daemon/http-api.md)

## 5. Phase U Embedded-Graft Rules

Requirement IDs:
- `REQ-GRAFT-CONFIG-001`
- `REQ-GRAFT-RUNTIME-001`
- `REQ-GRAFT-CLIENT-001`
- `REQ-GRAFT-NOTIFY-001`
- `REQ-GRAFT-OBS-001`

Required rules:
- if no `.atm.toml` is discovered, `atm-graft` remains inactive and performs no
  daemon interaction or post-send handoff work
- the `atm-graft` crate must rely on ATM-owned config loading via
  `atm_core::load_atm_config`; it must not privately reparse `.atm.toml`
- if graft mode is active, runtime identity comes from `ATM_IDENTITY`; graft
  mode must not invent a separate identity source
- a caller chat-id, when present, is part of the explicit ADR-037
  `AgentAddress`; `GraftSession` is host-local lifecycle state and must not be
  serialized as a daemon session or substitute for `ChatId`
- if graft mode is active, `ATM_IDENTITY` and `ATM_TEAM` resolve successfully,
  and the standard same-host daemon endpoint can be derived from ATM-owned
  config/environment inputs, `atm-graft` may attempt the standard supervised
  daemon auto-start path instead of requiring the daemon to be pre-running
- graft mode is enabled by default when active and may be disabled only by
  explicit config or runtime opt-out
- `atm-graft` must use the same-host daemon API for:
  - `send`
  - `read`
  - `ack`
  - optional runtime heartbeat / activity reporting when the host enables it
- graft-backed post-send receipt must come through the ATM-owned post-send
  capability seam rather than a graft-private bypass around ATM send semantics
- `atm-graft` must not bypass the daemon by talking directly to SQLite or inbox
  JSONL
- `atm-graft` must not obtain same-host daemon bootstrap convenience by taking
  a compile-time dependency on runtime/storage composition crates
- `atm-graft` must not require daemon-owned graft session registration,
  daemon-owned pending-nudge queues, or a dedicated shared advisory-stream
  packet family as part of the accepted contract
- `GraftNudgeSink` is the graft-backed concrete receiver sink behind the shared
  post-send capability seam; it is a sibling of `TmuxNudgeSink`, not a daemon
  subsystem
- the host-facing nudge payload is structured and must contain at least:
  - `from`
  - `message_id`
  - `task_id` when the selected template kind is task-scoped
- delivery-family nudges must additionally carry `description`
- acknowledge-family nudges must preserve the compact built-in envelope shape
  and must not add delivery-only body text
- in embedded mode, `atm-graft` must automatically surface daemon-originated
  nudges into the host's between-tool-call context flow; manual polling is not
  sufficient for `atm-graft` acceptance
- same-host host-nudge delivery tests must use explicit receiver-readiness
  signaling before asserting injection success; acceptance must not depend on
  scheduler luck or on a shorter `#[cfg(test)]` delivery deadline than the
  accepted production path
- if the host is idle when a nudge arrives, `atm-graft` must deliver one
  bounded receiver-local wake-up through its host injection seam and fire the
  host wake/event signal promptly. Any receiver-local buffer is transient,
  bounded, and contains only nudge metadata; it must not become mail storage
  or a durable retry queue.
- the exact task/thread/callback mechanism used for that behavior is private to
  `atm-graft` and is not fixed by this requirement doc
- production `atm-graft` acceptance must not depend on `tmux send-keys`,
  shell-hook polling, or any equivalent external terminal-injection mechanism
- the intended production integration is a custom host CLI with `atm-graft`
  linked in-process so context injection happens without terminal automation
- the host executable owns the final insertion point between tool calls, but
  `atm-graft` must drive that path automatically through its host injection
  seam rather than exposing only a passive fetch API
- `atm-graft` must expose a small library surface rather than mirroring the
  full CLI:
  - daemon client operations for `send`, `read`, and `ack`
  - minimal graft activation/lifecycle entrypoints
  - host-facing automatic nudge-delivery integration points
- the concrete `U.9` public surface must include:
  - `GraftClient`
  - `GraftSession`
  - `HostNudgeInjector`
  - `GraftObservability`
- the shared `atm-core` public contract for that surface is:
  - `DaemonApiClient` plus canonical `AgentAddress`/`WriteRequest` DTOs for
    unary `send` / `read` / `ack`
  - no graft-specific public trait family unless the shared boundary proves
    insufficient
- the standard convenience `GraftClient::connect()` path must reuse the same
  thin-client same-host bootstrap helper seam owned by `atm-daemon-client` and
  used by the CLI rather than inventing a graft-private bootstrap path
- the thin-client bootstrap helper seam may resolve the canonical same-host
  endpoint, daemon binary, and supervised auto-start behavior, but it must not
  introduce a direct `atm-daemon-bootstrap` dependency or a transitive
  dependency on `atm-runtime`, `atm-storage-rusqlite`, or other concrete
  storage backends
- `atm-graft` compatibility with the primary `atm` install is defined by the
  documented same-host HTTP API, not by a requirement that both crates ship
  in lockstep versions
- any hook-facing command that renders insertion-ready nudge text belongs on
  the `atm` CLI surface and must call the same daemon API used by `atm-graft`,
  but it is not a production substitute for embedded-mode automatic injection
- `atm-graft` must emit structured observability for:
  - active / inactive graft mode
  - daemon connect / reconnect
  - receiver activation success / failure
  - nudge received / host handoff
  - receiver-local buffering/backpressure signals
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
  - the concrete `GraftSession` lifecycle type exists once `U.9` lands
  - embedded mode automatically surfaces post-send nudges to the host and
    triggers a host wake/event signal when new nudges arrive while the host is
    idle
  - receiver-local buffering and shutdown behavior are test-covered at the
    crate-consumer layer
- `REQ-GRAFT-CLIENT-001`
  - the public embedded client surface supports `send`, `read`, and `ack`
  - the concrete exported types include `GraftClient` and `GraftSession`
  - `atm-graft` does not take a Rust dependency on `atm-daemon`
  - the standard convenience connection path uses the shared thin-client
    bootstrap seam instead of a graft-private runtime/storage composition path
  - `cargo tree -p atm-graft -e normal --prefix none` does not pull
    `atm-runtime` or `atm-storage-rusqlite` solely to support same-host daemon
    bootstrap convenience
- `REQ-GRAFT-NOTIFY-001`
  - daemon-originated nudge receipt is automatic in embedded mode
  - the host-facing nudge payload exposes at least `from` and `message_id`
  - the client runtime delivers bounded transient nudges through the host
    injection seam and emits a host wake/event signal on arrival; durable mail
    recovery is supplied by the ordinary daemon read contract, not a graft
    queue
  - no acceptance path relies on `tmux send-keys` or external terminal key
    injection as the delivery mechanism
- `REQ-GRAFT-OBS-001`
  - graft activation/connectivity/receiver-handoff paths emit through an
    injected ATM-owned observability boundary
  - the concrete exported observability/injection seams include
    `GraftObservability` and `HostNudgeInjector`
- `REQ-GRAFT-RUNTIME-002`
  - a second simultaneous activation for the same canonical root/team/agent
    fails without changing the current endpoint record
  - closing an old receiver cannot remove a successor's endpoint record
  - an abandoned owner can be reclaimed after process death
- `REQ-GRAFT-HERMES-002` and `REQ-GRAFT-HERMES-003`
  - an adapter test proves live graft notification invokes the configured steer
    seam and never ordinary user-message ingress
  - a restart test proves exactly one summary steer after ten seconds when
    durable `bucket_counts.unread` or `bucket_counts.pending_ack` is non-zero
  - a zero-count restart emits no steer; a live nudge during the delay remains
    a normal one-event wake-up
