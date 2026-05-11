# ATM-Graft Implementation Plan

## 1. Purpose

This document turns the `atm-graft` requirements and architecture into an
implementation-targeted plan aligned to the current Phase T daemon/runtime
baseline rather than the older Phase Q planning line.

Planning baseline:
- `integrate/phase-T @ 75d341b`

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

## 3. Current Baseline

Useful implementation already present:
- durable mail/task/roster store contracts in `atm-core`
- mature SQLite-backed record families in `atm-rusqlite`
- same-host daemon runtime with singleton control and bounded shutdown
- real same-host request handling as a product path, not a future placeholder
- retained CLI paths already proving the daemon/client integration shape

Remaining gaps relative to `atm-graft`:
- no graft registration or daemon-to-client nudge stream exists yet
- no daemon-owned pending-nudge queue or drain API exists yet
- no `[atm.graft]` config surface exists in `atm-core`
- no thin embedded crate packages the existing daemon client behavior for host
  agents
- the public client-facing `atm-core` surface still needs a dedicated
  embeddable shape rather than exposing only CLI-oriented composition
- no documented automatic embedded-mode injection loop exists yet

Planning consequence:
- `atm-graft` is no longer a large protocol bootstrap effort
- it is now a thin follow-on line on top of the current IPC/runtime baseline
- the work should therefore live as three additive Phase T sprints

## 4. Gap Analysis

### G.1 Embeddable client-surface gap

Current state:
- same-host daemon IPC already exists
- `atm` already proves the retained daemon-client path
- there is not yet one small, explicit `atm-core` client surface tailored for
  embedded host-agent consumers

Required change:
- define the public client-side request/response/session-facing models needed
  by `atm-graft`
- keep those models in `atm-core`
- keep concrete runtime/socket behavior out of the public `atm-core` surface
- update the protocol/interface docs that describe those client-facing request
  and response boundaries

### G.2 Session/nudge runtime gap

Current state:
- the daemon is already the correct owner of runtime coordination
- there is no graft registration / unregistration path
- there is no daemon-owned bounded pending-nudge queue or drain request

Required change:
- add graft registration / unregistration handlers
- add daemon-owned bounded nudge queueing
- add nudge drain/fetch requests for embedded and hook/poll consumers
- add one live embedded-session receive loop per active `GraftSession`
- keep queue ownership and backpressure behavior entirely daemon-side
- update the protocol/interface docs for registration, drain/fetch, and daemon
  event payloads

### G.3 Thin crate gap

Current state:
- there is no `atm-graft` crate yet
- host binaries therefore cannot consume a stable embedded ATM client surface

Required change:
- add the `atm-graft` crate as a thin wrapper over the `atm-core` client
  contract
- add minimal `[atm.graft]` activation
- add `GraftSession`
- add host-facing nudge fetch/drain bridging

## 5. Phase T Work Packages

### T.6: Embeddable Graft Client Surface

Implementation scope:
- `atm-core`
- `atm`
- `atm-daemon`

Deliverables:
- name the concrete `atm-core` graft-facing traits:
  - `AtmGraftClient`
  - `GraftSessionPort`
- typed `atm-core` client/request/response/session models used by embedded
  consumers
- explicit `atm-core` ownership of any public graft-facing protocol types
- `atm` CLI use of that same client surface where appropriate
- no `atm-daemon` crate dependency required for external graft consumers

### T.7: Graft Runtime In Daemon

Implementation scope:
- `atm-daemon`
- `atm-core`
- `atm`

Deliverables:
- graft registration / unregistration protocol
- daemon-owned bounded pending-nudge queue
- daemon-owned drain/fetch API
- automatic embedded-session nudge receive/injection path
- typed backpressure and queue-overflow behavior
- hook-facing `atm` command surface for nudge drain on the same daemon API

Documentation sections amended by T.7:
- `docs/atm-graft/architecture.md` §2.5 `GraftSession`
- `docs/atm-graft/architecture.md` §2.6 `Nudge Delivery Model`
- `docs/atm-graft/requirements.md` §5 `Phase T Embedded-Graft Rules`
- `docs/atm-graft/requirements.md` §5.2 `Req-QA Verification Anchors`

### T.8: `atm-graft` Crate

Implementation scope:
- `atm-graft`
- `atm-core`

Deliverables:
- `atm-graft` crate
- minimal `[atm.graft]` config activation
- `GraftSession` as the concrete implementation of the `atm_core`
  `GraftSessionPort` trait
- public API limited to:
  - `send`
  - `read`
  - `ack`
  - session lifecycle
  - nudge fetch/drain
- optional runtime adapter convenience only if needed by the host integration

Sequencing rule:
- `T.6` must land first because it defines the public graft-facing client
  contract
- `T.7` must land second because queue ownership and drain semantics belong in
  the daemon rather than in the crate
- `T.8` closes the line only after `T.6` and `T.7` are accepted

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
- `GraftSession` registers with the daemon
- nudges are delivered through the daemon session path
- one live receive task/thread runs for the active session
- the host receives automatic between-tool-call injection through the graft
  bridge

Non-production companion path:
- a CLI drain/poll command may still exist for debugging, migration, or
  non-embedded environments
- that path is explicitly not the production completion target for
  `atm-graft`

Architectural consequence:
- the queue must live in the daemon
- `atm-graft` becomes one consumer path, not the owner of queued nudge state
- external terminal automation is not part of the production acceptance path

## 8. Phase T Dependency

`atm-graft` now depends on the hardened Phase T shape, not on a replay of the
older Phase Q wire-protocol plan.

Required upstream outcomes before `atm-graft` implementation closes:
- `T.2` and `T.3` stabilize the SQLite write-path correctness model
- `T.4` and `T.5` close the remaining daemon runtime parity and shutdown/state
  gaps
- `T.6` lands the embeddable client surface
- `T.7` lands registration and daemon-owned nudge queue/drain behavior

The `atm-graft` crate itself is the `T.8` deliverable, not a separate future
phase.

Required closeout checks across `T.6`-`T.8`:
- every sprint doc includes explicit deliverables, dependencies, acceptance
  criteria, QA pointers, and required validation
- `docs/atm-graft/requirements.md` and `docs/atm-graft/architecture.md`
  include concrete verification anchors for `req-qa` and `arch-qa`
