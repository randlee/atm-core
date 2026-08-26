# ATM-Herdr Boundary Inventory

This document is the crate-local boundary inventory for `atm-herdr`.

`atm-herdr` is the Tokio-native process boundary to the external `herdr`
CLI. It owns every `herdr` argv shape, every Herdr JSON field this
workspace reads, and every Herdr exit/error-code branch — it must remain a
thin, leaf process-adapter crate rather than gaining roster, mail, queue,
or delivery-decision logic.

Canonical machine-readable boundary source:
- [../../boundaries/atm-herdr/herdr-process-adapter.toml](../../boundaries/atm-herdr/herdr-process-adapter.toml)

## Herdr Process Adapter

Purpose:
- own the `HerdrProcessAdapter` trait and its concrete
  `tokio::process`-backed `HerdrProcessInvoker` implementation
- own argv construction for every `herdr agent {prompt,wait,get,list}`
  shape this crate emits (ADR-058 D2), including per-invocation
  `HERDR_SESSION` child-environment handling (ADR-058 D1); `wait` is
  contracted but not invoked by any Phase AQ caller (Rand's 2026-08-26
  decision — the AQ2.7 queue pump uses `list` + `prompt` instead)
- own the external, per-child kill/reap deadline independent of any
  `--timeout` argv value (ADR-058 D10)
- own typed result (`AgentSnapshot`, `HerdrPromptOutcome`,
  `HerdrWaitOutcome`, `HerdrGetOutcome`, `HerdrListOutcome`) and error
  (`HerdrError`) mapping from Herdr's structured stderr JSON and exit
  codes (ADR-058 D8)
- own the per-host `HerdrSpawnBreaker` circuit-breaker type
  (ADR-058 D10.1)
- own a `test-utils`-gated fake adapter for use by every consumer crate's
  tests

Rules:
- `atm-herdr` must not take a Rust dependency on `atm-storage`,
  `atm-storage-rusqlite`, `atm-http-runtime`, `atm-daemon-bootstrap`, or
  `atm-daemon`
- `atm-herdr` must not perform direct SQLite access, direct roster
  reads/writes, or direct mail read/write of any kind
- `atm-herdr` must not resolve `LocalMessageReceivedBackend`,
  `HerdrSession`, or any other roster-derived routing value; every public
  method takes an already-resolved `AgentName` and an already-resolved
  optional session string supplied by the caller
- `atm-herdr` must not implement `PendingNudgeStore`, `MemberKey`,
  `NudgeClaim`, or any queue-state mutation; queue claim/release/requeue
  policy is entirely caller-owned
- `atm-herdr` must not implement the sealed
  `AsyncMessageReceivedHookEmitter` contract itself; `HerdrReceivedHook`
  (which does implement it) lives in `atm-daemon-bootstrap` and calls this
  crate's adapter
- `atm-herdr` must not implement, drive, or schedule the AQ2.7 queue-tick
  pump loop: its 5 s polling cadence, per-session `agent list` grouping,
  per-tick prompt cap, and FIFO claim ordering all belong to
  `atm-http-runtime`'s `HerdrQueueWakePump`, which only calls this
  crate's `list` and `prompt` methods (Rand's 2026-08-26 decision)
- `atm-herdr` must not fall back to `agent send-keys`, `pane send-keys`,
  `pane send-input`, `tmux send-keys`, or any other raw-keystroke or
  terminal-automation delivery mechanism on any code path
- `atm-herdr` must never pass a sender's original ATM message body to
  Herdr; the only text it writes into a Herdr child's argv is the fixed
  mailbox-read prompt constant (ADR-058 D2)
- `atm-herdr` must never read `HERDR_SESSION` or `HERDR_SOCKET_PATH` from
  its own process environment to select a session; the caller's explicit
  `Option<&str>` argument is the only session source (ADR-058 D1)
- `HerdrSpawnBreaker` state must remain in-memory, process-lifetime,
  per-host, and shared by every member; it must never be persisted to
  SQLite, the roster, or `.atm.toml`, and must never be keyed per member
- `HerdrSpawnBreaker` must open only on infrastructure-class outcomes
  (`server_not_running`, `protocol_mismatch`, an external-timeout kill, a
  failed `agent list` call, or a failed `agent get` doctor probe) and
  must never open on a lifecycle/target-shaped outcome (`agent_blocked`,
  `agent_not_found`, `agent_not_ready`, `agent_target_ambiguous`)
- no `herdr` argv literal, Herdr JSON field name, or Herdr `error.code`
  string literal may appear anywhere outside `crates/atm-herdr` — this is
  the source-audit gate enforced alongside this boundary
- `atm-herdr` must not depend on, or vendor test doubles that bypass, a
  real `herdr` binary in any automated test; `testing::FakeHerdrProcessAdapter`
  behind the `test-utils` feature is the sole permitted test double below
  the adapter boundary
- `atm-herdr` must not run or wrap `herdr agent start` or
  `herdr agent rename`; those are operator/launch-convention commands
  this crate defines no argv for (ADR-058 D6)

## Governing forbidden edges

The canonical boundary record's `forbidden_edges` list (mirrored here for
doc-review visibility; the TOML is authoritative):

- `atm-core -> atm-herdr`
- `atm-storage -> atm-herdr`
- `atm-storage-rusqlite -> atm-herdr`
- `atm-herdr -> atm-daemon-bootstrap`
- `atm-herdr -> atm-http-runtime`

`atm-core` and `atm-storage` are lower in the dependency graph than
`atm-herdr` and must never depend upward into it; `atm-herdr` is a leaf
process adapter and must never depend "sideways" into the two crates that
consume it (`atm-daemon-bootstrap`, `atm-http-runtime`) or that would
create a cycle.

## io_forbidden

- `direct_sqlite_io` — no SQLite connection, statement, or transaction of
  any kind
- `message_delivery` — `atm-herdr` injects prompts into a Herdr terminal
  session; it never constructs, stores, or delivers ATM mail. A steer or
  queue-tick prompt is a wake-up signal only, never a mailbox write
- `roster_mutation` — `atm-herdr` never writes a roster row, a
  `recipient_pane_id`, a `metadata_json` field, or any other durable
  routing datum; roster data flows into this crate only as already-parsed
  function arguments

## error_types

- `AtmError` — the workspace-wide error type a caller may fold a
  `HerdrError` into at its own boundary via `From<HerdrError> for
  AtmError`; `atm-herdr` itself never constructs an `AtmError` internally
- `HerdrError` — this crate's own closed error enum (see
  [`architecture.md`](./architecture.md) §4, §7), covering every Herdr
  `error.code` this crate's contract parses plus this crate's own
  timeout/breaker outcomes

## Rationale

`atm-herdr` exists so that exactly one crate in the workspace understands
Herdr's wire contract. Every other crate that needs a Herdr agent to wake
up — the immediate-steer emitter in `atm-daemon-bootstrap`, the
lifecycle-gated queue pump in `atm-http-runtime`, and `atm doctor`'s
presence probe in `atm-core`/`atm` — calls this crate's typed
`HerdrProcessAdapter` trait and never constructs `herdr` argv, parses
Herdr JSON, or matches a Herdr `error.code` itself. This mirrors the
existing `atm-storage::contract` pattern (a sealed, narrow trait owned by
one crate and consumed everywhere else) rather than letting Herdr's
process/wire details leak into the daemon composition, storage, or CLI
layers. Keeping the crate a strict `atm-core`-only leaf (no
`atm-storage`, no `atm-http-runtime`, no `atm-daemon-bootstrap` edge) also
means a future Herdr protocol bump (ADR-058's pin policy) touches one
crate's tests, not a scattered set of call sites across the workspace —
the same motivation ADR-058's "Required evidence" and this crate's
`HR-VER-002` requirement rely on.

The forbidden-edge set is deliberately symmetric: `atm-core` and
`atm-storage` must not reach up into a Phase-AQ-specific process adapter
they have no need of (an `atm-core -> atm-herdr` edge would make Herdr's
process contract a transitive dependency of every crate that already
depends on `atm-core`, which is most of the workspace), and `atm-herdr`
must not reach sideways into its own consumers (`atm-daemon-bootstrap`,
`atm-http-runtime`), which would create a dependency cycle the composition
root in §9 of `architecture.md` relies on not existing.
