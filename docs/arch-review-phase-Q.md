# Phase Q Architecture Review For `atm-graft`

## 1. Executive Summary

The Q.5 codebase is close enough to finalize the `atm-graft` boundary, but it
does not yet provide the implementation surfaces that `atm-graft` needs.

What is ready:
- durable SQLite-facing mail, task, and roster models in `atm-core`
- a same-host daemon runtime and auto-start path for retained CLI operations
- daemon-backed `read`, `clear`, `doctor`, and `heartbeat`

What is still missing:
- `atm-core` ownership of the full daemon client contract
- a versioned binary frame header and transport-scoped wire message id
- typed protocol models instead of `serde_json::Value` request bodies
- daemon-backed `send` and `ack`
- graft registration and daemon-originated nudge delivery
- daemon-owned pending-nudge drain support for hook-based consumers
- `[atm.graft]` config support

The practical result is that `atm-graft` should now be planned as a follow-on
integration line on top of Q.5, not as a crate that can be started directly
against the current code.

## 2. Findings

| Severity | Finding | Affected files/modules | Proposed fix | Plan impact |
|---|---|---|---|---|
| Blocking | Same-host client protocol ownership is still split the wrong way. `atm-daemon` owns client control-state, endpoint, wire-envelope, and frame-codec types that `atm-graft` would need. | `crates/atm-daemon/src/client.rs`, `crates/atm-daemon/src/lib.rs`, `crates/atm-core/src/dispatcher/mod.rs` | Move typed protocol, control-state, endpoint, and frame-envelope ownership into `atm-core`; keep concrete socket runtime code outside `atm-core`. | `GRAFT-1` |
| Blocking | The current newline-delimited transport frame is too weak for Q.6 completion and future graft/session work. There is no versioned binary header and no transport-scoped correlation id distinct from ATM mail ids. | `crates/atm-daemon/src/client.rs`, `crates/atm-daemon/src/lib.rs` | Replace newline framing with a binary `FrameHeader` carrying `protocol_version`, `WireMessageId`, and `payload_length`, all owned by `atm-core`. | `GRAFT-1`, `Q.6` |
| Blocking | The daemon protocol is still unary request/response only. There is no registration/session path and no daemon-to-client nudge stream. | `crates/atm-daemon/src/client.rs`, `crates/atm-daemon/src/lib.rs` | Add a long-lived graft-session protocol and typed daemon-originated `NudgeEvent` models owned by `atm-core`. | `GRAFT-3` |
| Important | There is no daemon-owned pending-nudge drain API for hook-based consumers, so unmodified hosts cannot fetch insertion-ready nudge text through a stable ATM command path. | `crates/atm-daemon/src/lib.rs`, `crates/atm/src/commands/*` | Add daemon-owned bounded pending-nudge queueing plus typed `DrainNudgesRequest` / `DrainNudgesResponse`, then expose a hook-facing `atm` command on top of that API. | `GRAFT-2`, `GRAFT-3` |
| Blocking | `send` and `ack` are not daemon-backed in the retained CLI, so `atm-graft` would otherwise target a different production path than `atm`. | `crates/atm/src/commands/send.rs`, `crates/atm/src/commands/ack.rs`, `crates/atm-daemon/src/lib.rs` | Complete daemon handlers for `send`/`ack` and converge the retained CLI on the same client contract before `atm-graft` lands. | `GRAFT-2` |
| Important | The existing `atm-core::dispatcher` envelope is only a precursor. `RequestPayload::{Send,Ack,Read,Clear,Heartbeat,Doctor}` still wrap raw `serde_json::Value`, which is too weak for a new crate boundary. | `crates/atm-core/src/dispatcher/mod.rs` | Replace `Value` payloads with typed request/response/event structs and keep any dynamic registry or handler indirection behind sealed traits. | `GRAFT-1` |
| Important | Q.5 has no `[atm.graft]` config surface, so activation/inert-mode behavior cannot be implemented in the documented way yet. | `crates/atm-core/src/config/types.rs`, `crates/atm-core/src/config/mod.rs` | Add minimal `[atm.graft]` support in `atm-core` with `enabled = true|false`; keep endpoint overrides deferred unless proven necessary. | `GRAFT-4` |
| Important | Workflow sidecar state is still transitional compatibility machinery, but the module docs still present it as ATM-owned mailbox durability truth. | `docs/atm-core/modules/workflow.md`, `crates/atm-core/src/workflow.rs`, `crates/atm-core/src/inbox_ingress/mod.rs` | Reword the docs to describe workflow state as transitional compatibility only; keep `atm-graft` entirely on daemon-backed read truth. | `GRAFT-4` |

## 3. Positive Foundations

The current code already gives the `atm-graft` line useful foundations:

- `MailStore`, `TaskStore`, and `RosterStore` are already clear boundaries in
  `atm-core`
- semantic wrappers such as `MessageKey`, `RecipientPaneId`, `ProcessId`, and
  timeout/budget types are already present in the store layer
- SQLite schema coverage in `atm-rusqlite` is broad enough that `atm-graft`
  should not need new durable tables for v1
- roster ingestion via `config.json` already routes through `team_ingress`

These pieces mean the remaining work is mainly protocol extraction, daemon API
completion, and host integration, not a fresh data-model design.

## 4. Rust Boundary Notes

This review specifically triggers:
- `RBP-001` Error Context + Recovery
- `RBP-002` Typestate
- `RBP-003` Sealed Trait
- `RBP-004` Newtype / Zero-Cost Abstraction
- `RBP-008` Trait Object Safety

Most important implications:
- new public protocol types must be typed, not `Value`-shaped
- transport correlation ids need their own semantic wrapper type so they do not
  collapse into ATM mail `message_id` semantics
- daemon-originated event frames should use that same transport id/header
  contract rather than inventing a second framing shape
- new client/session traits need an explicit sealed/open decision
- registration/session failure paths need stable error identity and recovery
  guidance
- the `GraftSession` lifecycle should keep its state machine explicit even if
  full typestate is deferred
