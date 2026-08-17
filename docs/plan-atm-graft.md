# ATM-Graft Implementation Plan

## 1. Purpose

This document turns the `atm-graft` requirements and architecture into an
implementation-targeted plan aligned to the current Phase U restack direction
and current daemon/runtime baseline rather than the older abandoned planning
line.

Planning baseline:
- `integrate/phase-T @ 75d341b`

## 2. Non-Negotiable Boundary Rules

- `atm-graft` must not depend on `atm-daemon` as a Rust crate
- this must be enforced by lint/boundary configuration rather than convention
- `atm-graft` must not depend on `atm-rusqlite`
- direct SQLite or inbox JSONL access is out of scope for `atm-graft`
- all protocol structs, enums, and traits needed by `atm-graft` must live in
  `atm-core`
- the concrete daemon peer remains `atm-daemon`
- the host executable owns the final between-tool-call injection point
- pending nudge durability/queue ownership belongs in the daemon rather than
  inside `atm-graft`

## 3. Current Baseline

Useful generic implementation already present:
- durable mail/task/roster store contracts in `atm-core`
- mature SQLite-backed record families in `atm-rusqlite`
- same-host daemon runtime with singleton control and bounded shutdown
- real same-host request handling as a product path, not a future placeholder
- retained CLI paths already proving the daemon/client integration shape

Reusable generic current-develop building blocks:
- shared `ClientTransport` boundary in `crates/atm-core/src/boundary/mod.rs`
- shared request/response envelope family in `crates/atm-core/src/protocol.rs`
- shared protocol codec `JsonAtmProtocolCodec` in
  `crates/atm-core/src/protocol.rs`
- reusable client test seams:
  - `FakeClientTransport` in `crates/atm-core/src/transport/testing.rs`
  - `LoopbackClientTransport` in `crates/atm-core/src/transport/testing.rs`
- reusable daemon-side integration test seams:
  - `DoctorOnlyDispatcher` in `crates/atm-daemon/src/test_support.rs`
  - daemon local IPC transport tests in
    `crates/atm-daemon/src/local_ipc_transport.rs`
- current shared ICD inventory in `docs/atm-daemon/protocol-icd.md`

Current `develop @ b6506ef` graft-specific discovery references:
- `crates/atm-core/src/graft.rs`
- `crates/atm-core/src/protocol.rs`
- `crates/atm-daemon/src/advisory_runtime.rs`
- `crates/atm-daemon/src/tests.rs`
- `crates/atm-graft/src/lib.rs`
- `crates/atm/src/composition.rs`
- `crates/atm/src/commands/graft.rs`

Historical/use rule:
- reuse DTO shapes, queue semantics, host-injection seams, and focused tests
  where they fit the new boundaries
- do not preserve the current `Graft*` daemon naming or the current poll/drain
  receive loop as the production design

Remaining gaps relative to the target `atm-graft` design:
- no generic consumer registration or daemon-to-client advisory stream exists
  yet
- no daemon-owned generic pending-advisory queue exists yet
- no `[atm.graft]` config surface exists in `atm-core`
- no thin embedded crate packages the existing daemon client behavior for host
  agents
- the public client-facing `atm-core` surface still needs a dedicated
  embeddable shape rather than exposing only CLI-oriented composition
- the current embedded runtime uses poll/drain instead of one dedicated live
  advisory-stream connection

Planning consequence:
- `atm-graft` is no longer a large protocol bootstrap effort
- it is now a thin follow-on line on top of the current IPC/runtime baseline
- the work should therefore live as three additive Phase U sprints
- all new thin-client work should start from the current shared protocol and
  transport seams already present on `develop`, without reintroducing
  graft-private packet families

## 4. Gap Analysis

### G.1 Embeddable client-surface gap

Current state:
- same-host daemon IPC already exists
- `atm` already proves the retained daemon-client path
- `U.8` lands one small explicit thin-client surface for embedded host-agent
  consumers: shared unary `send` / `read` / `ack` over `ClientTransport`

Required change:
- keep concrete runtime/socket behavior out of the public `atm-core` surface
- extend the shared client-side contract only where `U.9` / `U.10` prove that
  session/advisory behavior needs additive shared DTOs
- keep the protocol/interface docs aligned with that shared client boundary

### G.2 Session/nudge runtime gap

Current state:
- the daemon is already the correct owner of runtime coordination
- there is no graft registration / unregistration path
- there is no daemon-owned bounded pending-nudge queue or drain request
- there is no persistent client-side session thread holding one dedicated live
  daemon advisory-stream socket while the host is idle

Required change:
- add graft registration / unregistration handlers
- add daemon-owned bounded nudge queueing
- add one dedicated daemon advisory stream per active session as the
  production delivery path
- keep nudge fetch/drain only as companion CLI/debug support if still needed
- add one persistent embedded-session receive thread per active
  `GraftSession`; that thread must hold the live daemon socket connection used
  for advisory nudge delivery
- keep queue ownership and backpressure behavior entirely daemon-side
- require the client runtime to queue newly received nudge payloads until the
  host consumes them and to fire a host wake/event signal when a new nudge
  arrives so inactive hosts are forced to resume attention
- update the protocol/interface docs for registration, drain/fetch, and daemon
  event payloads

### G.3 Thin crate gap

Current state:
- `U.8` lands the `atm-graft` crate as a thin unary client over the shared
  ATM transport contract
- host binaries still do not have the `U.9` session runtime or the `U.10`
  generic advisory stream yet

Required change:
- add minimal `[atm.graft]` activation
- add `GraftSession`
- replace the current poll/drain runtime with host-facing automatic advisory
  delivery plus a minimal pending queue and wake/event callback
- add the concrete thin crate surfaces:
  - `GraftClient`
  - `GraftSession`
  - `HostNudgeInjector`
  - `GraftObservability`

## 5. Phase U Work Packages

### U.8: Shared Thin-Client ICD For CLI And Graft

Implementation scope:
- `atm-core`
- `atm`
- `atm-daemon`

Deliverables:
- shared unary `atm-core` client/request/response models used by embedded
  consumers
- explicit `atm-core` ownership of the generic thin-client naming line,
  including `AdvisorySessionId`
- `atm` CLI continues to use the same shared ICD family rather than a separate
  client-specific protocol line
- no `atm-daemon` crate dependency required for external graft consumers

### U.9: Client-Owned Graft Runtime

Implementation scope:
- `atm-graft`
- `atm-core`

Deliverables:
- `GraftSession` as the concrete lifecycle runtime
- one persistent receive thread per active session holding the open daemon
  socket used for advisory nudge delivery
- host-facing queueing plus wake/event signaling so nudges are retained until
  the embedding host consumes them even when the host loop is inactive
- reconnect and shutdown behavior owned by `atm-graft`

### U.10: Generic Daemon Advisory-Notification Surface

Implementation scope:
- `atm-daemon`
- `atm-core`
- `atm`

Deliverables:
- generic consumer registration / unregistration protocol
- daemon-owned bounded pending advisory-nudge queue
- daemon-owned persistent advisory stream using the same shared ICD family as
  CLI/thin clients
- optional daemon-owned generic fetch/drain API for companion CLI/debug
  behavior
- typed backpressure and queue-overflow behavior
- hook-facing `atm` command surface for nudge drain on the same daemon API
- daemon-side support consumed by the automatic embedded-session
  nudge receive/injection path implemented by `atm-graft`

Sequencing rule:
- `U.8` must land first because it defines the shared thin-client contract
- `U.9` must land second because the plugin-owned receive/injection runtime
  must be explicit before daemon notification semantics are finalized
- `U.10` closes the line by adding the generic daemon advisory-notification
  surface consumed by the thin client

## 6. Simplifications For V1

To keep the first implementation tractable:
- do not add direct SQLite reads
- do not add daemon-private business logic to `atm-graft`
- do not broaden config beyond `[atm.graft].enabled`
- do not make heartbeat/activity reporting part of the initial must-have
  surface unless host integration needs it immediately
- do not turn nudge payloads into a large schema before the host proves the
  need; `from` and `message` are the minimum
- keep the embedded crate small; for the most part this is simply an ATM client
  embedded in an agent process
- do not treat manual poll-only behavior as sufficient for embedded mode;
  automatic context injection is the point of `atm-graft`
- do not treat `tmux send-keys` or related terminal automation as a
  production-ready integration mechanism

## 7. Integration Modes

Embedded session mode:
- a custom host CLI links `atm-graft` in-process
- `GraftSession` registers with the daemon through the `U.10` shared
  registration/notification surface
  protocol
- nudges are delivered through the daemon session path
- one persistent receive task/thread runs for the active session inside
  `atm-graft` and keeps the daemon socket connection open for nudges
- received nudges are queued for host consumption inside the client runtime and
  trigger a host wake/event callback so inactive hosts take action promptly
- the host receives automatic between-tool-call injection through the graft
  bridge once it resumes at the next safe insertion point

Non-production companion path:
- a CLI fetch/drain command may still exist for debugging, migration, or
  non-embedded environments
- that path is explicitly not the production completion target for
  `atm-graft`

Architectural consequence:
- the queue must live in the daemon
- `atm-graft` becomes one consumer path, not the owner or namesake of queued
  nudge state
- external terminal automation is not part of the production acceptance path

## 8. Phase U Dependency

`atm-graft` now depends on the Phase U restack shape, not on a replay of the
older abandoned wire-protocol plan.

Required upstream outcomes before `atm-graft` implementation closes:
- `U.1` through `U.7` tighten the shared ATM boundaries, identity, and SQLite
  read/state model
- `U.8` lands the shared thin-client ICD surface
- `U.9` lands the plugin-owned client runtime with the persistent receive
  thread, host wake/event path, and automatic injection bridge
- `U.10` lands registration and daemon-owned generic advisory queue/drain or
  stream behavior

Required closeout checks across `U.8`-`U.10`:
- every sprint doc includes explicit deliverables, dependencies, acceptance
  criteria, QA pointers, and required validation
- `docs/atm-graft/requirements.md` and `docs/atm-graft/architecture.md`
  include concrete verification anchors for `req-qa` and `arch-qa`
