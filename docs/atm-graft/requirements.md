# ATM-Graft Crate Requirements

## 1. Purpose

This document defines the `atm-graft` crate requirements.

The `atm-graft` crate owns the embedded Rust host-agent integration surface for
the daemon/runtime line. Product behavior remains defined in
[`../requirements.md`](../requirements.md). `atm-graft` must satisfy those
product requirements without re-owning `atm-core` service semantics,
`atm-daemon` runtime behavior, or storage durability.

## 2. Ownership

`atm-graft` owns:

- same-host daemon-client integration for linked Rust host-agent executables
- graft-session lifecycle
- receiver-owned same-host post-send listener when graft mode is active
- receipt of daemon-originated `PostSendHookEvent` payloads for host injection
- automatic between-tool-call nudge injection bridge for the embedding host
- one receiver-local listener thread while the session is active
- graft-mode activation rules based on discovered `.atm.toml`
- graft-side observability through an ATM-owned injected boundary supplied by
  the embedding host

`atm-graft` does not own:

- daemon business logic
- daemon-owned queue state
- direct SQLite access
- direct inbox JSONL parsing or writes
- direct ownership of ATM semantic types that already belong to `atm-core`
- forced interruption of a running tool call inside the host executable
- runtime/storage composition ownership
- daemon advisory registration, fetch/drain, or stream/session protocols

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
  config-loading behavior.
- `REQ-GRAFT-RUNTIME-001` `atm-graft` owns the runtime-neutral graft-session
  lifecycle used by linked Rust host agents.
- `REQ-GRAFT-CLIENT-001` `atm-graft` owns the embedded same-host daemon client
  surface for first-party Rust host agents.
- `REQ-GRAFT-NOTIFY-001` `atm-graft` owns the host-facing post-send delivery
  contract and structured payload rendering used for between-tool-call
  injection.
- `REQ-GRAFT-OBS-001` `atm-graft` owns graft-side structured observability
  emission for activation, connectivity, listener health, and nudge-delivery
  behavior.

## 4. Required References

The `atm-graft` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../atm-error-codes.md`](../atm-error-codes.md)
- [`../atm-core/requirements.md`](../atm-core/requirements.md)
- [`../atm-core/architecture.md`](../atm-core/architecture.md)
- [`../atm-daemon/requirements.md`](../atm-daemon/requirements.md)
- [`../atm-daemon/architecture.md`](../atm-daemon/architecture.md)

## 5. Embedded-Graft Rules

Requirement IDs:
- `REQ-GRAFT-CONFIG-001`
- `REQ-GRAFT-RUNTIME-001`
- `REQ-GRAFT-CLIENT-001`
- `REQ-GRAFT-NOTIFY-001`
- `REQ-GRAFT-OBS-001`

Required rules:
- if no `.atm.toml` is discovered, `atm-graft` remains inactive and performs no
  nudge work
- the `atm-graft` crate must rely on ATM-owned config loading via
  `atm_core::load_atm_config`; it must not privately reparse `.atm.toml`
- if graft mode is active, runtime identity comes from ATM-owned identity
  resolution; graft mode must not invent a separate identity source
- if graft mode is active and ATM launch conditions are met, `atm-graft` may
  attempt the standard supervised daemon auto-start path instead of requiring
  the daemon to be pre-running
- graft mode is enabled by default when active and may be disabled only by
  explicit config or runtime opt-out
- `atm-graft` must use the same-host daemon API for:
  - `send`
  - `read`
  - `ack`
  - `list`
- `atm-graft` must not bypass the daemon by talking directly to SQLite or inbox
  JSONL
- `atm-graft` must not obtain same-host daemon bootstrap convenience by taking
  a compile-time dependency on runtime/storage composition crates
- the host-facing nudge payload is structured and must contain at least:
  - `sender`
  - `sender_team`
  - `recipient`
  - `recipient_team`
  - `message_id`
  - `message`
  - `requires_ack`
- in embedded mode, `atm-graft` must automatically surface daemon-originated
  nudges into the host's between-tool-call context flow by binding one
  receiver-owned same-host local socket, accepting one bounded
  `PostSendHookEvent` request per connection, and returning one typed reply
- embedded mode must keep one persistent receiver listener task/thread while
  the session is active
- production `atm-graft` acceptance must not depend on `tmux send-keys`,
  shell-hook polling, or any equivalent external terminal-injection mechanism
- the intended production integration is a custom host CLI with `atm-graft`
  linked in-process so context injection happens without terminal automation
- the host executable owns the final insertion point between tool calls, but
  `atm-graft` must drive that path automatically through its session/runtime
  bridge rather than exposing only a passive fetch API
- `atm-graft` must expose a small library surface rather than mirroring the
  full CLI:
  - daemon client operations for `send`, `read`, `ack`, and `list`
  - graft-session lifecycle entrypoints
  - host-facing automatic nudge-delivery integration points
- the public surface must include:
  - `GraftClient`
  - `GraftSession`
  - `HostNudgeInjector`
  - `GraftObservability`
- the shared `atm-core` public contract for that surface is the existing unary
  transport and typed protocol DTOs for `send`, `read`, `ack`, `list`, and
  `PostSendHookEvent`
- the standard convenience `GraftClient::connect()` path must reuse the same
  thin-client same-host bootstrap helper seam owned by `atm-daemon-client` and
  used by the CLI rather than inventing a graft-private bootstrap path
- the thin-client bootstrap helper seam may resolve the canonical same-host
  endpoint, daemon binary, and supervised auto-start behavior, but it must not
  introduce a direct `atm-daemon-bootstrap` dependency or a transitive
  dependency on concrete storage backends
- `atm-graft` compatibility with the primary `atm` install is defined by the
  documented same-host RPC surface, not by a requirement that both crates ship
  in lockstep versions
- `atm-graft` must emit structured observability for:
  - active / inactive graft mode
  - daemon connect / reconnect
  - listener health
  - nudge delivery success / failure
- the observability boundary must be injected by the host binary; `atm-graft`
  must not require a direct public dependency on `sc-observability`

## 5.1 Req-QA Verification Anchors

`req-qa` should treat these as fail-closed presence checks:

- `REQ-GRAFT-CONFIG-001`
  - `[atm.graft].enabled` exists in ATM-owned config models and config loading
  - `atm_core::load_atm_config` is the public loader consumed by `atm-graft`
  - `atm-graft` remains inert when `.atm.toml` is absent
- `REQ-GRAFT-RUNTIME-001`
  - `GraftSession` owns the receiver-local listener thread
  - receiver-local state is limited to lifecycle state and bounded in-flight
    nudge handling
  - no daemon advisory registration, fetch/drain, or stream/session protocol is
    required
- `REQ-GRAFT-CLIENT-001`
  - the public embedded client surface supports `send`, `read`, `ack`, and
    `list`
  - the concrete exported types include `GraftClient` and `GraftSession`
  - `atm-graft` does not take a Rust dependency on `atm-daemon`
  - the standard convenience connection path uses the shared thin-client
    bootstrap seam instead of a graft-private runtime/storage composition path
- `REQ-GRAFT-NOTIFY-001`
  - daemon-originated nudge receipt is automatic in embedded mode
  - host-facing delivery uses `PostSendHookEvent`
  - delivery uses one bounded same-host request/reply exchange per nudge
  - no acceptance path relies on `tmux send-keys` or external terminal key
    injection as the delivery mechanism
- `REQ-GRAFT-OBS-001`
  - graft activation/connectivity/listener/nudge paths emit through an
    injected ATM-owned observability boundary
  - the concrete exported observability/injection seams include
    `GraftObservability` and `HostNudgeInjector`
