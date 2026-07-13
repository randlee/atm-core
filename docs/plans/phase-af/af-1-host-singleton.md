# AF-1 — Host-wide daemon singleton

## Sprint intent

Close `SMOKE-FIND-001` at production quality. ADR-002's invariant is literal:
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
pub struct HostRuntimeScope {
    pub root: PathBuf,
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
    StateRootMismatch,
    SocketOverrideForbidden,
}
```

`HostRuntimeScope` is the sole source of the pre-spawn launch lock, daemon-side
owner lock, and default local IPC endpoint. A supplied `ATM_DAEMON_SOCKET` is
rejected with a typed configuration error on production commands; it cannot
select a parallel endpoint. The daemon records the active state-root identity
in its owner metadata. A client whose requested state root does not match the
serving daemon receives a typed mismatch error rather than launching another
daemon or silently talking to the wrong database.

The implementation must retain three independent barriers:

1. client launch lock before spawning;
2. daemon owner lock after exec and before bind; and
3. a fixed singleton endpoint whose bind fails if already occupied.

Failure at any barrier leaves no child daemon running. Shutdown removes only
the locks owned by that process and never unlinks another process's endpoint.

## Authoritative deliverables

| ID | Deliverable | Primary paths | Acceptance criteria | Required validation |
| --- | --- | --- | --- | --- |
| AF1-D0 | Design-review decision record | `docs/adr/ADR-002-host-wide-daemon-singleton.md`, `docs/plans/phase-af/af-1-runtime-path-inventory.md` | Before D1–D6 implementation begins, ADR-002 records the approved macOS/Linux/Windows persistent OS-account-derived runtime-root strategy, permission/symlink checks, state-root mismatch code, and migration from `ATM_HOME/.atm/daemon`. The inventory classifies every `ATM_HOME`, `ATM_DAEMON_SOCKET`, and direct-daemon call site as retained state configuration, rejected override, or removed. | Reviewer can inspect ADR-002 and the checked-in inventory; an inventory-to-`rg` reconciliation has no unclassified runtime admission call site. |
| AF1-D1 | Shared `HostRuntimeScope` boundary | `crates/atm-core/src/home.rs` (or dedicated runtime module) | Implements D0's approved OS-user scope, state-root mismatch behavior, and no-override contract. All runtime path derivation is centralized behind `current_host_runtime_scope`. | Unit tests cover path construction without consulting forbidden environment variables; source inventory identifies every former `host_runtime_lock_path*` user. |
| AF1-D2 | Client admission uses the shared scope | `crates/atm-daemon-client/src/lib.rs`, `crates/atm/src/composition.rs` | Client acquires D1's launch lock, connects to the canonical endpoint when an owner exists, and never spawns after a contended/rejected admission. `ATM_HOME` cannot create another gate. | Concurrent-client process test: home A and home B result in one owner PID and one endpoint. |
| AF1-D3 | Daemon admission and bind use the shared scope | `crates/atm-daemon/src/host_ownership.rs`, `crates/atm-daemon/src/local_ipc_transport.rs`, `crates/atm-core/src/protocol.rs` | Direct daemon execution obtains the same owner lock and endpoint as D2. A second direct executable exits typed before bind. Owner metadata prevents accidental lock/socket cleanup by non-owner processes. | Start direct daemon A, start direct daemon B under a different home, assert B's typed rejection, A remains healthy, and process count is one. |
| AF1-D4 | Runtime state mismatch is fail-closed | daemon bootstrap, request admission/health protocol, doctor projection | A client asking for a different daemon state root is told the active root fingerprint is incompatible; it neither creates a daemon nor mutates the active database. The message contains no private filesystem path unless doctor already permits it. | Process test from homes A/B: one daemon, B gets the documented mismatch code, and A's database is unchanged. |
| AF1-D5 | Production-equivalent test and lint gates | `scripts/lint_daemon_singleton.py`, `scripts/smoke/run_thorough_shared_host.py`, CI workflow | Static gates scan Rust, Python, shell, and CI launch sites for alternate daemon roots/endpoints. Smoke harness cannot leave a daemon and contains no bypass. Tests run as an isolated OS user/CI host rather than using a runtime test escape hatch. | Grep/lint gate passes; controlled full-smoke process test reports one PID before/during/after; cleanup assertion is mandatory. |
| AF1-D6 | Reliable lifecycle cleanup | daemon lifecycle/shutdown modules and integration tests | Graceful termination releases owned resources within the documented timeout; ungraceful death permits safe takeover only after ownership verification. No test relies on `SIGKILL` as normal cleanup. | TERM integration test, stale-owner recovery test, and repeat start/stop loop leave zero owned lock/socket artifacts. |

## Paths to delete or replace

| Retired path | Required replacement / proof |
| --- | --- |
| Per-`ATM_HOME` launch and owner-lock derivation through `host_runtime_lock_path*` | D1's `current_host_runtime_scope`; no caller may derive an admission lock from `ATM_HOME`. |
| Per-`ATM_HOME` daemon endpoint selection through `daemon_socket_path_from_home` | D1's fixed singleton endpoint; state-root selection is admitted separately by D4. |
| `launch_gate_isolated_per_atm_home_root` acceptance test and equivalent fixture assumptions | D2/D3 process test proving cross-home second launch is rejected and leaves one owner. |

## Non-closure

AF-1 does not change doctor hook output, generic connection-worker logging,
or wrapper/documentation policy. It may add only the admission diagnostics
needed to make the singleton contract testable.
