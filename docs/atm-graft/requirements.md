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
- automatic between-tool-call steer-nudge injection bridge for the embedding host
- host wake/event signaling when a new steer nudge arrives while the host is idle
- identity-driven graft receiver activation with optional ATM-owned `.atm.toml`
  configuration
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
- `REQ-GRAFT-NOTIFY-001` `atm-graft` owns the host-facing post-send steer-nudge
  delivery
  contract and structured payload rendering used for between-tool-call
  injection. (Phase AQ adds a second, independent queue-shaped nudge channel
  to `atm-graft`; whether and how it lands is the harness integration's
  decision, not specified here.) Satisfies:
  `REQ-P-GRAFT-001`.
- `REQ-GRAFT-OBS-001` `atm-graft` owns graft-side structured observability
  emission for activation, connectivity, receiver handoff, and
  receiver-local buffering
  behavior. Satisfies:
  `REQ-P-OBS-001`, `REQ-P-GRAFT-001`.
- `REQ-GRAFT-PYTHON-001` AI.18 may expose the existing typed graft API through
  PyO3/Maturin. The binding is a translation layer over `DaemonApiClient`; it
  must not open a socket, access storage, serialize a graft-private message,
  or add another send/read path or a graft acknowledgement operation.
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
- `REQ-GRAFT-NOTIFY-002` Graft steer nudges are bounded wake-up signals only.
  They are never mail storage, a durable retry queue, a conversation manager,
  or a second mailbox. The daemon mailbox remains the source of truth.
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
- `REQ-GRAFT-HERMES-004` `hermes-atm` may register exactly the initial native
  Hermes tools `atm_send`, `atm_read`, and `atm_list` through a verified public
  Hermes tool-registration seam. Each tool's accepted arguments, defaults,
  validation, selection semantics, and bounded-result behavior must match its
  corresponding `atm send`, `atm read`, or `atm list` CLI command. Native
  `atm_read` is deliberately read-only: it uses `seen_state_update=false` and
  rejects every mutation, acknowledgement, or mark-seen input. Its parity is
  limited to supported read-only selection, filters, and result semantics.
  The host's installed profile configuration supplies
  ATM identity, team, home, and workspace root; a tool invocation must not
  override any of those values.
  `AtmSendRequest.requires_ack` is in scope, defaults to `false`, and maps to
  the ordinary `SendRequest` / CLI `--ack-required` send semantics. This does
  not add a separate graft or Python acknowledgement tool or mode:
  `AtmSendRequest.acknowledges_message_id` is an optional field on the same
  native `atm_send` path. When set it acknowledges the named pending-ack
  message through the canonical `WriteRequest` acknowledgement shape; `to`
  is omitted and `requires_ack` is false. The `atm ack` CLI command is
  unchanged and remains available for callers who prefer it.
- `REQ-GRAFT-HERMES-005` Native Hermes tools use only the public installed
  `atm_graft` Python API and the public Hermes registration API. They must not
  invoke the `atm` executable, create a second daemon client/receiver,
  introduce a poll or retry loop, call private extension symbols, or access
  SQLite, storage, or daemon lifecycle/network internals directly. The
  `atm_graft` Python surface may grow only as a thin typed translation of the
  ordinary daemon API necessary for CLI-equivalent `send`, `read`, and `list`.
- `REQ-GRAFT-HERMES-006` Every native tool result is JSON-compatible and uses
  this discriminated union: success is
  `{"kind":"success","result":...}`; failure is
  `{"kind":"error","error":{"code":...,"message":...,"recovery":...}}`.
  `AtmToolError` is exactly that nested `error` object; it never wraps the
  outer discriminated envelope. Its `layer` distinguishes ingress validation
  from environment/runtime/native-client failures.
  Failures must preserve a stable machine-readable code and provide a concrete
  recovery action without exposing profile secrets. Registration or invocation
  capability failures must fail closed and must not crash a running Hermes
  gateway.
- `REQ-GRAFT-HERMES-007` Coverage must prove CLI-equivalent argument
  validation and mutation semantics, bounded `atm_list` defaults and limits,
  identity/team non-overridability, `requires_ack` default and send mapping,
  and the absence of a graft/Python inbound-ack operation; native read's no-seen/no-ack mutation,
  discriminated success and error results,
  idempotent registration, and clean-wheel package discovery. A live Hermes
  proof is performed only after those isolated checks pass and must show a
  normal installed profile can use the registered tools without hand-editing
  package files or gateway code.
- `REQ-GRAFT-HERMES-008` The public Python agent-tool ingress is modeled with
  direct `pydantic` v2 request models owned by the public `atm-graft-python`
  Maturin wheel: `AtmSendRequest`, `AtmReadRequest`, and `AtmListRequest`.
  `pydantic` is a direct `atm-graft-python` package dependency. `hermes-atm`
  consumes those models and remains a thin Hermes-only adapter.
  Hermes handlers accept JSON-compatible input and call
  `model_validate` exactly once before translating to typed `PyGraftSession`
  send/read/list calls. Models reject unknown fields; validation rejects
  invalid arguments and mutating read requests before native transport.
  Trusted typed graft outcomes are converted directly to JSON-compatible
  `AtmSendResult`, `AtmReadResult`, `AtmListResult`, or shared `AtmToolError`
  result data; production must not re-validate the egress path. Validation
  failures normalize to the required JSON-safe error union. Raw JSON
  strings/dicts must never pass through `atm_graft`, and no Python layer may
  reproduce HTTP serialization. Tests cover ingress model-validation success
  and failure for every public tool.
- `REQ-GRAFT-HERMES-009` A long-lived Hermes native-tool session must refresh
  its public `PyGraftSession` client after an `ATM_DAEMON_UNAVAILABLE` result
  caused by an operator-managed daemon cycle. `atm_read` and `atm_list` may
  replay exactly once after that refresh because they are read-only. `atm_send`
  must refresh but must not replay automatically: it returns a structured
  retry-once recovery action because the original write may already have been
  accepted. Refreshing a client never starts, replaces, or otherwise controls
  the managed daemon.

AI.38 evidence note: the reference adapter calls only the injected Hermes
`session.steer` port with a runtime session id resolved from the configured
`ATM_CHAT_ID`. An accepted result is `queued`; rejected/error results are
visible failures with no normal-message fallback, retry loop, or mail mutation.
The adapter fails closed when Hermes has not registered a runtime session, and
never sends the platform chat id as `session_id`. The checked-in fixture proves
this for both a live steer nudge and the AI.37 recovery summary.

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
- a valid `ATM_IDENTITY` and `ATM_TEAM` activation envelope starts the receiver
  regardless of whether `.atm.toml` is present; activation may not
  successfully return with an inert receiver
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
- `.atm.toml` is optional configuration only. Its absence and legacy
  `[atm.graft].enabled` values must not gate receiver activation; malformed
  configuration, an invalid workspace root, ownership conflict, or bind
  failure must surface as an error
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
- `GraftReceiveHook` is the graft-backed concrete receiver hook behind the shared
  post-send capability seam; it is a sibling of `TmuxNudgeSink`, not a daemon
  subsystem
- the host-facing steer-nudge payload is structured and must contain at least:
  - `from`
  - `message_id`
  - `task_id` when the selected template kind is task-scoped
- delivery-family steer nudges must additionally carry `description`
- acknowledge-family steer nudges must preserve the compact built-in envelope
  shape and must not add delivery-only body text
- in embedded mode, `atm-graft` must automatically surface daemon-originated
  steer nudges into the host's between-tool-call context flow; manual
  polling is not sufficient for `atm-graft` acceptance
- same-host host-nudge delivery tests must use explicit receiver-readiness
  signaling before asserting injection success; acceptance must not depend on
  scheduler luck or on a shorter `#[cfg(test)]` delivery deadline than the
  accepted production path
- if the host is idle when a steer nudge arrives, `atm-graft` must deliver one
  bounded receiver-local wake-up through its host injection seam and fire the
  host wake/event signal promptly. Any receiver-local buffer is transient,
  bounded, and contains only steer-nudge metadata; it must not become mail
  storage or a durable retry queue.
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
  - daemon client operations for `send` and `read`
  - minimal graft activation/lifecycle entrypoints
  - host-facing automatic steer-nudge-delivery integration points
- the concrete `U.9` public surface must include:
  - `GraftClient`
  - `GraftSession`
  - `HostNudgeInjector`
  - `GraftObservability`
- the shared `atm-core` public contract for that surface is:
  - `DaemonApiClient` plus canonical `AgentAddress`/`WriteRequest` DTOs for
    unary `send` / `read`
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
- any hook-facing command that renders insertion-ready steer-nudge text belongs on
  the `atm` CLI surface and must call the same daemon API used by `atm-graft`,
  but it is not a production substitute for embedded-mode automatic injection
- `atm-graft` must emit structured observability for:
  - active / inactive graft mode
  - daemon connect / reconnect
  - receiver activation success / failure
  - steer nudge received / host handoff
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
- `atm-graft` v1 should keep its public API to `send`, `read`,
  `GraftSession`, and automatic host-facing steer-nudge injection integration
- runtime heartbeat / activity reporting is explicitly deferred unless host
  integration proves it is needed in the same sprint

## 5.2 Req-QA Verification Anchors

`req-qa` should treat these as fail-closed presence checks:

- `REQ-GRAFT-CONFIG-001`
  - optional ATM configuration is loaded through ATM-owned config models and
    config loading
  - `atm_core::load_atm_config` is the public loader consumed by `atm-graft`
  - a bare workspace receiver either reaches `listening` and publishes its
    endpoint record or returns an error; it never successfully no-ops
- `REQ-GRAFT-RUNTIME-001`
  - the concrete `GraftSession` lifecycle type exists once `U.9` lands
  - embedded mode automatically surfaces post-send steer nudges to the host
    and triggers a host wake/event signal when new steer nudges arrive while
    the host is idle
  - receiver-local buffering and shutdown behavior are test-covered at the
    crate-consumer layer
- `REQ-GRAFT-CLIENT-001`
  - the public embedded client surface supports `send` and `read`
  - the concrete exported types include `GraftClient` and `GraftSession`
  - `atm-graft` does not take a Rust dependency on `atm-daemon`
  - the standard convenience connection path uses the shared thin-client
    bootstrap seam instead of a graft-private runtime/storage composition path
  - `cargo tree -p atm-graft -e normal --prefix none` does not pull
    `atm-runtime` or `atm-storage-rusqlite` solely to support same-host daemon
    bootstrap convenience
- `REQ-GRAFT-NOTIFY-001`
  - daemon-originated steer-nudge receipt is automatic in embedded mode
  - the host-facing steer-nudge payload exposes at least `from` and
    `message_id`
  - the client runtime delivers bounded transient steer nudges through the
    host injection seam and emits a host wake/event signal on arrival;
    durable mail recovery is supplied by the ordinary daemon read contract,
    not a graft queue
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
  - a zero-count restart emits no steer; a live steer nudge during the delay
    remains a normal one-event wake-up
