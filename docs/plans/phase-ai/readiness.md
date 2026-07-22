---
title: Phase AI readiness
status: proposed
---

# Phase AI readiness

Phase AI is ready only when AI.1 through AI.10 are merged in order to
`integrate/phase-AI`, all architecture checks pass at that tip, and the proof
matrix below has durable evidence.

| Proof | Required result |
| --- | --- |
| Unix local | HTTP/UDS send, read, ack, and nudge succeed; `agent`, `agent:chat-a`, and `agent:chat-b` stay independent |
| Windows local | HTTP over loopback TCP send, read, ack, and nudge succeed; no alternate local transport is used |
| Self address | The daemon's configured host/IP uses HTTPS and the ordinary router; full chat-qualified reply routing is retained |
| Two Mac hosts | Bidirectional HTTPS send and ack, mTLS authenticated, exact peer trust enforced, chat-qualified addresses retained |
| Windows peer | Windows participates in bidirectional HTTPS send and ack with chat-qualified identity retained |
| Negative paths | Untrusted certificate, non-allowlisted peer, unavailable peer, duplicate ULID, and failed remote ack preserve correct shared-handler behavior |
| Regression | `just lint`, `just test`, and all Phase AI architecture checks pass |

No result may claim cross-host closure from raw TCP reachability or a
loopback-only mode.
