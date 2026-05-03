# ATM-Graft Implementation Plan

## 1. Purpose

This document turns the `atm-graft` requirements and architecture into an
implementation-targeted plan aligned to the current Q.5 codebase rather than
the older published `1.0.4` line.

Primary implementation review target:
- `/Users/randlee/Documents/github/atm-core-worktrees/feature/pQ-s5-lock-retirement`

## 2. Non-Negotiable Boundary Rules

- `atm-graft` must not depend on `atm-daemon` as a Rust crate
- `atm-graft` must not depend on `atm-rusqlite`
- direct SQLite or inbox JSONL access is out of scope for `atm-graft`
- all protocol structs, enums, and traits needed by `atm-graft` must live in
  `atm-core`
- the concrete daemon peer remains `atm-daemon`
- the host executable owns the final between-tool-call injection point
- pending nudge durability/queue ownership belongs in the daemon rather than
  inside `atm-graft`

## 3. Q.5 Snapshot

Useful implementation already present in Q.5:
- durable mail/task/roster store contracts in `atm-core`
- mature SQLite-backed record families in `atm-rusqlite`
- daemon-backed same-host request handling for:
  - `read`
  - `clear`
  - `doctor`
  - `heartbeat`
- daemon auto-start and same-host control-state publishing for retained CLI
  use

Important Q.5 gaps relative to `atm-graft`:
- same-host client control-state and wire-envelope models still live in
  `atm-daemon`
- `atm_core::dispatcher` still uses `serde_json::Value` payloads
- no graft registration or daemon-to-client nudge stream exists
- no daemon-owned pending-nudge queue or poll/drain API exists
- `send` and `ack` are still direct-store CLI paths rather than daemon-backed
  requests
- no `[atm.graft]` config surface exists in `atm-core`
- workflow sidecar compatibility state is still active during the transition
- the current transport still uses newline-delimited framing and does not yet
  have a versioned binary header or a transport-scoped wire message id

## 4. Gap Analysis

### G.1 `atm-core` protocol ownership gap

Current state:
- `atm-core` has a partial `dispatcher` envelope, but request payloads are raw
  `Value` and there are no typed graft-session or nudge-event models
- same-host client control-state and wire-envelope types are still defined in
  `atm-daemon`

Required change:
- introduce an `atm-core` daemon-API module that owns:
  - typed request structs
  - typed response structs
  - same-host endpoint/control-state types
  - wire envelope / frame codec helpers over generic `Read` / `Write`
  - small client-facing traits for unary request execution

Why this blocks `atm-graft`:
- without these surfaces, `atm-graft` would either depend on `atm-daemon` or
  re-mint parallel protocol models, both of which violate the target boundary

Concrete ownership target:
- `atm_core::daemon_api::types`
  - `WireProtocolVersion`
  - `WireMessageId`
  - `PayloadLength`
  - `MaxFramePayloadBytes`
  - `LocalEndpoint`
  - `ControlState`
  - `FrameHeader`
- `atm_core::daemon_api::request`
  - `DaemonRequestEnvelope`
  - `ReadRequest`
  - `ClearRequest`
  - `DoctorRequest`
  - `HeartbeatRequest`
- `atm_core::daemon_api::response`
  - `DaemonResponseEnvelope`
  - per-family typed response payloads
  - `WireFailure`
- `atm_core::daemon_api::codec`
  - `encode_frame`
  - `decode_frame`
  - `encode_header`
  - `decode_header`
  - binary-header parsing helpers
- `atm_core::daemon_api::client`
  - sealed request/client traits

Naming rule:
- `WireMessageId` is a transport-scoped correlation identifier
- it is not the ATM mail `message_id` / `metadata.atm.messageId`
- any docs that say “wire message_id” refer to `WireMessageId`

Recommended binary header layout:
- fixed-size `FrameHeader` encoded before every payload frame
- first implementation target: 22 bytes total
  - `protocol_version: u16` big-endian
  - `message_id: [u8; 16]`
  - `payload_length: u32` big-endian
- `WireProtocolVersion` wraps the `u16`
- `WireMessageId` wraps the `[u8; 16]` and should render to text in logs/debug
  output without sharing semantics with ATM mail ids
- `PayloadLength` wraps the `u32` and must be validated against one documented
  maximum frame size before payload allocation
- `MaxFramePayloadBytes` owns that documented payload cap; the Q.6 target cap
  is `16 MiB`

Header parsing rule:
- read exactly the fixed header first
- switch by `protocol_version`
- validate `payload_length` against `MaxFramePayloadBytes` before allocation
- read the payload body
- then decode the JSON payload

### G.2 Q.6 daemon/runtime implementation gap

Current state:
- the daemon only handles `read`, `clear`, `doctor`, and `heartbeat`
- the transport is newline-delimited, one request followed by one response,
  with no binary header
- shared same-host client control-state and frame helpers still live partly in
  `atm-daemon`

Required change:
- move shared same-host wire/control models into `atm-core`
- replace newline framing with the shared binary header owned by `atm-core`
- add header payload-length validation against `MaxFramePayloadBytes`
- keep request-family business logic and any future notifier/plugin runtime
  private inside `atm-daemon`

Why this blocks `atm-graft`:
- the crate cannot begin against a stable client boundary until the shared
  header and `atm-core` ownership move are complete

### G.3 post-Q daemon API convergence gap

Current state:
- `atm read`, `atm clear`, and `atm doctor` already use daemon requests
- `atm send` and `atm ack` still open `RusqliteStore` directly
- `read` and `clear` also retain direct file-backed fallback paths on daemon
  unavailability

Required change:
- migrate `atm send` and `atm ack` to the shared daemon client contract
- restructure fallback removal as a post-cutover gate once daemon parity is
  proven, not as a Q.6 precondition

Why this matters to `atm-graft`:
- these are important convergence items, but they are not part of Q.6
- they belong in a named post-Q sprint such as `Phase R`

### G.4 post-Q graft/runtime gap

Current state:
- there is no graft registration / unregistration protocol
- there is no daemon-originated nudge event stream
- there is no daemon-owned pending-nudge drain API for hook-based consumers

Required change:
- add graft registration / unregistration handlers
- add typed daemon-originated `NudgeEvent` delivery
- add daemon-owned bounded pending-nudge queueing
- add `DrainNudgesRequest` / `DrainNudgesResponse`
- expose a hook-facing `atm` command on top of that same daemon API

Why this matters:
- these are the core prerequisites for the actual embedded/hook integration
  modes, but they are outside the narrow Q.6 scope confirmed by the user and
  `team-lead`
- they belong in a named post-Q sprint such as `Phase R`

### G.5 config activation gap

Current state:
- `.atm.toml` loading supports `[atm]`, aliases, team members, and post-send
  hooks
- no `[atm.graft]` config model exists

Required change:
- extend `atm-core` config types and loader with a minimal `[atm.graft]`
  section:
  - `enabled = true|false`
- keep endpoint override out of scope unless implementation need proves it

Why this blocks `atm-graft`:
- inert-vs-active behavior is a product rule, not a host-private convention

### G.6 transitional workflow gap

Current state:
- SQLite is the durable Phase Q direction
- workflow sidecar state is still imported, projected, and updated as
  transitional compatibility machinery
- `docs/atm-core/modules/workflow.md` still overstates it as the ATM-owned
  source of truth

Required change:
- keep `atm-graft` off the workflow sidecar path entirely
- treat daemon-backed `read` as the only mail-truth read surface for graft
- update docs to describe workflow sidecar as transitional compatibility state
  rather than enduring durable truth

Why this matters:
- the embedded host-agent path should align to the post-lock-retirement model,
  not re-entrench file-sidecar semantics

## 5. Proposed Work Packages

### 5.1 Recommended Worktree Map

Unless implementation sequencing changes, the `atm-graft` line should use one
worktree per package under:
- `/Users/randlee/Documents/github/atm-core-worktrees/`

Recommended branch/worktree ownership:
- `GRAFT-1 / Q.6`
  - worktree: `feature/pQ-s6-wire-protocol`
  - branch: `feature/pQ-s6-wire-protocol`
  - scope: binary header, `WireMessageId`, `MaxFramePayloadBytes`,
    `LocalEndpoint`, `ControlState`, frame codec, and the `atm` tail/debug
    helper
- `GRAFT-2 / Phase R`
  - worktree: `feature/pR-graft-daemon-parity`
  - branch: `feature/pR-graft-daemon-parity`
  - scope: retained `send` / `ack` daemon parity, shared client-path cutover,
    and hook-facing `atm` drain command
- `GRAFT-3 / Phase R`
  - worktree: `feature/pR-graft-daemon-session`
  - branch: `feature/pR-graft-daemon-session`
  - scope: graft registration, daemon-originated nudge delivery, daemon-owned
    pending-nudge queueing, and drain API
- `GRAFT-4 / Phase R`
  - worktree: `feature/pR-atm-graft`
  - branch: `feature/pR-atm-graft`
  - scope: `atm-graft` crate, `GraftSession`, runtime adapters, and host
    integration

Sequencing rule:
- `GRAFT-1` lands first because it defines the shared protocol boundary
- `GRAFT-2` and `GRAFT-3` may proceed as separate worktrees after `GRAFT-1`,
  but `GRAFT-4` should not start until both have stabilized enough to keep the
  public graft API from thrashing
- if a docs-only follow-up is needed between implementation packages, it
  should stay on `plan/atm-graft` rather than reuse one of the feature
  worktrees above

### GRAFT-1 / Q.6: extract wire/control ownership into `atm-core`

Owning crates:
- `atm-core`
- `atm-daemon`

Deliverables:
- typed endpoint/control-state models in `atm-core`
- shared frame codec helpers in `atm-core`
- `FrameHeader` with:
  - `protocol_version: WireProtocolVersion`
  - `message_id: WireMessageId`
  - `payload_length: PayloadLength`
- `MaxFramePayloadBytes` validation gate
- `atm_core::daemon_api` ownership for `LocalEndpoint` and `ControlState`
- tail/debug helper owned by the `atm` crate command surface

Primary risks:
- accidental leakage of daemon-private runtime details into `atm-core`
- transport correlation ids being confused with ATM mail ids unless the wrapper
  types and docs stay explicit

### GRAFT-2 / Phase R: finish daemon API parity for retained operations

Owning crates:
- `atm-daemon`
- `atm-core`
- `atm`

Deliverables:
- daemon handlers for `send` and `ack`
- shared client path used by `atm` for `send`, `read`, `ack`, `clear`, and
  `doctor`
- hook-facing `atm` command that renders insertion-ready nudge text from the
  daemon drain API
- fallback-removal gate after daemon parity proves stable
- same-host and remote runtime paths using the `atm-core` binary-header codec

Primary risks:
- changing CLI production paths before parity tests are ready
- introducing new daemon failure codes without matching recovery guidance

### GRAFT-3 / Phase R: add daemon-side graft registration and nudge delivery

Owning crates:
- `atm-daemon`
- `atm-core`

Deliverables:
- graft registration / unregistration protocol
- daemon-owned `post-send-event` to notifier fanout
- daemon-originated `NudgeEvent` payload with at least `from` and `message`
- daemon-owned bounded pending-nudge queue keyed by identity/session target
- `DrainNudgesRequest` / `DrainNudgesResponse` for poll-based consumers
- bounded daemon/session backpressure behavior with typed failures
- daemon event envelopes correlated by the same `WireMessageId` header contract
  used for other daemon frames

Primary risks:
- unclear session ownership when `ATM_IDENTITY` conflicts with an existing pid
- unbounded queue growth between daemon and host

### GRAFT-4 / Phase R: implement `atm-graft`

Owning crates:
- `atm-graft`
- embedding host executable

Deliverables:
- `.atm.toml` discovery and inert-mode behavior
- default-on `GraftSession`
- client-side drain/fetch support over the daemon API
- public API limited to:
  - `send`
  - `read`
  - `ack`
  - session lifecycle
  - nudge draining / fetch
- `tokio` adapter as the first convenience runtime

Non-deliverable note:
- `atm-graft` does not own a standalone end-user CLI command surface
- hook-facing commands belong to the `atm` crate and consume the same daemon
  API contract

Primary risks:
- host runtime integration assumptions leaking Tokio into the core crate
- queue/drain behavior becoming more complex than the actual host needs

## 6. Simplifications For V1

To keep the first implementation tractable:
- do not add direct SQLite reads
- do not add daemon-private logic to `atm-graft`
- do not broaden config beyond `[atm.graft].enabled`
- do not make heartbeat/activity reporting part of the initial must-have
  surface unless host integration needs it immediately
- do not turn nudge payloads into a large schema before the host proves the
  need; `from` and `message` are the minimum
- keep payload bodies JSON-backed for the first header migration; the scope is
  header hardening and ownership cleanup, not a full binary payload protocol

## 6.1 Integration Modes

The current design now supports two host-integration modes.

Embedded session mode:
- a modified host binary links `atm-graft`
- `GraftSession` registers with the daemon
- nudges are delivered through the daemon session path
- the host drains fetched nudges between tool calls

Hook/poll mode:
- the host binary is not modified for direct embedding
- a post-tool-use hook invokes an ATM command to fetch pending nudge text
- the daemon returns pending nudges for the current `ATM_IDENTITY`
- the command emits insertion-ready text and clears or advances the daemon queue

Architectural consequence:
- the queue must live in the daemon
- `atm-graft` becomes one consumer path, not the owner of queued nudge state

Recommended command shape:
- first implementation should use an `atm` CLI command, not a new standalone
  `atm-graft` binary, because the hook can invoke the already-installed ATM
  executable
- candidate surface:
  - `atm graft nudge --drain`
  - or another clearly named `atm` subcommand that returns insertion-ready text
- the command should call the same `DrainNudgesRequest` daemon API used by
  embedded consumers

Command output rule:
- default output should be plain text suitable for direct context insertion
- optional structured output may be added later for debugging or richer hosts
- when no nudge is pending, the command should exit successfully with empty
  output so post-tool-use hooks stay simple

Backpressure rule:
- the daemon queue must be bounded
- drain semantics must be explicit:
  - drain one
  - or drain all pending nudges into one rendered text block
- silent drop is not acceptable without structured observability

## 7. Q.6 Dependency

`atm-graft` now depends on the hardened Q.6 Phase Q shape.

Required upstream Q.6 outcomes before `atm-graft` implementation starts:
- daemon framing uses the shared binary header
- `WireMessageId` is present and documented as transport-only
- `LocalEndpoint` and `ControlState` have moved into `atm_core::daemon_api`
- shared wire/control/frame types have moved into `atm-core`
- the protocol tail/debug helper exists for operator inspection
- maximum payload validation is documented and enforced through one ATM-owned
  limit type

Q.7 / Q.8 note:
- release-gate planning and release execution are not blockers for beginning
  `atm-graft` implementation once the Q.6 protocol and ownership work is
  complete

Post-Q note:
- daemon-backed `send`/`ack`, graft registration, daemon-originated nudge
  delivery, and hook-facing nudge drain commands are not Q.6 prerequisites
- they belong to the post-Q implementation sprint currently named `Phase R`

Recommended Q.6 helper location:
- `atm` crate debug/ops surface, not `atm-graft`
- helper ownership candidate:
  - `crates/atm/src/commands/transport_tail.rs`
  - or another clearly named debug command module in `atm`
- helper behavior:
  - read a captured frame stream or socket dump
  - decode the fixed binary header
  - print header fields
  - print the decoded JSON payload body

Recommended hook command location:
- `atm` crate command surface, not `atm-graft`
- helper ownership candidate:
  - `crates/atm/src/commands/graft_nudge.rs`
  - or another clearly named hook-oriented command module in `atm`
- helper behavior:
  - resolve current `ATM_IDENTITY` / team context
  - issue `DrainNudgesRequest`
  - render insertion-ready text
  - exit cleanly when no pending nudge exists

## 8. Re-Reviewed Workflow Finding

The original workflow-sidecar review finding still holds, but the accurate
rephrasing against Q.5 is narrower:

- workflow sidecar state is no longer the intended durable truth for Phase Q
  mail correctness
- it is still active compatibility machinery during Q.5 because inbox ingress
  imports it into SQLite and some command paths still project/update it
- `atm-graft` must therefore avoid any dependency on workflow-sidecar state and
  wait for daemon-backed mail truth only
