# Phase R Continuation — Issues Tracker

**Branch base**: `integrate/phase-R` @ `40b9842`
**Review sources**: TASK-995 (arch-ctm gap analysis) + TASK-996 (quality-mgr Phase R final QA-1)
**Created**: 2026-05-06
**Status**: OPEN — Phase R not merge-ready

---

## BLOCKING FINDINGS

### B-001 — Singleton lock paths are socket-scoped, not host-wide
**Source**: arch-ctm TASK-995 Finding #1 + quality-mgr REQ-R-001
**Files**: `crates/atm/src/composition.rs:65-68,301-303` | `crates/atm-daemon/src/lib.rs:246-247`
**ADR**: ADR-002 §3.2 explicitly rejects socket-scoped enforcement

Both guard layers derive their lock path from the socket path:
- `LaunchGateGuard` → `socket_path.with_extension("launch.lock")`
- `SingletonGuard` → `socket_path.with_extension("lock")`

Different `ATM_HOME`/`ATM_DAEMON_SOCKET` configs produce different lock paths, allowing
multiple daemon processes to coexist on the same host.

**Fix**: Move both lock files to a single fixed host-wide path not derived from socket or home
config. Keep socket path as serving endpoint only.

---

### B-002 — Five ADR-002 error codes absent from error_codes.rs
**Source**: quality-mgr REQ-R-002
**Files**: `crates/atm-core/src/error_codes.rs` | `docs/atm-error-codes.md §5.10.4`

Documented but not implemented:
- `ATM_DAEMON_LAUNCH_GATE_REJECTED`
- `ATM_DAEMON_SERVING_STATE_REJECTED`
- `ATM_DAEMON_STALE_OWNER_RECOVERY_FAILED`
- `ATM_DAEMON_AUTO_START_FAILED`
- `ATM_TEST_FAKE_TRANSPORT_INJECTION_FAILED`

**Root cause**: These codes are the typed rejection paths for the singleton gate. They were
documented in ADR-002 and error-codes doc but never wired because the host-wide gate (B-001)
was never fully implemented. B-001 and B-002 are the same underlying gap.

**Fix**: Implement all five in `error_codes.rs` and wire each to its corresponding rejection
path once B-001 is resolved.

---

### B-003 — RuntimeComposition startup/lifecycle is scaffold-only
**Source**: arch-ctm TASK-995 Finding #2 + TASK-997 gap report item 2
**Files**: `crates/atm-daemon/src/composition.rs:39-49,109-118` | `crates/atm-daemon/src/lib.rs:625-684,781-782`

`RuntimeComposition::start()` returns `"daemon runtime start scaffold is not implemented yet"`.
`run_daemon()` bypasses `start()` entirely and calls `serve()` directly. No lifecycle
transitions (Starting/Running/Draining/Stopped), no startup ownership checks, no shutdown
path routing through a single runtime root.

**Fix**: Implement actual runtime bootstrap path. Wire explicit lifecycle state machine.
Route all startup/shutdown through one runtime root rather than bypassing it.

---

### B-004 — Live daemon status cache not implemented
**Source**: arch-ctm TASK-997 gap report item 4
**Files**: `crates/atm-daemon/src/lib.rs` | `crates/atm-core/src/boundary_support.rs`

`DaemonStatusSource` delegates to `boundary_support::snapshot_status()`, which returns a
placeholder ready/detail response. Requirements say daemon memory owns live status, caches
durable PID, and owns `last_active_at`. No runtime status map/cache exists. No
cache-rebuild-from-unknown on restart. No cap/eviction behavior.

**Fix**: Implement daemon-memory runtime status cache. Wire durable PID from SQLite as
primary liveness field. Implement `last_active_at` in daemon memory. Add restart recovery
and cap/eviction behavior per daemon architecture.

---

### B-005 — Heartbeat/member runtime-state path completely absent
**Source**: arch-ctm TASK-997 gap report item 5
**Files**: `crates/atm-core/src/protocol.rs`
**Docs**: `docs/team-member-state.md`

`RequestEnvelope` only supports Send/Receive/Clear/Doctor. No heartbeat request family
exists anywhere in the codebase. No handler, no runtime state machine for
Active/Idle/Offline, no `AgentPidChanged` emission, no PID ownership conflict detection,
no admin takeover path for live-old-pid conflicts.

**Fix**: Add heartbeat protocol request/response family. Implement daemon handler for
`TeamMateHeartbeat`. Wire runtime state machine and PID ownership conflict detection.

---

### B-006 — Doctor daemon health interface unimplemented
**Source**: arch-ctm TASK-997 gap report item 6
**Files**: `crates/atm-daemon/src/lib.rs` daemon dispatcher

Dispatcher calls `atm_core::doctor::run_doctor()` directly — no daemon-backed health
projection. Missing: daemon reachability, singleton ownership status, live status-cache
summary, ingest backlog/degraded-ingest state, SQLite open/readiness state, liveness vs
readiness distinction.

**Fix**: Implement explicit daemon health query surface. Wire daemon-backed health
projection into doctor command. Split liveness from readiness.

---

### B-007 — Watch runtime is placeholder-level
**Source**: arch-ctm TASK-997 gap report item 7
**Files**: `crates/atm-daemon/src/lib.rs` | `crates/atm-core/src/boundary_support.rs`

`FileWatchEventSource` delegates to `boundary_support::poll_watch()`, which discovers
paths once and returns. Not a real long-running watch subsystem. No subscription lifecycle,
no bounded polling/wake behavior, no structured runtime events or degradation handling.

**Fix**: Implement runtime-owned watch loop. Add subscription lifecycle, bounded
polling/wake behavior, and structured runtime events.

---

### B-008 — Reconcile runtime is placeholder-level
**Source**: arch-ctm TASK-997 gap report item 8
**Files**: `crates/atm-daemon/src/lib.rs` | `crates/atm-core/src/boundary_support.rs`

`DaemonReconcileCoordinator` delegates to `boundary_support::reconcile()`, which does one
poll + one import pass. No scheduling, no debounce/coalesce, no explicit ownership of
reconcile triggering or completion semantics.

**Fix**: Implement runtime-owned reconcile scheduling/orchestration with debounce/coalesce
and explicit triggering/completion semantics.

---

### B-009 — PeerClientTransport is an explicit stub
**Source**: arch-ctm TASK-997 gap report item 3
**Files**: `crates/atm-daemon/src/lib.rs`

`PeerClientTransport::send()` returns a stub error. Requirements/architecture still require
remote daemon-to-daemon transport. No request framing over remote transport, no
timeout/retry behavior, no integration into runtime routing path.

**Fix**: Implement peer client transport. Add request framing, timeout/retry, and
integration into runtime routing.

---

## IMPORTANT FINDINGS

### I-001 — Bare expect() panics in signal installation
**Source**: arch-ctm TASK-995 Finding #3
**Files**: `crates/atm-daemon/src/lib.rs:205-208,224-226`

`DaemonShutdownSignals::install()` uses:
- `.expect("daemon signal install lock")`
- `.expect("daemon shutdown signals should be initialized")`

Daemon panics in production on signal handler contention. Arch doc §107-108 states runtime
failures must remain typed.

**Fix**: Convert both paths to typed `AtmError` failures or prove invariants via
non-panicking construction.

---

### I-002 — Force-cancel cannot interrupt socket-blocked threads
**Source**: quality-mgr RSH-R-001/RSH-R-002
**Files**: `crates/atm-daemon/src/lib.rs` shutdown path

`FORCE_CANCEL_DEADLINE` polling loop fires `process::exit(1)` if threads don't exit, but no
`stream.shutdown()` is wired to the force path. A thread blocked on a socket read (up to
`REQUEST_DEADLINE` = 3s) cannot be interrupted before the deadline expires.

**Fix**: Wire `stream.shutdown(Shutdown::Both)` to the force-cancel path so blocked reads
return immediately.

---

### I-003 — Daemon completely silent in default deployments
**Source**: quality-mgr RSH-R-004
**Files**: `crates/atm-daemon/src/lib.rs` init_tracing()

`init_tracing()` returns no-op when `ATM_LOG` is unset. Daemon produces zero log output
by default. Production deployments have no visibility into daemon lifecycle or errors.

**Fix**: Set a default log level (e.g. `warn` or `info`) when `ATM_LOG` is unset so daemon
lifecycle events are always emitted.

---

### I-004 — No request-ID in protocol envelopes
**Source**: quality-mgr RSH-R-006

`RequestEnvelope`/`ResponseEnvelope` have no `request_id` field. No per-request tracing
span is possible. Request correlation across daemon logs is impossible.

**Fix**: Add `request_id: Uuid` to both envelope types and emit a tracing span per request.

---

### I-005 — Wall-clock assertion in read.rs test
**Source**: quality-mgr FTQ-R-001
**File**: `crates/atm/src/commands/read.rs:281`

`assert!(elapsed < Duration::from_secs(4))` on a 5s timeout test. False failures on slow CI.

**Fix**: Remove wall-clock assertion or widen to a multiple of the configured timeout.

---

### I-006 — wait_for_tail_ready polling race + missing child reap in log.rs
**Source**: quality-mgr FTQ-R-002/FTQ-R-003
**File**: `crates/atm/src/commands/log.rs:313,400`

- `wait_for_tail_ready` is polling-based with no deterministic ready signal and no `Drop`-based
  child reap
- `read_record` panics without `child.wait()` after `kill` — Unix zombie left behind

**Fix**: Add deterministic ready signal (pipe/channel); add `Drop` impl that calls `child.wait()`.

---

### I-007 — Concurrent dedup TOCTOU in send.rs
**Source**: quality-mgr FTQ-R-004
**File**: `crates/atm/src/commands/send.rs:354`

File-based alert lock allows two concurrent subprocesses to race through the dedup gate.
Assert expects 1 dedup win but can get 2.

**Fix**: Serialize the dedup check or use an atomic flag rather than a file-based race gate.

---

### I-008 — AgentAddress uses raw String fields
**Source**: quality-mgr RBP-R-002
**File**: `crates/atm-core/src/address.rs:9`

`AgentAddress.agent` and `.team` are raw `String` instead of `AgentName`/`TeamName` newtypes.
Compiler cannot distinguish agent from team at address construction sites.

**Fix**: Replace with `AgentName`/`TeamName` newtypes. Thread through construction call sites.

---

### I-009 — home.rs accepts raw &str instead of newtypes
**Source**: quality-mgr RBP-R-003
**File**: `crates/atm-core/src/home.rs:28`

Public functions accept raw `&str` for team/agent parameters. Same class of type-safety gap
as I-008.

**Fix**: Replace with `&TeamName`/`&AgentName` parameters.

---

### I-010 — Orphaned typestate markers never wired
**Source**: quality-mgr RBP-R-004
**File**: `crates/atm-core/src/types.rs:306`

Five typestate marker types defined but never used as generic type parameters anywhere in
the codebase. Dead code with misleading intent.

**Fix**: Either wire to generic types that use them, or remove.

---

### I-011 — sqlite_error always returns wrong error code
**Source**: quality-mgr RBP-R-005

`sqlite_error` produces `ATM_MESSAGE_VALIDATION_FAILED` regardless of the actual SQLite
failure kind. Callers cannot distinguish constraint violations from I/O failures from
lock timeouts.

**Fix**: Map SQLite error kinds to appropriate `AtmErrorCode` variants.

---

### I-012 — Daemon notification/plugin runtime delivery is placeholder-only
**Source**: arch-ctm TASK-997 gap report item 9
**Files**: `crates/atm-daemon/src/lib.rs` | `crates/atm-core/src/boundary_support.rs`

`DaemonNotificationSink` delegates to `boundary_support::deliver_notification()`, which
just logs a notification-delivered event. Not a real notifier/plugin runtime. No actual
daemon-owned notifier/plugin delivery adapter, no runtime boundary for local agent/plugin
traffic, no failure/degradation handling.

**Fix**: Implement daemon-owned notifier/plugin delivery adapter with failure and
degradation handling.

---

### I-013 — Crash recovery / replay durability not wired as runtime subsystem
**Source**: arch-ctm TASK-997 gap report item 10

Requirements say crash recovery must preserve SQLite commit → export/remote handoff
ordering and support durable replay keyed by `message_key`. `atm-core` has related
boundary shapes but daemon runtime does not wire a concrete replay/re-export subsystem.

**Fix**: Implement durable replay/re-export runtime keyed by `message_key`. Add bounded
persisted retry/re-export state with expiry. Wire startup replay/recovery path after crash.

---

### I-014 — Config reload / serving-config validation not finished
**Source**: arch-ctm TASK-997 gap report item 11

Requirements say daemon config validates at startup and on SIGHUP while preserving
last-known-good serving config. Signal installation exists, but the runtime layer that
validates and applies/rejects serving config on SIGHUP is not implemented.

**Fix**: Implement validated serving-config model. Add bounded SIGHUP reload path with
typed reload failure that does not corrupt serving state.

---

### I-015 — Runtime composition boundaries forward to generic helpers instead of runtime-owned state
**Source**: arch-ctm TASK-997 gap report item 13
**Files**: multiple in `crates/atm-daemon/src/`

`DaemonConfigIngress`, `DaemonInboxIngress`, `DaemonInboxExport`, `DaemonStatusSource`,
`FileWatchEventSource`, `DaemonReconcileCoordinator` all exist as types but several
forward straight into generic `boundary_support` helpers instead of daemon-owned runtime
state/subsystems. Runtime-owned boundaries were declared but not turned into
runtime-owned implementations.

**Note**: This is the code-level form of the parity gap — the boundary surface looks
complete structurally but does not own or manage runtime state.

---

### I-016 — std::env::set_var() in test code — PORT-003 violations
**Source**: arch-inj TASK-993 sc-portability advisory (feature/pR-s3-boundary-lint)
**Files**:
- `crates/atm/src/main.rs:686,697`
- `crates/atm-core/src/config/mod.rs:793`
- `crates/atm-core/src/home.rs:161`
- `crates/atm-core/src/identity/mod.rs:218`
- `crates/atm-core/src/mailbox/lock.rs:1233`
- `crates/atm-core/src/team_admin/restore.rs:664`
- `crates/atm-core/tests/mailbox_locking.rs:893`

8 call sites use `std::env::set_var()` for test environment setup. Unsafe in multi-threaded
contexts per Rust 2024 edition; mutation affects the entire process environment globally.

**Fix**: Replace subprocess setup with `cmd.env("KEY", "value")`. Replace in-process test
mutation with `temp_env::with_var(...)` for scoped, safe environment overrides.

---

## MINOR FINDINGS

### m-001 — Plan doc R.10 status stale
**Source**: quality-mgr REQ-R-003/REQ-R-004
**File**: `docs/plan-phase-R.md`

R.10 section still shows `Status: planned`. ARCH-SINGLETON [B] and CI-WIN-001 [B] recorded
as open on `feature/pR-s10-thin-client` — both resolved in R.10.

**Fix**: Update R.10 status and closure notes to match 40b9842 branch state.

---

## PROCESS GAPS — Architectural Gates Needed

These are systemic failures that allowed B-001 through B-003 to survive seven QA rounds.
New gates must be in place before R.11 begins.

### PG-001 — Round-limit policy must not protect initial implementation
**Problem**: Round-limit scoping is correct for fix/verify loops but was applied to the
initial implementation review. `RuntimeComposition::start()` scaffold and socket-scoped locks
were present since the first R.10 commit but were never re-examined because they were not
in "changed files" for subsequent rounds.

**Gate needed**: Any sprint that delivers new implementation (not a fix round) must run a
full req-qa sweep against ADR requirements and requirements docs, unrestricted by changed-file
scope. Round-limit applies only to fix verification rounds (round 2+).

---

### PG-002 — Scaffold/placeholder returns must be auto-blocking
**Problem**: `RuntimeComposition::start()` returning `"daemon runtime start scaffold is not
implemented yet"` (a string literal) survived all seven QA rounds. No reviewer flagged it.

**Gate needed**: Any string literal containing "not implemented", "scaffold", "placeholder",
"todo", or `todo!()`/`unimplemented!()` macro calls in non-test production code must be a
BLOCKING finding in every QA round, regardless of scope. Add explicit grep-based check to
`just lint` or `lint_daemon_singleton.py`.

---

### PG-003 — ADR compliance checks must be behavioral, not structural
**Problem**: arch-qa passed ADR-002 compliance by verifying that `LaunchGateGuard` and
`SingletonGuard` types exist and hold file locks. It did not verify that the lock _path_
satisfies the "not socket-scoped" constraint stated in ADR-002 §3.2.

**Gate needed**: Each ADR must include a testable behavioral assertion (a lint check, a
unit test, or an explicit compliance checklist item) that can be mechanically verified.
Structural presence alone is insufficient.

---

### PG-004 — Phase-ending review must happen per implementation sprint, not at phase end
**Problem**: By the time the phase-ending review ran, ten sprints of accumulated work meant
the gap between documented intent and actual implementation was very large. B-003 in
particular grew across multiple sprints without detection.

**Gate needed**: After each implementation sprint (R.10.x class), run a targeted
requirements-vs-implementation spot check covering the ADRs that sprint claims to address.
This is separate from and in addition to the normal QA round.

---

## ARCH-CTM ROOT CAUSE FRAMING

From arch-ctm TASK-997 gap report:

> send/ack/read/clear were implemented through atm-core parity, so the daemon line looked
> healthy in normal command flow. Singleton/test/lint/Windows churn consumed most of the
> implementation and QA focus. The unresolved runtime-owned daemon subsystems stayed in
> placeholder form while CI still passed on the thin local request path.
>
> Phase R merged a working thin daemon request server, but not the full production-ready
> daemon runtime described in plan/docs.

**What Phase R actually delivered** (confirmed working):
- send/ack/read/clear/doctor business logic shared through atm-core dispatcher routing
- log intentionally local via sc-observability (not daemon-owned, by design)
- teams/members intentionally local retained recovery/roster surfaces (by design)
- singleton/test/lint/Windows hardening
- bounded framing, transport seams, test fidelity migration

**What Phase R did NOT deliver** (all B-003 through B-009 above):
- Full production daemon runtime with owned runtime subsystems
- Host-wide singleton (B-001, B-002)
- Runtime lifecycle orchestration (B-003)
- Status cache, heartbeat, health, watch, reconcile (B-004 through B-008)
- Peer transport (B-009)

---

## EXTRACTION READINESS (arch-ctm scores)

| Crate | Score | Main blocker |
|-------|-------|-------------|
| `atm-core` | 4/5 | Placeholder boundary_support-backed runtime behavior; test-support/env seams close to core |
| `atm-daemon` | 2/5 | Host-wide singleton not host-wide; `RuntimeComposition::start()` stubbed; peer transport stubbed; status/watch/reconcile placeholders |
| `atm-rusqlite` | 4/5 | Mostly blocked by daemon/runtime integration incompleteness, not its own shape |
| `atm` | 3/5 | Transport seams + lint gate improved; extraction still depends on singleton gate semantics + stable daemon contract |

---

## ARCH-CTM RECOMMENDED COMPLETION BUCKETS

arch-ctm proposed these groupings for remaining Phase R work. Sprint breakdown pending
team-lead/arch-ctm planning discussion.

| Bucket | Scope |
|--------|-------|
| A | Host-wide singleton (B-001, B-002) + runtime lifecycle completion (B-003) |
| B | Heartbeat/status-cache (B-004, B-005) + doctor health (B-006) |
| C | Peer transport (B-009) + remote retry/recovery (I-013) |
| D | Watch (B-007) + reconcile (B-008) + notifier runtime (I-012) |
| E | Panic removal (I-001, I-002) + config reload (I-014) + final production-hardening sweep |

---

## NEXT STEPS

1. team-lead / arch-ctm planning discussion on scope, sequencing, and sprint definitions
2. Implement process gates PG-001 through PG-004 — required before any R.11 sprint begins
3. Define acceptance criteria per bucket before dispatching implementation
4. QA after each bucket with full no-round-limit sweep (PG-001 enforced)
