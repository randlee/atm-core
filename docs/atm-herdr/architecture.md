# ATM-Herdr Crate Architecture

## 1. Purpose

This document defines the `atm-herdr` crate architectural boundary.

It complements the product architecture in
[`../architecture.md`](../architecture.md) and owns only the Tokio-native
process boundary to the external `herdr` CLI: argv construction, child
spawning and bounding, stdout/stderr parsing, typed result/error mapping,
and the per-host spawn circuit breaker.

This crate is introduced by Phase AQ (sprints AQ2.6, AQ2.7) and is governed
by [ADR-058](../adr/ADR-058-herdr-local-steer-backend-contract.md), which
this document cites by decision id (`D1`-`D10.1`). It is not part of the
pre-Phase-AQ workspace.

## 2. Architectural Rules

- `atm-herdr` is a leaf process-adapter crate: it depends on `atm-core`
  (for `AgentName`, `HerdrSession`, `RequestDeadline`, and `AtmError`),
  `tokio` (for `tokio::process`), and `serde`/`serde_json` (for
  stderr/stdout JSON parsing) only. No other dependency is permitted
  without a boundary amendment. Passing `HerdrSession` — an already-
  validated `atm-core` newtype — as the adapter's session parameter is
  the whole point of depending on it here: this crate never resolves or
  parses a session identifier itself, it only accepts one the caller
  already validated.
- `atm-herdr` must not depend on `atm-storage`, `atm-storage-rusqlite`,
  `atm-http-runtime`, or `atm-daemon-bootstrap`. Those crates depend on
  `atm-herdr`, never the reverse.
- `atm-herdr` owns every `herdr` argv literal, every Herdr JSON field name
  it reads (`error.code`, `result.agent.agent_status`, …), and every Herdr
  exit-code branch in the workspace. No other crate constructs `herdr`
  argv, matches a Herdr `error.code` string, or spawns a `herdr` child
  process directly — this is the source-audit gate `boundaries.md`
  enforces.
- `atm-herdr` performs no roster lookup, no delivery-channel
  classification, no mail read/write, and no queue-state mutation. Every
  method takes an already-resolved `AgentName` and an already-resolved
  optional session string; it derives neither from `.atm.toml`, roster
  metadata, or its own process environment.
- `atm-herdr` never falls back to a raw-keystroke or terminal-automation
  delivery mechanism. `agent prompt`'s own `agent_blocked` pre-write
  rejection is the only accepted safety gate this crate relies on
  (ADR-058 D4).
- `atm-herdr` is runtime-neutral only insofar as it requires a Tokio
  runtime; it does not attempt to remain generic over an arbitrary async
  runtime the way `atm-graft`'s core does — `tokio::process::Command` is a
  direct, declared dependency, not an optional adapter.

## 3. Module Layout

```text
crates/atm-herdr/
  Cargo.toml            # atm-core, tokio, serde, serde_json only (+ test-utils feature)
  src/
    lib.rs               # public re-exports; crate-level docs
    status.rs             # HerdrAgentStatus, AgentSnapshot
    outcome.rs            # HerdrPromptOutcome, HerdrWaitOutcome, HerdrGetOutcome
    error.rs               # HerdrError, HerdrBreakerState, stderr-JSON parsing (WireEnvelope et al.)
    breaker.rs             # HerdrSpawnBreaker
    adapter.rs              # HerdrProcessAdapter trait
    invoker.rs               # HerdrProcessInvoker (tokio::process implementation)
    testing.rs                # #[cfg(feature = "test-utils")] FakeHerdrProcessAdapter
```

The current in-tree extraction (moving this surface out of
`atm-http-runtime::herdr_process`) may land as a single `lib.rs` before a
later cleanup splits it into the modules above; the module boundary is a
non-normative organizational aid, not a public API commitment. The public
API sketch in §4 is normative.

## 4. Public API Sketch

```rust
// -- status / snapshot -------------------------------------------------

/// Herdr's `agent_status` vocabulary (`idle | working | blocked | done |
/// unknown`, ADR-058 D5). `Unknown` covers any value this crate does not
/// recognize, so a future Herdr status addition degrades safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HerdrAgentStatus { Idle, Working, Blocked, Done, Unknown }

/// Typed projection of Herdr's `AgentInfo`. Only `status` is contracted;
/// `name` and `workspace_id` are informational passthrough only (ADR-058
/// "Explicitly NOT relied upon" — no other `AgentInfo` field, and no JSON
/// key order, carries a contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSnapshot {
    pub name: Option<String>,
    pub status: HerdrAgentStatus,
    pub workspace_id: Option<String>,
}

// -- outcomes -------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HerdrPromptOutcome {
    /// Submission accepted (text written, Enter scheduled +300ms
    /// server-side). Does NOT mean the agent has read or acted on it.
    Accepted(AgentSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrWaitOutcome { pub snapshot: AgentSnapshot }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrGetOutcome { pub snapshot: AgentSnapshot }

/// Every agent Herdr's server currently reports, across all of its
/// workspaces, for the session the child was invoked under (ADR-058
/// D2/D9 sibling; `ResponseResult::AgentList`,
/// `src/app/api/agents.rs:19`). This is the AQ2.7 queue pump's sole
/// liveness/status discovery call in Phase AQ (Rand's 2026-08-26
/// decision) — see architecture.md §6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrListOutcome { pub agents: Vec<AgentSnapshot> }

// -- error ------------------------------------------------------------------

/// Closed mapping of every `error.code` this crate's contract (ADR-058 D8)
/// parses, plus this crate's own transport/timeout/breaker outcomes.
/// `atm-herdr` never matches on `error.message` text or JSON key order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HerdrError {
    AgentBlocked,
    AgentNotFound,
    AgentNotReady,
    AgentTargetAmbiguous,
    AgentNotRunning,
    AgentPromptStalled,
    ServerNotRunning,
    ProtocolMismatch,
    Timeout,
    InvalidAgentName,
    EmptyAgentPrompt,
    ServerUnavailable,
    InternalError,
    /// This crate's own external child-process deadline elapsed
    /// (HR-SAFE-002); the child was killed and reaped before this is
    /// returned.
    TimedOut,
    /// The per-host `HerdrSpawnBreaker` is open; no child was spawned.
    Unavailable { retry_after: std::time::Duration },
    /// An `error.code` this crate does not recognize, passed through
    /// verbatim rather than dropped.
    Advisory { code: String },
}

/// Lets a caller fold a `HerdrError` into the workspace-wide `AtmError`
/// at its own boundary; `atm-herdr` itself never constructs `AtmError`.
impl From<HerdrError> for atm_core::error::AtmError { /* .. */ }

// -- adapter ------------------------------------------------------------------

/// The only cross-crate contract point. `HerdrReceivedHook` (AQ2.6) calls
/// `prompt`; `HerdrQueueWakePump` (AQ2.7) calls `list` then `prompt` on
/// each tick (Rand's 2026-08-26 decision — see architecture.md §6);
/// `atm doctor`'s presence probe calls `get`. `wait` is part of this
/// trait's contract (ADR-058 D2) but is not invoked by any Phase AQ
/// caller; it is retained for a possible future lifecycle-gated delivery
/// mode.
pub trait HerdrProcessAdapter: Send + Sync {
    fn prompt<'a>(
        &'a self,
        agent: &'a atm_core::types::AgentName,
        session: Option<&'a atm_core::HerdrSession>,
        deadline: atm_core::RequestDeadline,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<HerdrPromptOutcome, HerdrError>> + Send + 'a>>;

    /// Contracted (ADR-058 D2), NOT invoked by any Phase AQ caller;
    /// retained for a future lifecycle-gated mode. No diagram in this
    /// document uses this method.
    fn wait<'a>(
        &'a self,
        agent: &'a atm_core::types::AgentName,
        session: Option<&'a atm_core::HerdrSession>,
        until: &'a [HerdrAgentStatus],
        timeout: std::time::Duration,
        deadline: atm_core::RequestDeadline,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<HerdrWaitOutcome, HerdrError>> + Send + 'a>>;

    fn get<'a>(
        &'a self,
        agent: &'a atm_core::types::AgentName,
        session: Option<&'a atm_core::HerdrSession>,
        deadline: atm_core::RequestDeadline,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<HerdrGetOutcome, HerdrError>> + Send + 'a>>;

    /// `herdr agent list` — one call per distinct session; `HERDR_SESSION`
    /// set on the child environment only when `session` is `Some`. The
    /// AQ2.7 queue pump's sole liveness/status discovery call.
    fn list<'a>(
        &'a self,
        session: Option<&'a atm_core::HerdrSession>,
        deadline: atm_core::RequestDeadline,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<HerdrListOutcome, HerdrError>> + Send + 'a>>;
}

// -- breaker ------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HerdrBreakerState {
    Closed,
    Open { retry_after: std::time::Duration },
    HalfOpen,
}

/// Per-host, shared by every member (ADR-058 D10.1). Exponential backoff
/// `1s * 2^consecutive_failures`, capped at 30s. In-memory only
/// (HR-SAFE-004) — never persisted, never per-member.
#[derive(Debug, Clone)]
pub struct HerdrSpawnBreaker { /* .. */ }

impl HerdrSpawnBreaker {
    #[must_use] pub fn new() -> Self { /* .. */ }
    #[must_use] pub fn state(&self) -> HerdrBreakerState { /* .. */ }
    #[must_use] pub fn permits_spawn(&self) -> bool { /* .. */ }
    pub fn record_success(&self) { /* .. */ }
    /// Opens the breaker only for the infrastructure-class outcomes named
    /// in HR-SAFE-005 (`server_not_running`, `protocol_mismatch`, an
    /// external-timeout kill, or a failed `list`/`get` call); a
    /// lifecycle-shaped `HerdrError` must not reach this.
    pub fn record_infrastructure_failure(&self) { /* .. */ }
}

// -- invoker ------------------------------------------------------------------

/// The concrete `tokio::process`-backed `HerdrProcessAdapter`.
#[derive(Debug, Clone)]
pub struct HerdrProcessInvoker {
    breaker: std::sync::Arc<HerdrSpawnBreaker>,
}

impl HerdrProcessInvoker {
    #[must_use]
    pub fn new(breaker: std::sync::Arc<HerdrSpawnBreaker>) -> Self { /* .. */ }
}

impl HerdrProcessAdapter for HerdrProcessInvoker { /* .. */ }

// -- testing (feature = "test-utils") ------------------------------------------

#[cfg(feature = "test-utils")]
pub mod testing {
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum FakeHerdrCall {
        Prompt { agent: String, session: Option<atm_core::HerdrSession> },
        Wait { agent: String, session: Option<atm_core::HerdrSession>, until: Vec<super::HerdrAgentStatus>, timeout: std::time::Duration },
        Get { agent: String, session: Option<atm_core::HerdrSession> },
        List { session: Option<atm_core::HerdrSession> },
    }

    /// Records every call for assertion; configurable per-call outcome.
    /// The sole test double any consumer crate uses below the adapter
    /// boundary (HR-TEST-001).
    #[derive(Debug, Default, Clone)]
    pub struct FakeHerdrProcessAdapter { /* .. */ }

    impl FakeHerdrProcessAdapter {
        #[must_use] pub fn calls(&self) -> Vec<FakeHerdrCall> { /* .. */ }
        pub fn queue_prompt_result(&self, result: Result<super::HerdrPromptOutcome, super::HerdrError>) { /* .. */ }
        pub fn queue_wait_result(&self, result: Result<super::HerdrWaitOutcome, super::HerdrError>) { /* .. */ }
        pub fn queue_get_result(&self, result: Result<super::HerdrGetOutcome, super::HerdrError>) { /* .. */ }
        pub fn queue_list_result(&self, result: Result<super::HerdrListOutcome, super::HerdrError>) { /* .. */ }
    }

    impl super::HerdrProcessAdapter for FakeHerdrProcessAdapter { /* .. */ }
}
```

## 5. Data Flow: Steer Path

Immediate steer is fire-and-forget: `atm-herdr` never waits for a
lifecycle settlement or a recipient turn (sprint-AQ2-6).

```text
atm-daemon-bootstrap (AQ2.6, not owned here)   atm-herdr                          herdr (external process)
---------------------------------------------  ---------------------------------  -------------------------
HerdrReceivedHook::emit(event)
  | resolve AgentName + Option<session>
  | (from roster via atm-core's
  |  LocalMessageReceivedBackend::Herdr,
  |  already resolved before this call)
  v
HerdrProcessInvoker::prompt(agent, session, deadline)
                                                 | breaker.permits_spawn()?
                                                 |   no -> return HerdrError::Unavailable (no spawn)
                                                 |   yes
                                                 v
                                                 Command::new("herdr")
                                                   .args(["agent","prompt",<agent>,WAKE_TEXT])
                                                   .env("HERDR_SESSION", s)?  <- only if Some
                                                 | spawn, external 5s deadline (HR-SAFE-002)
                                                 v
                                                                                    herdr agent prompt <agent> "You have
                                                                                    unread ATM messages. Run: atm read"
                                                                                      resolve target (D1)
                                                                                      blocked check (D4 #4) -- BEFORE any write
                                                                                      write text; schedule Enter +300ms
                                                 <---- stdout/stderr JSON, exit code ----
                                                 |
                                                 | parse_prompt() -> HerdrPromptOutcome | HerdrError
                                                 | breaker.record_success() /
                                                 |   record_infrastructure_failure()
  <----------------------------------------------|
  |
  | map outcome to PostSendEmissionPath
  | (AgentBlocked -> warning, no retry;
  |  Accepted -> ok; other -> requeue policy
  |  is caller-owned, not atm-herdr's)
  v
```

## 6. Data Flow: Queue-Tick Path

Per Rand's 2026-08-26 decision, the AQ2.7 queue pump is a fixed-interval
poller built on `list` + `prompt`, not a per-member lifecycle-gated
`wait` loop. Cadence, FIFO ordering, and all queue state live in the
AQ2.7 pump (`atm-http-runtime`); `atm-herdr` supplies only `list`,
`prompt`, `get`, and the breaker.

```text
atm-http-runtime (AQ2.7, HerdrQueueWakePump,     atm-herdr                          herdr (external process)
not owned here)
------------------------------------------------ ---------------------------------  -------------------------
every 5 s tick:
  list_pending_members()  [atm-storage]
  | filter DeliveryChannel::HerdrSteer
  |   [atm-core::delivery_channel]
  | group pending members by session
  v
HerdrProcessInvoker::list(session, deadline)        (once per distinct session)
                                                  | breaker.permits_spawn()?
                                                  |   no -> HerdrError::Unavailable (no spawn)
                                                  | yes: spawn, external 5s deadline (HR-SAFE-002)
                                                  v
                                                                                     herdr agent list
                                                                                       (HERDR_SESSION=<session> if Some)
                                                  <---- Vec<AgentSnapshot> (exit 0),
                                                        or error.code (exit 1) ----
  <-------------------------------------------------|
  | record_observed_state(member, state,
  |   RuntimeObservationSource::HerdrPoll)  [atm-http-runtime,
  |   RuntimeHealth] -- never record_heartbeat; never writes pid
  |
  | for each pending member whose listed
  |   status is Idle | Done:
  |     claim_next_pending(member)  [atm-storage, oldest first: FIFO]
  |     rebuild_received_hook_dispatch(.., NudgeKind::Queue)  [atm-core]
  |     -> HerdrReceivedHook (atm-daemon-bootstrap, AQ2.6)
  |          -> HerdrProcessInvoker::prompt(agent, session, deadline)
  |               (identical call/diagram to §5)
  |
  |   at most one prompt per member per tick;
  |   a host-wide cap bounds total prompts issued this tick
  v
match prompt outcome (from §5)
  AgentBlocked | AgentNotFound  -> release_pending(member, claim)  [atm-storage]
                                    (no retry-budget spend)
  Accepted                      -> claim completes
match list outcome
  HerdrError::ServerNotRunning |
  HerdrError::ProtocolMismatch |
  HerdrError::Timeout |
  (list call itself failed)     -> breaker.record_infrastructure_failure()
                                    (HR-SAFE-005); listed members this tick
                                    are skipped, not claimed
```

## 7. Error Mapping Table

| stderr `error.code` (ADR-058 D8) | `HerdrError` variant | Emitted by | Source |
| --- | --- | --- | --- |
| `agent_blocked` | `AgentBlocked` | `prompt` | D4 row 4; pre-write rejection |
| `agent_not_found` | `AgentNotFound` | `prompt`, `get` (`wait`, if ever invoked: initial probe) | D1, D9 |
| `agent_not_running` | `AgentNotRunning` | (`wait` mid-wait only — unreachable in Phase AQ, `wait` not invoked) | D5 |
| `agent_target_ambiguous` | `AgentTargetAmbiguous` | `prompt`, `get` (`wait`, if ever invoked) | D1 |
| `agent_not_ready` | `AgentNotReady` | `prompt` | D4 rows 5, 7 |
| `agent_prompt_stalled` | `AgentPromptStalled` | (unreachable — `--wait` never emitted, HR-CORE-002) | D4; parsed for completeness only |
| `timeout` | `Timeout` | (`wait` only — unreachable in Phase AQ, `wait` not invoked) | D5, Herdr-side deadline |
| `agent_prompt_failed` | (folds into `InternalError`) | `prompt` | D4 row 8 |
| `empty_agent_prompt` | `EmptyAgentPrompt` | (unreachable by construction — HR-CORE-002's text is a non-empty constant) | D4 row 1 |
| `server_not_running` | `ServerNotRunning` | any (including `list`) | D3 |
| `protocol_mismatch` | `ProtocolMismatch` | any (including `list`) | D3 |
| `server_unavailable` | `ServerUnavailable` | `prompt`, `list` | D4 |
| `internal_error` | `InternalError` | any | — |
| `invalid_agent_name` | `InvalidAgentName` | (not reachable from this crate's `prompt`/`get`/`list` calls; reserved for the `start`/`rename` argv this crate does not emit, D6) | D6 |
| any other string | `Advisory { code }` | any | never matched on `error.message` or key order |
| (exit 2, plain text, no JSON) | impossible by construction | any | argv bug — must fail a fixture/unit test before reaching a real invocation |
| (child never exits) | `TimedOut` | any (including `list`) | this crate's own external deadline, HR-SAFE-002 |
| (breaker open) | `Unavailable { retry_after }` | any | HR-SAFE-006, no spawn attempted |
| (`agent list` call itself fails: transport error or non-zero exit) | breaker-opening trigger | `list` | HR-SAFE-005; treated as infrastructure-class regardless of the specific `error.code` returned, because a failed roster-wide `list` means the pump cannot observe any member this tick |

`empty_agent_prompt` and `agent_prompt_stalled` are parsed (never
`panic!`-on-unknown) so a Herdr behavior change surfaces as a typed,
observable outcome rather than an unwrap failure, even though this
crate's own argv construction should make them unreachable. `wait`-only
rows (`agent_not_running`, `timeout`, and `wait`'s initial-probe path for
`agent_not_found`/`agent_target_ambiguous`) remain part of this crate's
contract (`HR-CORE-003`) but are unreachable in Phase AQ because no
caller invokes `wait`.

## 8. Breaker State Machine

```text
                         record_infrastructure_failure()
                         (server_not_running | protocol_mismatch |
                          external-timeout kill | failed `agent list` call |
                          failed `agent get` probe)
              +---------------------------------------------------------+
              |                                                          |
              v                                                          |
        +-----------+   consecutive_failures reaches implicit trip   +--------+
        |  CLOSED   | ----------------------------------------------> |  OPEN  |
        +-----------+   (first infrastructure-class failure opens     +--------+
              ^          the breaker; backoff starts at 1s)                |
              |                                                            | backoff elapses:
              | probe succeeds                                            | min(1s * 2^n, 30s)
              | (record_success(), reset n=0)                             v
        +-----------+                                              +-------------+
        |  (closed) | <------------------------------------------- |  HALF_OPEN  |
        +-----------+          probe fails (n += 1,                +-------------+
                                 backoff recalculated,                     |
                                 returns to OPEN)  <-----------------------+
```

- While `OPEN`, `permits_spawn()` returns `false` without spawning a
  child; every call returns `HerdrError::Unavailable { retry_after }`.
- Exactly one probe is allowed through when `retry_after` elapses
  (`HALF_OPEN`); its own success or infrastructure-class failure decides
  the next transition. A lifecycle-shaped failure (`AgentBlocked`,
  `AgentNotFound`, …) during `HALF_OPEN` does not reopen the breaker
  (HR-SAFE-005) — it is a per-target condition, not evidence the Herdr
  server itself is unavailable.
- State is one `HerdrSpawnBreaker` instance per host process (§9); it is
  never serialized, never keyed per member, and resets to `Closed` on
  daemon restart (HR-SAFE-004).

## 9. Composition

`atm-herdr` defines the `HerdrSpawnBreaker` type but does not construct
its own singleton — composition is the responsibility of the composition
root, matching how `active_received_hook_selector` and
`StorageAndNudgeRouter` are already built (sprint-AQ2-7 deliverable 1):

- `atm-daemon-bootstrap::build_replacement_handler` constructs exactly one
  `Arc<HerdrSpawnBreaker>` and exactly one `HerdrProcessInvoker` wrapping
  it, at daemon startup.
- That single `Arc<dyn HerdrProcessAdapter>` (erasing to the trait so both
  consumers depend only on `atm-herdr`'s public trait, not on
  `HerdrProcessInvoker` concretely) is handed to:
  - `HerdrReceivedHook` (`atm-daemon-bootstrap`, AQ2.6) for the immediate
    steer path (§5)
  - `HerdrQueueWakePump` (`atm-http-runtime`, AQ2.7) for the queue-tick
    gate path (§6)
- Sharing one breaker instance across both consumers and every member is
  required by ADR-058 D10.1 ("per-host circuit breaker shared by every
  Herdr member") — a second, independently-constructed breaker anywhere
  in the process would defeat the backoff's purpose by letting one path
  spawn children while the other believes Herdr is down.
- No roster row, SQLite table, or `.atm.toml` field stores breaker state;
  it is composition-root-owned, process-lifetime memory only.

## 10. Testing Strategy

- **Argv/contract tests** (`HR-TEST-002`, `HR-TEST-003`) assert
  byte-for-byte argv equality and stderr-parsing correctness for
  `prompt`, `get`, and `list` against `herdr-cli-contract-fixture.md`'s
  rows, so this crate's parsing logic and the pinned Herdr contract
  cannot silently drift apart. `wait`'s argv/parsing is still tested even
  though no Phase AQ caller invokes it (`HR-CORE-003`), so the contracted
  method does not silently bit-rot.
- **Deadline tests** (`HR-TEST-004`) use a fake adapter whose future never
  resolves to prove the single flat 5 s external bound fires, kills, and
  reaps the child, for each of `prompt`, `get`, and `list`.
- **Breaker tests** (`HR-TEST-005`) drive `HerdrSpawnBreaker` directly
  through `CLOSED -> OPEN -> HALF_OPEN -> CLOSED` and
  `CLOSED -> OPEN -> HALF_OPEN -> OPEN` (failed probe) without spawning
  any process.
- **Consumer-facing double**: `testing::FakeHerdrProcessAdapter`
  (`feature = "test-utils"`) is the only test double any other crate uses
  below the adapter boundary; no consumer test constructs a real
  `HerdrProcessInvoker` or shells out to `herdr` (`HR-TEST-001`,
  `HR-TEST-006`).
- **No live Herdr in CI.** Every automated test in this crate and every
  consumer runs without a `herdr` binary on `PATH`. Live validation
  (`herdr-cli-contract-fixture.md` §F5, and AQ2.6's required macOS/Linux
  transcript) is a manual procedure outside `just test`.

## 11. Boundary Verification Anchors

`arch-qa` and `req-qa` should reject the `atm-herdr` line if any of the
following are not true:

- `atm-herdr`'s only workspace dependency is `atm-core`; its only external
  dependencies are `tokio`, `serde`, `serde_json` (plus `test-utils`-gated
  test helpers)
- no `herdr` argv literal, Herdr JSON field name, or Herdr `error.code`
  string literal appears anywhere outside `crates/atm-herdr`
- `HerdrProcessAdapter` is the only public trait this crate exposes for
  cross-crate use; `HerdrProcessInvoker` is constructed exactly once per
  daemon process, at the composition root, never per-member
- `HerdrSpawnBreaker` state is never written to SQLite, the roster, or
  `.atm.toml`
- the `test-utils` feature gates `FakeHerdrProcessAdapter`; it is not
  reachable from a non-test, non-`test-utils` build
- no test in this crate or any consumer crate depends on a `herdr` binary
  being present on `PATH`
- `HerdrQueueWakePump` (`atm-http-runtime`, AQ2.7) calls `list` and
  `prompt` only; no Phase AQ call site invokes `wait`
