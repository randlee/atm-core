# AL.9 local daemon-switch smoke attempt — 2026-08-07

## Outcome

**Blocked before smoke.** `just smoke thorough` was not run because neither
attempt to switch the managed daemon to the AL.9 release build reached the
required healthy `atm doctor --json` gate. The system was verified healthy on
the restored AJ pair after each failed attempt.

This is an evidence of a failed physical setup gate, not a smoke PASS result.
It must not be substituted for `reports/smoke/smoke-thorough.md`.

## Scope and preconditions

- Evidence branch: `evidence/al9-local-smoke`.
- Initial remote update: `be01a3a0089f599b86616a24645621043fd2ac96`.
- Built release pair: `target/release/atm` and `target/release/atm-daemon`.
- Built workspace version: `1.4.1-beta-ai-1`.
- Managed LaunchAgent confirmed before each switch:
  - label: `com.atm.daemon.crosshost-smoke`
  - plist: `~/Library/LaunchAgents/com.atm.daemon.crosshost-smoke.plist`
- The pre-switch daemon-switch status/doctor gate was healthy on the matched
  installed AJ pair (`1.4.1-beta-aj`).

The requested thorough lane is local-only: frozen local rows plus the
same-host `atm-graft` advisory and unary ICD lane. Cross-host smoke was not in
scope for this run.

## Attempt 1: observability health failure

The documented paired selector switch was attempted with the confirmed
LaunchAgent label and plist. The daemon-switch script failed closed because
the selected `1.4.1-beta-ai-1` pair reported:

```text
ATM_OBSERVABILITY_HEALTH_FAILED
shared observability is unavailable; ... observability adapter is not configured
```

### Root cause

`atm-daemon-bootstrap::run_replacement_daemon_with_selector` constructed the
real `StorageAndNudgeRouter` with `NullObservability`. `NullObservability` is
the no-op fallback and deliberately reports unavailable health. The former
daemon binary bootstrapped `DaemonObservability`, but AL.9's replacement
entrypoint bypassed that adapter.

### Fixes made

1. Commit `e7a85570` (subsequently merged with the concurrent release lock as
   `7ee35d121e9379466c8ec44a18dd49328b8dff1a`) restores process-owned retained
   `DaemonObservability` at the binary entrypoint and passes it to the
   replacement bootstrap through the core `ObservabilityPort` boundary.
2. The retained adapter was decoupled from the retired internal daemon trait;
   the active replacement runtime consumes only the sealed core
   `ObservabilityPort` boundary.
3. `cargo clippy -p atm-daemon -- -D warnings` and the release build passed
   for that revision.

## Attempt 2: daemon startup/lifecycle failures

The second switch did not reach doctor: the selected CLI reported
`daemon=<missing>`.

### Runtime assembly root cause and fix

The managed daemon stderr showed repeated:

```text
daemon runtime assembly is unavailable; atm-daemon startup is blocked
```

The previous daemon entrypoint called
`install_sqlite_retained_runtime_factory()` before assembling the host runtime;
the replacement path did not. Commit
`2167eafdb8565471ce96714b577ae6ea53153a06` restores that initialization at the
common replacement-daemon startup boundary. It was release-built and checked
with:

```text
cargo clippy -p atm-daemon-bootstrap -p atm-daemon -- -D warnings
cargo build --release -p agent-team-mail -p atm-daemon
```

### Socket ownership and repair attempt

The remaining startup output was:

```text
Unix HTTP socket path is already occupied
```

Before invoking repair, ownership was proven rather than assumed:

- socket: `~/.atm/daemon/atm-daemon.sock`
- holder at inspection time: PID `25717`, executable
  `/opt/homebrew/bin/atm-daemon`
- LaunchAgent label: `com.atm.daemon.crosshost-smoke`

That is the one managed daemon selected by the confirmed plist, so the
documented paired switch was retried with `--repair-orphan`. It still failed
with `daemon=<missing>` and the socket-occupied error. No unrelated process
was terminated manually.

## Follow-up root-cause: controlled-stop socket cleanup

The replacement runtime's Unix listener is protected by an inode-bound
`UnixSocketPathGuard`; its normal drain path drops that guard after joining
the server task, so it unlinks only the inode it created. The host's currently
installed AJ daemon, however, completed a controlled shutdown while leaving
the canonical socket pathname behind. The original daemon-switch check only
looked for an owning PID. It could therefore proceed when no process owned the
socket even though the pathname remained, at which point the replacement
runtime correctly refused to overwrite it.

The pending daemon-switch hardening changes:

- wait for the SIGTERM'd, proven orphan to disappear *and* for the socket path
  to disappear;
- remove a remaining path only if it is still a Unix socket, owned by the
  current user, and (for orphan recovery) matches the inode proven to have
  belonged to that daemon; and
- apply the same guarded cleanup after an otherwise successful controlled
  stop, before changing either selector.

A retry then passed the former socket-owner stop point, but surfaced a second
independent issue: `launchctl bootstrap` reports the job loaded before the
daemon is doctor-ready. The script now polls matched CLI/daemon doctor
versions for up to five seconds rather than immediately rolling back on the
first unavailable response. That bounded poll still exhausted: the evidence
daemon continued to report `daemon=<missing>`, after which the script restored
the AJ pair and its doctor check was healthy. This is not a smoke pass.

## Restore and current system state

After every unsuccessful switch, the daemon-switch `restore` command itself
reported a stale recorded target:

```text
target atm CLI does not exist: ~/.atm/releases/1.3.2-beta.1/atm
```

Read-only verification nevertheless showed both selectors back at the matched
AJ build under `integrate/phase-aj`, and `daemon-switch.py status --doctor`
reported a healthy live daemon. The last confirmed healthy daemon PID was
`26141` under `com.atm.daemon.crosshost-smoke`.

## Required follow-up before retrying smoke

1. Diagnose why the managed restart leaves the UDS inode occupied even after
   the authorized repair-orphan path; do not delete or replace that socket
   outside the daemon lifecycle contract.
2. Repair or re-provision the daemon-switch restore default separately; its
   recorded `1.3.2-beta.1` target is stale.
3. After a paired switch reaches a healthy doctor, run `just smoke thorough`,
   then restore and re-run doctor before recording a smoke verdict.
