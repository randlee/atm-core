# ATM Daemon Protocol ICD

## 1. Purpose

This document is the canonical interface control document for ATM daemon
request/response transport.

It defines:
- the shared ATM frame header
- the ATM packet family
- request/response exchange rules
- transport failure semantics
- the reuse rules for same-host local IPC and cross-host daemon traffic

This ICD applies to:
- ATM CLI to daemon same-host local IPC
- other same-host daemon clients such as harnesses or thin extensions
- daemon-to-daemon remote transport
- shared transport-functional test harnesses

This ICD does not define:
- CLI argument parsing
- daemon lifecycle-control events
- transport-specific access control internals
- business-logic handler semantics beyond the packet contracts they expose

This ICD also does not claim that every retained ATM command is a daemon
packet. The retained product surface is broader than the current daemon
request/response packet family.

## 2. Governing Requirements

- `REQ-P-RUNTIME-001`
- `REQ-P-PLATFORM-001`
- `REQ-P-PLATFORM-002`
- `REQ-CORE-TRANSPORT-001`
- `REQ-CORE-TRANSPORT-002`
- `REQ-CORE-TRANSPORT-003`
- `REQ-CORE-TRANSPORT-004`
- `REQ-CORE-TRANSPORT-006`
- `REQ-DAEMON-TRANSPORT-001`
- `REQ-DAEMON-TRANSPORT-002`
- `REQ-DAEMON-TRANSPORT-003`
- `REQ-DAEMON-TRANSPORT-005`
- `REQ-DAEMON-TRANSPORT-006`
- `REQ-DAEMON-TRANSPORT-007`

## 3. Scope And Non-Scope

The ATM daemon protocol is one shared request/response message system for:
- same-host daemon access
- cross-host daemon access

The protocol must not fork into:
- one local-only message format
- one remote-only message format

Transport adapters may differ in:
- connect/listen mechanics
- access control
- lifecycle ownership integration
- bounded retry policy

They must not differ in:
- ATM frame header shape
- ATM packet kind registry
- typed payload DTO families
- protocol failure meaning

UDP is not an accepted transport for ATM daemon request/response messaging in
the retained product surface.

## 3.1 Phase S Supported Packet Surface

The current Phase S daemon packet family is intentionally smaller than the full
retained CLI command surface.

Daemon packet families that this ICD must cover:
- send compose
- send acknowledge
- receive
- clear
- doctor
- heartbeat

Retained product workflows that are not daemon request/response packets in the
current Phase S line:
- `atm log`
  - uses the shared observability boundary directly
- `atm teams`
  - uses team-admin/config/store surfaces directly
- `atm members`
  - uses team-admin/config/store surfaces directly

Rule:
- the ICD must fully specify the daemon packet families that exist today
- it must not imply that `log`, `teams`, or `members` are daemon packets when
  the current implementation does not route them through the daemon protocol

## 3.2 Phase T Graft Semantic Surface

Sprint T.6 introduces the typed graft-facing semantic DTO family in
`atm-core` without yet adding new daemon packet kinds.

The new semantic surface covers:
- graft registration
- graft unregistration
- pending-nudge fetch
- pending-nudge drain
- daemon-originated graft nudge payloads

Rules:
- these DTOs live in `atm-core`, not in `atm-daemon`
- embedded consumers must not invent alternate raw payload shapes outside this
  semantic family
- concrete packet-kind allocation for the graft runtime is deferred to Sprint
  T.7 when the daemon-side registration and nudge queue runtime lands

## 4. Transport Model

ATM daemon transport is a framed, connection-oriented, request/response
protocol.

Phase S baseline transport shape:
- one logical request
- one logical response
- one connection

Phase S does not require framed multiplexing.

Implications:
- `request_id` is still mandatory even in one-request-per-connection mode
- later multiplexing may reuse the same packet family without redesigning the
  header
- current implementations may close the connection after one response without
  violating the protocol

Transport reliability rule:
- the transport can only prove request acceptance when the sender receives a
  valid ATM response packet
- transport delivery is not treated as business-operation completion by itself

## 5. Shared ATM Frame

Every ATM packet uses this shape:

1. fixed header
2. payload bytes

The frame contract is shared across:
- same-host local IPC
- cross-host TCP/TLS

EOF or stream half-close is not the packet boundary contract.

### 5.1 Fixed Header Fields

Header fields in order:
- `magic`
- `version`
- `message_kind`
- `flags`
- `request_id`
- `payload_length`

Then:
- `payload_bytes`

### 5.2 Field Roles

- `magic`
  - identifies ATM traffic early
  - rejects foreign/garbage input before payload work begins
- `version`
  - gates incompatible wire changes explicitly
- `message_kind`
  - selects the expected payload DTO family before decode
- `flags`
  - reserves additive protocol behavior without creating a second header
  - must be zero unless the version defines non-zero semantics
- `request_id`
  - correlates request/response pairs
  - must remain stable across the lifetime of one logical exchange
- `payload_length`
  - explicitly delimits the payload
  - must remain within the ATM frame size cap

### 5.3 Fixed Header Encoding

The Phase S header uses network byte order (big-endian) for every integer
field.

Fixed widths:
- `magic: u32`
- `version: u16`
- `message_kind: u16`
- `flags: u16`
- `request_id: u64`
- `payload_length: u32`

The fixed header is exactly `22` bytes:

| Offset | Field | Width | Value / rule |
|---|---|---:|---|
| `0` | `magic` | `4` | ASCII `ATMD` = `0x41 0x54 0x4d 0x44` |
| `4` | `version` | `2` | `0x0001` for the Phase S.0 contract |
| `6` | `message_kind` | `2` | one registry value from Section 6 |
| `8` | `flags` | `2` | `0x0000` in protocol version `0x0001` |
| `10` | `request_id` | `8` | caller-chosen non-zero correlation id |
| `18` | `payload_length` | `4` | encoded payload byte count |

`request_id` rules:
- request senders must choose a non-zero `request_id`
- response senders must echo the request `request_id` unchanged
- callers may allocate request ids by monotonic counter or random `u64`, but
  they must avoid live-connection collisions within one process

### 5.4 Framing Rules

Required receiver behavior:
1. read the fixed header first
2. validate `magic`
3. validate required `version`
4. validate `flags` for the selected version
5. validate `message_kind` against the registry in Section 6
6. validate `payload_length <= MAX_DAEMON_FRAME_BYTES`
7. read exactly `payload_length` bytes
8. decode payload according to `message_kind`

Required sender behavior:
1. construct a valid typed payload
2. serialize payload bytes
3. compute `payload_length`
4. emit exactly one header followed by exactly one payload body

## 6. Packet Kind Registry

The protocol is packet-kind-based. Receivers switch on `message_kind` before
payload decode.

Phase S packet families:

### 6.1 Request Packet Kinds

- `0x0001` `send_compose_request`
- `0x0002` `send_acknowledge_request`
- `0x0003` `heartbeat_request`
- `0x0004` `list_request`
- `0x0005` `receive_request`
- `0x0006` `clear_request`
- `0x0007` `doctor_request`

### 6.2 Success Response Packet Kinds

- `0x1001` `send_sent_response`
- `0x1002` `send_acknowledged_response`
- `0x1003` `heartbeat_response`
- `0x1004` `list_response`
- `0x1005` `receive_response`
- `0x1006` `clear_response`
- `0x1007` `doctor_response`

### 6.3 Error Packet Kind

- `0x1fff` `error_response`

Error responses are ATM protocol packets, not out-of-band transport exceptions.

### 6.4 Registry Rule

- `message_kind` values are part of the protocol contract
- same-host and cross-host transports must use the same registry
- adding a new request family requires a new packet kind and a documented
  payload DTO
- changing the meaning of an existing packet kind incompatibly requires a
  versioned protocol update

### 6.5 Packet-Kind To Workflow Mapping

| Kind value | Packet kind | Current source workflow | Notes |
|---|---|---|---|
| `0x0001` | `send_compose_request` | `atm send` | retained send workflow over daemon transport |
| `0x0002` | `send_acknowledge_request` | `atm ack` | retained ack workflow is send-shaped, not a separate top-level ack packet family |
| `0x0003` | `heartbeat_request` | daemon/runtime heartbeat path | not a retained user CLI command; runtime/member liveness path |
| `0x0004` | `list_request` | `atm list` | bounded metadata queue query workflow |
| `0x0005` | `receive_request` | `atm read` | retained single-message read workflow |
| `0x0006` | `clear_request` | `atm clear` | retained clear workflow |
| `0x0007` | `doctor_request` | `atm doctor` | retained doctor runtime query surface |
| `0x1001` | `send_sent_response` | response to `atm send` | success response |
| `0x1002` | `send_acknowledged_response` | response to `atm ack` | success response |
| `0x1003` | `heartbeat_response` | response to heartbeat | success response |
| `0x1004` | `list_response` | response to `atm list` | success response |
| `0x1005` | `receive_response` | response to `atm read` | success response |
| `0x1006` | `clear_response` | response to `atm clear` | success response |
| `0x1007` | `doctor_response` | response to `atm doctor` | success response |
| `0x1fff` | `error_response` | typed service failure | may answer any request kind |

Current non-packet retained workflows:
- `atm log`
- `atm teams`
- `atm members`

## 7. Payload DTO Contract

Payload bytes are interpreted according to `message_kind`.

Payload ownership:
- packet DTOs are owned by the shared ATM protocol layer
- same-host and cross-host daemon transport must not fork separate DTO
  families

Phase S payload encoding direction:
- protocol version `0x0001` uses UTF-8 JSON payload bytes produced and
  consumed by the shared serde-based DTO layer
- payload JSON is wrapped by the ATM frame header
- frame structure is independent of payload serialization choice for a future
  protocol version

Current request/response payload ownership maps to the shared envelope family
and the concrete Rust DTO types in `crates/atm-core/src/protocol.rs`.

Field-authority rule:
- the Rust DTO definitions in `crates/atm-core/src/protocol.rs` are the field-
  level source of truth for packet payload contents in protocol version
  `0x0001`
- adding, removing, or renaming a public packet field requires an ICD update,
  matching requirements/architecture updates, and a compatibility review

### 7.1 Current Packet Payload Types

| Packet kind | Payload Rust type | Current envelope path |
|---|---|---|
| `send_compose_request` | `SendRequest` | `RequestEnvelope::Send(SendRequestEnvelope::Compose(...))` |
| `send_acknowledge_request` | `AckRequest` | `RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(...))` |
| `heartbeat_request` | `TeamMemberHeartbeatRequest` | `RequestEnvelope::Heartbeat(...)` |
| `list_request` | `ListQuery` | `RequestEnvelope::List(...)` |
| `receive_request` | `ReadQuery` | `RequestEnvelope::Receive(...)` |
| `clear_request` | `ClearQuery` | `RequestEnvelope::Clear(...)` |
| `doctor_request` | `DoctorQuery` | `RequestEnvelope::Doctor(...)` |
| `send_sent_response` | `SendOutcome` | `ResponseEnvelope::Send(SendResponseEnvelope::Sent(...))` |
| `send_acknowledged_response` | `AckOutcome` | `ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(...))` |
| `heartbeat_response` | `TeamMemberHeartbeatResponse` | `ResponseEnvelope::Heartbeat(...)` |
| `list_response` | `ListOutcome` | `ResponseEnvelope::List(...)` |
| `receive_response` | `ReadOutcome` | `ResponseEnvelope::Receive(...)` |
| `clear_response` | `ClearOutcome` | `ResponseEnvelope::Clear(...)` |
| `doctor_response` | `DoctorReport` | `ResponseEnvelope::Doctor(...)` |
| `error_response` | `ProtocolErrorEnvelope` | `ResponseEnvelope::Error(...)` |

### 7.1.1 DTO Definition References

The current packet payload DTO definitions live in:
- `crates/atm-core/src/protocol.rs`
  - `SendRequest`
  - `AckRequest`
  - `TeamMemberHeartbeatRequest`
  - `ReadQuery`
  - `ClearQuery`
  - `DoctorQuery`
  - `SendOutcome`
  - `AckOutcome`
  - `TeamMemberHeartbeatResponse`
  - `ReadOutcome`
  - `ClearOutcome`
  - `DoctorReport`
  - `ProtocolErrorEnvelope`

### 7.2 Current Shared Envelope Mapping

The current protocol-layer envelope mapping is:

- `RequestEnvelope::Send(SendRequestEnvelope::Compose(...))`
  - `send_compose_request`
- `RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(...))`
  - `send_acknowledge_request`
- `RequestEnvelope::Heartbeat(...)`
  - `heartbeat_request`
- `RequestEnvelope::List(...)`
  - `list_request`
- `RequestEnvelope::Receive(...)`
  - `receive_request`
- `RequestEnvelope::Clear(...)`
  - `clear_request`
- `RequestEnvelope::Doctor(...)`
  - `doctor_request`

- `ResponseEnvelope::Send(SendResponseEnvelope::Sent(...))`
  - `send_sent_response`
- `ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(...))`
  - `send_acknowledged_response`
- `ResponseEnvelope::Heartbeat(...)`
  - `heartbeat_response`
- `ResponseEnvelope::List(...)`
  - `list_response`
- `ResponseEnvelope::Receive(...)`
  - `receive_response`
- `ResponseEnvelope::Clear(...)`
  - `clear_response`
- `ResponseEnvelope::Doctor(...)`
  - `doctor_response`
- `ResponseEnvelope::Error(...)`
  - `error_response`

### 7.3 DTOs That Are Not Current Public Packet Kinds

The following protocol-layer types exist today but are not current public ATM
daemon packet kinds in the Phase S request/response registry:
- `FramePayload`
- `NotificationEvent`
- `RuntimeStatusSnapshot`
- `WatchSubscriptionRequest`
- `WatchEventBatch`
- `ReconcileRequest`
- `ReconcileResult`

Rule:
- these types must not be assigned caller-visible packet kinds silently
- if any becomes part of the public daemon request/response surface, this ICD,
  the requirements docs, and the sprint plan must be updated first

### 7.4 Message Kind Before Payload Rule

`message_kind` exists so the receiver can:
- switch packet family first
- choose the expected DTO
- reject unknown kinds before speculative payload parsing

This is a required protocol behavior, not an optimization detail.

## 8. Exchange Rules

### 8.1 Request/Response Pairing

Every request packet must receive exactly one response packet:
- success response of the expected family
- or `error_response`

`request_id` rules:
- client sets `request_id` on every request
- server echoes the same `request_id` on the corresponding response
- transport adapters must not rewrite `request_id`

Success-family pairing rules:
- `send_compose_request -> send_sent_response | error_response`
- `send_acknowledge_request -> send_acknowledged_response | error_response`
- `heartbeat_request -> heartbeat_response | error_response`
- `list_request -> list_response | error_response`
- `receive_request -> receive_response | error_response`
- `clear_request -> clear_response | error_response`
- `doctor_request -> doctor_response | error_response`

### 8.1.1 One-Request-Per-Connection Rule

Phase S.0 keeps one logical request and one logical response per connection.

Required behavior:
- after a transport has accepted one valid request frame, it must not accept a
  second request frame on that same connection in protocol version `0x0001`
- if the peer sends additional bytes after the first completed request/response
  exchange, the transport must close the connection and surface a typed
  protocol failure
- implementations must not silently treat a second request as pipelining or
  multiplexing

### 8.2 Same-Host Local IPC

Same-host local IPC must:
- use the shared ATM frame contract
- use the same request/response packet kinds as remote transport
- return typed ATM error responses through the same packet family

Same-host local IPC must not:
- invent a local-only header
- rely on EOF as packet delimiting
- rely on UDP datagrams for ATM request/response traffic

### 8.3 Cross-Host Daemon Transport

Cross-host daemon transport must:
- use the shared ATM frame contract above the chosen stream transport
- preserve the same packet kinds and DTO families
- preserve `request_id`
- preserve typed ATM error responses

Cross-host transport may add:
- connect deadlines
- bounded retry before request acceptance
- TLS/session/authentication state outside the packet payload

Cross-host transport must not add:
- a second daemon packet family
- different request or response DTO semantics

## 9. Resource Caps And Limits

Shared frame-size rule:
- `payload_length` must remain bounded by
  `MAX_DAEMON_FRAME_BYTES`

Fixed Phase S.0 assignment:
- `MAX_DAEMON_FRAME_BYTES = 1_048_576` bytes

Phase S baseline cap direction:
- max encoded request or response frame size remains 1 MiB unless a later
  requirement changes the cap

Protocol-level rule:
- the receiver validates the declared length before allocating or reading the
  body
- oversize frames are protocol failures, not partial-success events

Connection-model rule:
- current same-host and current peer implementations may remain
  one-request-per-connection
- any later multiplexing must preserve the same header contract and the
  current typed error semantics

## 10. Timeout And Failure Semantics

These conditions invalidate the connection:
- bad `magic`
- unsupported required `version`
- invalid non-zero flags for the selected version
- unknown required `message_kind`
- declared payload larger than the ATM frame cap
- timeout while reading the fixed header
- timeout while reading the declared payload
- EOF before the declared payload length is satisfied
- payload decode failure for the declared `message_kind`

Required runtime behavior on protocol failure:
- close the connection
- return a typed transport/protocol failure at the caller boundary
- do not continue parsing later bytes from the same stream
- do not scan for the next possible `magic` marker on the same connection

### 10.1 Frame I/O Deadline Budget

Phase S.0 deadline assignments are:
- same-host local IPC fixed-header read timeout: `1s`
- same-host local IPC payload read timeout: `2s`
- same-host local IPC response write timeout: `3s`
- cross-host fixed-header read timeout: `5s`
- cross-host payload read timeout: `5s`
- cross-host response write timeout: `5s`

Budget rules:
- the same-host header and payload sub-budgets together must fit within the
  same-host daemon request deadline documented in `docs/atm-daemon/architecture.md`
- remote adapters may retry connect attempts within their separate bounded
  retry budget, but one accepted frame read or write operation must still obey
  the per-leg `5s` deadline above

### 10.2 No Mid-Stream Resynchronization

Mid-stream resynchronization is explicitly forbidden.

Reason:
- partial or timed-out frame state is ambiguous
- scanning for later `magic` bytes adds parser ambiguity and recovery churn
- the protocol remains simpler and more reliable when one broken frame
  invalidates the connection

Recovery model:
- caller retries on a fresh connection

## 11. Error Semantics

There are two error classes:

1. transport/protocol failures
2. ATM service-level failures

### 11.1 Transport/Protocol Failures

Examples:
- cannot connect
- header timeout
- payload timeout
- bad `magic`
- decode failure
- oversize frame

These are surfaced through the transport boundary as typed `AtmError` failures.

### 11.2 Service-Level Failures

Examples:
- configuration invalid
- target not found
- daemon unavailable for a typed operational reason
- doctor/read/clear/send request rejected by service logic

These are returned as `error_response` packets carrying the typed ATM protocol
error DTO.

Rule:
- service failures that occur after a valid request packet is decoded must be
  represented as ATM error response packets whenever a response can still be
  sent reliably

Reliably sendable means:
- the transport has not already observed a read or write failure on the
  connection
- the request frame decoded successfully
- the adapter still owns a live writable response stream for that request
- the response can be emitted within the write deadline from Section 10.1

## 12. Delivery And Outcome Semantics

The protocol does not claim business-operation completion until a valid ATM
response packet is received.

That means:
- a successfully written request frame is not enough
- a connected local IPC session is not enough
- a connected remote TCP/TLS session is not enough

Known-success condition:
- caller receives a valid success response packet

Known-service-failure condition:
- caller receives a valid `error_response`

Unknown-outcome condition:
- transport fails after some or all request bytes were written but before a
  valid response packet is received

Remote host-host delivery may layer replay/outcome rules above this ICD, but
the packet contract remains the same.

## 13. Versioning Rules

`version` exists to make wire evolution explicit.

Rules:
- incompatible header or packet-kind changes require a new protocol version
- additive payload fields may use normal payload-schema compatibility rules
  when the active payload encoding supports them
- a receiver that does not support the required version must fail the
  connection rather than guessing compatibility
- protocol version `0x0001` does not define downgrade negotiation, mixed-
  version fallback, or best-effort compatibility probing
- a peer advertising any version other than `0x0001` must be rejected for the
  current Phase S.0 line

Version `0x0001` and the numeric packet-kind assignments in Section 6 are the
current source of truth. Any incompatible change requires an ICD update before
implementation acceptance.

## 14. Test And Reuse Rules

Shared transport-functional tests must prove:
- same ATM frame contract on Unix and Windows
- same packet-kind handling on same-host and remote transports
- same typed error behavior for frame failures

The shared transport code should remain easy to reuse outside ATM.

Required ownership split:
- `atm-core`
  - frame schema
  - packet-kind registry
  - payload DTOs
  - framed read/write helpers
- `atm-daemon`
  - local IPC server adapter
  - remote peer transport adapter
  - runtime integration, deadlines, drain behavior, ownership semantics
- `atm`
  - same-host local IPC client adapter using the shared frame helpers

Implementation guidance:
- same-host IPC code should live in a dedicated transport module tree rather
  than crate-root runtime code
- the shared framing layer must not depend on Unix socket types, Windows pipe
  types, or daemon runtime state

## 15. Deferred Items

S.0 resolves the current wire contract. Remaining follow-up items are limited
to future protocol evolution work:
- exact additive compatibility rules for future packet kinds and payload
  fields
- any future use of non-zero `flags`
- any future extension of the public packet registry beyond the current Phase S
  request/response surface
