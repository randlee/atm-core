# AF-1 — Host-wide daemon singleton

## Sprint intent

Close `SMOKE-FIND-001` at production quality. ADR-002's invariant, retained
and superseded without weakening by planned ADR-026, is literal:
for one OS user on one host, **no second `atm-daemon` process may remain
running**, irrespective of `ATM_HOME`, working directory, CLI binary override,
or direct invocation of `atm-daemon`.

This sprint does not claim to close doctor, logging, or broad release-process
work; those are AF-2.

## Contract to implement

Replace the current per-`ATM_HOME` topology with one non-configurable
`HostRuntimeScope` used by both client and daemon. Its paths must be obtained
from OS account/platform APIs rather than `HOME`, `USERPROFILE`, `ATM_HOME`,
`ATM_DAEMON_SOCKET`, or the current directory.

```rust
/// Production identity of the single runtime permitted for this OS user/host.
pub struct HostRuntimeRoot(PathBuf);
pub struct DurableStateRoot(PathBuf);

pub struct HostRuntimeScope {
    pub runtime_root: HostRuntimeRoot,
    pub durable_state_root: DurableStateRoot,
    pub launch_lock: PathBuf,
    pub owner_lock: PathBuf,
    pub socket: LocalIpcEndpoint,
}

pub fn current_host_runtime_scope() -> Result<HostRuntimeScope, AtmError>;

pub enum DaemonAdmission {
    Started { owner: DaemonOwner },
    ServingExisting { endpoint: LocalIpcEndpoint },
    Rejected { code: DaemonAdmissionCode },
}

pub enum DaemonAdmissionCode {
    LaunchGateContended,
    OwnerAlreadyHeld,
    SocketOverrideForbidden,
}
```

`HostRuntimeScope` is the sole source of the pre-spawn launch lock, daemon-side
owner lock, default local IPC endpoint, and the one host-scoped SQLite durable
state root. A supplied `ATM_DAEMON_SOCKET` is rejected with a typed
configuration error on production commands; it cannot select a parallel
endpoint. `ATM_HOME` may remain a workspace/config discovery input where
documented, but it cannot request or select another daemon runtime or durable
database. The daemon records the canonical durable-state fingerprint in owner
metadata and health output for diagnosis, not client admission selection.

The implementation must retain three independent barriers:

1. client launch lock before spawning;
2. daemon owner lock after exec and before bind; and
3. a fixed singleton endpoint whose bind fails if already occupied.

Failure at any barrier leaves no child daemon running. Shutdown removes only
the locks owned by that process and never unlinks another process's endpoint.

## Hard dependencies and delivery standard

AF1-D0 must land before D1–D6. D1 precedes D2–D4; D2 and D3 precede D5; D3
precedes D6. D5 is the phase dependency for AF-2/AF-3 shared-smoke work.
Every table row is expected to land production-ready: code, governing docs and
machine-readable boundary records, validation, and negative-path recovery
must all close in the same sprint.

## Error inventory

| Failure mode | Stable code | Required recovery |
| --- | --- | --- |
| Caller supplies `ATM_DAEMON_SOCKET` | `SocketOverrideForbidden` | Remove the override and connect through the host singleton endpoint. |
| A second launcher or daemon attempts admission | `LaunchGateContended` or `OwnerAlreadyHeld` | Connect to the serving endpoint; never retry by changing `ATM_HOME`. |
| Runtime-root permission or symlink validation fails | a documented `AtmErrorCode` selected by D0 | Repair ownership/permissions and remove the unsafe artifact; do not bypass the guard. |

## Authoritative deliverables

| ID | Deliverable | Primary paths | Acceptance criteria | Required validation |
| --- | --- | --- | --- | --- |
| AF1-D0 | Superseding architecture decision and runtime-path inventory | `docs/requirements.md`, `docs/architecture.md`, `docs/adr/ADR-026-host-singleton-and-durable-state-root.md`, `docs/adr/ADR-002-host-wide-daemon-singleton.md`, `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`, `docs/adr/INDEX.md`, `docs/plans/phase-af/af-1-runtime-path-inventory.md` | Before D1–D6 implementation begins, new ADR-026 supersedes ADR-002 and ADR-005 without rewriting either accepted record; ADR-002/005 are marked Superseded and the index is updated. ADR-026 defines macOS/Linux/Windows OS-account-derived runtime and durable-state roots, permission/symlink checks, migration from `ATM_HOME/.atm/{daemon,db}`, and the one-daemon/one-SQLite-root invariant. The inventory classifies every `ATM_HOME`, `ATM_DAEMON_SOCKET`, direct-daemon, lock/socket, and db-root derivation call site as retained workspace/config use, canonical host-state use, rejected override, or removed. | Reviewer can inspect ADR-026, supersession links, index, requirements/architecture alignment, and checked-in inventory; inventory-to-`rg` reconciliation has no unclassified runtime-admission or durable-state-root call site. |
| AF1-D1 | Shared `HostRuntimeScope` boundary | `crates/atm-core/src/home.rs` (or dedicated runtime module) | Implements D0's approved OS-user runtime root, durable SQLite root, and no-override contract with semantic `HostRuntimeRoot`/`DurableStateRoot` types. All lock, endpoint, and durable-state derivation is centralized behind `current_host_runtime_scope`. | Unit tests cover path construction without consulting forbidden environment variables; source inventory identifies every former `host_runtime_lock_path*` and db-root derivation user. |
| AF1-D2 | Client admission uses the shared scope | `crates/atm-daemon-client/src/lib.rs`, `crates/atm/src/composition.rs` | Client acquires D1's launch lock, connects to the canonical endpoint when an owner exists, and never spawns after a contended/rejected admission. `ATM_HOME` cannot create another gate. | Concurrent-client process test: home A and home B result in one owner PID and one endpoint. |
| AF1-D3 | Daemon admission and bind use the shared scope | `crates/atm-daemon/src/host_ownership.rs`, `crates/atm-daemon/src/local_ipc_transport.rs`, `crates/atm-core/src/protocol.rs` | Direct daemon execution obtains the same owner lock and endpoint as D2. A second direct executable exits typed before bind. Owner metadata prevents accidental lock/socket cleanup by non-owner processes. | Start direct daemon A, start direct daemon B under a different home, assert B's typed rejection, A remains healthy, and process count is one. |
| AF1-D4 | Canonical durable-state binding | runtime composition, `crates/atm-core/src/protocol.rs`, doctor/health projection | Every client, including one launched with a distinct `ATM_HOME`, connects to the same serving daemon and its one canonical SQLite durable-state root. The daemon reports only a safe root fingerprint for diagnosis; no client-selected alternate state root or mismatch admission path exists in the supported runtime. | Process test from homes A/B proves one owner PID, one endpoint, one durable-state fingerprint, and one database mutation/readback path; changing `ATM_HOME` cannot create a second database or daemon. |
| AF1-D5 | Production-equivalent test and lint gates | `scripts/lint_daemon_singleton.py`, `scripts/smoke/run_thorough_shared_host.py`, CI workflow | Static gates scan Rust, Python, shell, and CI launch sites for alternate daemon roots/endpoints. Smoke harness cannot leave a daemon and contains no bypass. Tests run as an isolated OS user/CI host rather than using a runtime test escape hatch. | Grep/lint gate passes; controlled full-smoke process test reports one PID before/during/after; cleanup assertion is mandatory. |
| AF1-D6 | Reliable lifecycle cleanup | daemon lifecycle/shutdown modules and integration tests | Graceful termination releases owned resources within the documented timeout; ungraceful death permits safe takeover only after ownership verification. No test relies on `SIGKILL` as normal cleanup. | TERM integration test, stale-owner recovery test, and repeat start/stop loop leave zero owned lock/socket artifacts. |

## Paths to delete or replace

| Retired path | Required replacement / proof |
| --- | --- |
| Per-`ATM_HOME` launch and owner-lock derivation through `host_runtime_lock_path*` | D1's `current_host_runtime_scope`; no caller may derive an admission lock from `ATM_HOME`. |
| Per-`ATM_HOME` daemon endpoint and database-root selection through `daemon_socket_path_from_home` and db-root helpers | D1/D4's fixed singleton endpoint and canonical durable-state root; `ATM_HOME` is not an admission or database selector. |
| `launch_gate_isolated_per_atm_home_root` acceptance test and equivalent fixture assumptions | D2/D3 process test proving cross-home second launch is rejected and leaves one owner. |

## Non-closure

AF-1 does not change doctor hook output, generic connection-worker logging,
or wrapper/documentation policy. It may add only the admission diagnostics
needed to make the singleton contract testable.
