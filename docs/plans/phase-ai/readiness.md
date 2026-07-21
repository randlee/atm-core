---
title: Phase AI readiness
status: blocked
---

# Phase AI readiness

Phase AI is ready only when AI.1 through AI.10 are merged in order to
`integrate/phase-AI`, all architecture checks pass at that tip, and the proof
matrix below has durable evidence.

| Proof | Required result |
| --- | --- |
| Unix local | HTTP/UDS send, read, ack, and nudge succeed; `agent`, `agent:chat-a`, and `agent:chat-b` stay independent |
| Windows local | The same HTTP/UDS flow succeeds with AF_UNIX; no named pipe is used |
| Self address | The daemon's configured host/IP uses HTTPS and the ordinary router; full chat-qualified reply routing is retained |
| Two Mac hosts | Bidirectional HTTPS send and ack, mTLS authenticated, exact peer trust enforced, chat-qualified addresses retained |
| Windows peer | Windows participates in bidirectional HTTPS send and ack with chat-qualified identity retained |
| Negative paths | Untrusted certificate, non-allowlisted peer, unavailable peer, duplicate ULID, and failed remote ack preserve correct shared-handler behavior |
| Regression | `just lint`, `just test`, and all Phase AI architecture checks pass |

No result may claim cross-host closure from raw TCP reachability or a
loopback-only mode.

## AI.10 accepted-tip evidence

Evidence commit: `f28577f8` on
`feature/pAI-s10-crosshost-proof-closeout`. The rows below distinguish
automated in-process transport proof from live-host release evidence; no row
substitutes TCP reachability for message delivery.

| ID | Command | Hosts / transport | Result | Evidence |
| --- | --- | --- | --- | --- |
| AI10-LOCAL-001 | `cargo test -p atm-daemon https_transport --lib` | one process; HTTP-over-TLS loopback listener | PASS: exact pinned mTLS request reaches `ApiRouter`; bad pin is rejected before routing | `crates/atm-daemon/src/https_transport.rs` tests |
| AI10-NEG-001 | same command | one process; two enabled HTTPS rows, second occupied | PASS: invalid enabled configuration returns before any listener is published | `invalid_enabled_interface_leaves_no_partial_listener` |
| AI10-SHUTDOWN-001 | same command | one process; HTTPS listener | PASS: accepted HTTPS request workers are retained and joined during listener shutdown; each request has the documented five-second I/O bound | `HttpsListenerSet::shutdown` |
| AI10-CONTRACT-001 | `cargo test -p agent-team-mail --test cli_surface --test openapi_surface` | local | PASS: live clap tree and parsed OpenAPI schema match additions-only checked-in baselines | `crates/atm/tests/*_surface*` |
| AI10-LOCAL-002 | `just lint && just test` | local UDS / unit and integration suite | PASS | accepted-tip run after all AI.10 changes |
| AI10-CHAT-001 | local / own-IP / two-host send-read-nudge-ack matrix | all transports | BLOCKED: current write envelope does not persist an authenticated source host, and `canonical_ack_write_request` constructs `host: None`; a remote ack cannot select HTTPS | `crates/atm-core/src/ack/mod.rs` |
| AI10-TWOMAC-001 | bidirectional HTTPS send/ack + nudge | two physical Macs | BLOCKED: no second Mac is available to this worktree, and AI10-CHAT-001 prevents a valid remote-ack proof | separate live-host execution required after routing fix |
| AI10-WINDOWS-001 | bidirectional HTTPS send/ack + nudge | macOS and Windows | BLOCKED: no Windows peer is available to this worktree, and AI10-CHAT-001 prevents a valid remote-ack proof | separate live-host execution required after routing fix |

### Release blockers

1. The authenticated inbound peer identity must become part of the canonical
   write/address data so the stored incoming message retains its source host.
   The ordinary acknowledgement write must then use that source host. This is
   one canonical write route, not a second ack transport path.
2. Execute the two-Mac and Windows rows on real daemon pairs only after the
   source-host/ack route is fixed. Capture receiver-visible message, nudge,
   duplicate-ULID idempotence, unavailable-peer, and failed-ack non-mutation
   evidence in this record.
