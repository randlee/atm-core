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

### B-003 — Daemon runtime is scaffold, not implementation
**Source**: arch-ctm TASK-995 Finding #2
**Files**: `crates/atm-daemon/src/composition.rs:39-49,109-118` | `crates/atm-daemon/src/lib.rs:625-684,781-782`
**Docs**: `docs/atm-daemon/architecture.md:67-76,93-105,267-299`

Unimplemented surfaces shipped as explicit scaffolds:
- `RuntimeComposition::start()` returns `"daemon runtime start scaffold is not implemented yet"`
- `run_daemon()` bypasses `RuntimeComposition::start()` entirely, only calls `serve()`
- `PeerClientTransport::send()` is an explicit stub
- `DaemonStatusSource`, `FileWatchEventSource`, `DaemonReconcileCoordinator` are placeholder
  adapters delegating to `boundary_support` helpers — no real status cache, watch loop, or
  reconcile runtime

**Fix**: Option B confirmed — implement the daemon runtime. Replace scaffolds with real wiring.
Peer transport, status cache, watch/reconcile, and health surface must be implemented or
explicitly gated with typed `NotImplemented` errors and corresponding descope ADR amendments.

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

## EXTRACTION READINESS (arch-ctm scores)

| Crate | Score | Main blocker |
|-------|-------|-------------|
| `atm-core` | 4/5 | Placeholder boundary_support-backed runtime behavior; test-support/env seams close to core |
| `atm-daemon` | 2/5 | Host-wide singleton not host-wide; `RuntimeComposition::start()` stubbed; peer transport stubbed; status/watch/reconcile placeholders |
| `atm-rusqlite` | 4/5 | Mostly blocked by daemon/runtime integration incompleteness, not its own shape |
| `atm` | 3/5 | Transport seams + lint gate improved; extraction still depends on singleton gate semantics + stable daemon contract |

---

## NEXT STEPS

1. Implement process gates PG-001 through PG-004 before R.11 begins
2. Plan R.11 sprint scope: B-001 host-wide singleton + B-002 error codes + B-003 daemon runtime
3. Include all I-001 through I-011 in R.11 fix scope
4. QA-2 after R.11 fixes with full no-round-limit sweep
