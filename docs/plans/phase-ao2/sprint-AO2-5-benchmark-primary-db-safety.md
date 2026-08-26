---
phase: AO2
sprint: AO2.5
title: Physical benchmark primary-database safety
branch: fix/benchmark-primary-db-safety
integration_branch: integrate/phase-ao2
status: complete
depends_on:
  - PR-977-scoped-live-database-guard
blocks:
  - AO2.6-admission-writer-batching-regression-live-evidence
---

# AO2.5 — Physical benchmark primary-database safety

## Decision summary

The physical admission benchmark must never rename, replace, delete, restore,
or otherwise mutate the current interactive OS user's `~/.atm/db` tree. It
must run only as a dedicated benchmark OS user whose canonical
`HostRuntimeScope` is intentionally disposable. A durable, verified snapshot
of that benchmark user's state is required before any destructive reset or
restore in that account. A snapshot is evidence and recovery material; it is
not authorization to touch the primary database.

## Problem statement and root cause

`scripts/smoke/run_admission_capacity.py` formerly implemented the
`ATM_CAPACITY_BACKUP_RESTORE_HOST_STATE=1` path by renaming the current OS
user's `~/.atm/db` to a transient sibling and creating an empty replacement.
The recovery path then removed that replacement and renamed the transient
directory back. This is a directory swap, not a durable backup: interruption,
operator error, or a later move can leave the primary state absent or stale.
It caused the 2026-08-21 M4 data-loss incident.

PR #977 is the immediate containment change: it removes the public
managed-host flags and makes the runner refuse both the environment escape
hatch and every managed-daemon lifecycle before it can call `daemon-switch`.
It does not implement the durable benchmark-account workflow described here.

ADR-026 requires a single non-configurable `HostRuntimeScope` for each OS
user. An `ATM_HOME` override is not, and must never become, a way to select a
different SQLite root. The design must therefore isolate by OS account, not by
workspace path, symlink, environment variable, or an extra daemon endpoint.

## Scope and non-goals

In scope:

- an explicit, cross-platform operator workflow for a disposable benchmark OS
  account;
- durable snapshot, verification, and restore of that account's canonical
  SQLite state;
- benchmark harness preflight and evidence proving which OS account and state
  policy were used;
- failure-safe cleanup and operator recovery instructions.

Out of scope:

- any move, replacement, or restart of the interactive user's ATM database or
  daemon;
- alternate `HostRuntimeScope` roots, endpoint overrides, or a second daemon
  for one OS user;
- TLS, HTTP, writer batching, or benchmark throughput changes;
- automatic recovery of the already-lost M4 data.

## Required design

### 1. Dedicated benchmark account contract

Introduce a checked benchmark-account manifest owned by the account bootstrap
workflow. It records only portable facts: account home, numeric account ID,
canonical durable-state path, creation time, and a random account-local token.
The runner validates all of them against the executing process before it can
open a database or start a daemon. The account name is an operator parameter,
never a hardcoded hostname or username.

The runner fails closed when:

- the manifest is absent, malformed, symlinked, not owned by the executing
  account, or does not match its home/UID;
- an ambient ATM daemon belongs to that account before a clean-run preflight;
- its canonical durable-state root contains data without a completed verified
  snapshot; or
- the process is the interactive account, as established by the bootstrap
  manifest rather than an environment assertion.

The physical invocation launches the released CLI and Tokio/Axum
`atm-http-runtime` daemon as that benchmark account. It uses the ordinary
ADR-026 root for that user. No benchmark argument changes the durable root,
lock path, HTTP endpoint, or daemon ownership semantics.

### 2. Durable benchmark-account snapshot protocol

Before resetting or restoring the disposable account, the tool must:

1. Stop only the benchmark account's daemon through the normal paired
   `daemon-switch` workflow and verify it is stopped.
2. Create a SQLite-consistent snapshot in a sibling staging directory using
   the SQLite backup API (never directory rename), then fsync the snapshot and
   its manifest.
3. Verify `PRAGMA quick_check`, schema version, page count, byte count, and
   SHA-256 recorded in the manifest.
4. Atomically publish the completed snapshot manifest only after verification.
   Incomplete staging material is never a restore candidate.
5. On restore, verify the snapshot again, stage a replacement only within the
   benchmark account's durable-state parent, atomically activate it, restart
   that same account's selected CLI/daemon pair via `daemon-switch`, and prove
   `atm doctor --json` plus a loopback send/read.

Every step reports machine-readable evidence with snapshot ID, account UID,
hash, verification result, daemon pair, and rollback status. The cleanup path
must preserve failed staging material for diagnosis; it must not silently
delete the last verified snapshot.

### 3. Primary-data backup policy

The benchmark command never backs up or restores the interactive primary
database. AO2.5 will separately document the supported operator-owned whole
host backup/recovery command and its verification contract, because the
existing team backup is a selected-team recovery surface rather than a
benchmark safety mechanism. That work requires an ADR/requirements decision
before implementation. No physical benchmark is unblocked by an unverified
primary snapshot.

## Performance-regression prevention

The risk is accidental benchmark distortion: synchronous copying, hashing,
fsync, database checkpoints, lock contention, or daemon startup inside the
timed admission profile can lower measured throughput and be mistaken for an
application regression.

Mitigation:

- perform snapshot/verification, reset, daemon startup, and post-run restore
  outside the timed profile;
- timestamp each phase in raw evidence and exclude it from `run_profile`;
- run the released daemon with the existing public HTTP admission boundary;
- add no storage, TLS, or HTTP work to a timed write; and
- fail a run rather than reuse a dirty account or silently skip verification.

Measurement is required on the same fixed harness and hardware for
`just benchmark --target tcp` (plaintext-test) and
`just benchmark --target tcp-tls` (mutual TLS). Retain raw and compact
artifacts, report p50/p95/p99/throughput and snapshot phase durations, and
compare plaintext f64 throughput to the approved approximately 17k msg/s
same-host baseline. Snapshot work must not appear within the measured interval;
if it does, the result is invalid rather than a performance result.

## Work breakdown and dependencies

1. **AO2.5.1 — Requirements/ADR decision** (no code): reconcile ADR-026 and
   the retained backup requirements with the benchmark-account model; explicitly
   forbid live-root benchmark mutation. Blocks all implementation.
2. **AO2.5.2 — Account bootstrap and manifest validation:** implement the
   portable account-local manifest and fail-closed preflight. Depends on 1.
3. **AO2.5.3 — Snapshot/restore transaction:** implement the verified SQLite
   snapshot/staging/atomic-activation protocol for the benchmark account only.
   Depends on 1 and 2.
4. **AO2.5.4 — Harness integration and evidence schema:** make `just benchmark`
   use the account workflow and retain phase evidence outside timed samples.
   Depends on 2, 3, and AO2.5.3b's reviewed `daemon-switch` typed temporary
   launch-overlay capability.
5. **AO2.5.5 — Physical proof and rollback drill:** run plaintext and mTLS on
   M4, M5, and Windows when available; restore the benchmark account after an
   injected failure. Depends on 4.

### AO2.5.4 daemon-switch dependency

The cross-platform managed-service launch overlay is intentionally owned by
[AO2.5.3b](./sprint-AO2-5-3b-daemon-switch-launch-overlay.md), not by the
benchmark harness.  AO2.5.4 may consume only its reviewed typed session API
and returned evidence.  It must not start a child daemon, alter a platform
service file, set an environment selector, choose an alternate endpoint/root,
or pass arbitrary daemon arguments.  Snapshot, roster setup, daemon
transitions, and restore remain timed phases with monotonic start/end
timestamps; `run_profile` starts only after the post-snapshot doctor proof and
ends before quiesce/restore.

## Acceptance criteria and test matrix

| Case | Required proof |
| --- | --- |
| Interactive account | Preflight refuses before `daemon-switch`, SQLite open, rename, remove, or daemon start. |
| Missing/tampered manifest | Preflight fails with recovery guidance and no state mutation. |
| Snapshot success | Hash, SQLite integrity, manifest fsync/publication, and retained evidence pass. |
| Interrupted snapshot | No completed manifest; restore refuses it; last completed snapshot remains intact. |
| Restore success | Only benchmark-account files change; doctor and loopback send/read pass after paired restart. |
| Restore failure | Daemon remains stopped or returns to last verified snapshot; evidence names the recovery action. |
| Timed benchmark | Plaintext and mTLS artifacts show setup/restore outside the timed interval. |
| Physical platforms | M4, M5, and fastpc4/Windows evidence records the benchmark account and result. |

Required gates: focused unit tests, `just lint`, `just test`, static search
guard against primary-root move/delete operations in the harness, then a live
benchmark-account smoke before QA. QA receives raw evidence and exact commit,
not only a textual claim.

## Rollback and recovery

Implementation rollback is a normal code revert: PR #977's hard refusal stays
in place. Operational rollback stops the benchmark account daemon, restores
only its last completed verified snapshot, verifies integrity, then runs paired
`daemon-switch` plus doctor and loopback proof. A failure never triggers a
primary-user restore attempt. Operators receive the preserved snapshot/staging
paths and the exact failed phase.

## Boundary and ADR impact review

- **`atm-http-runtime`:** used only as the ordinary released daemon target;
  this sprint must not modify its HTTP or TLS admission path.
- **`atm-storage-rusqlite`:** snapshot implementation may use SQLite backup
  facilities, but must not alter the writer hot path.
- **daemon/CLI boundary:** paired start/stop remains `daemon-switch` for the
  benchmark account only; no new controller or second daemon design.
- **ADR-026:** requires an ADR/requirements clarification before code because
  the account manifest and durable snapshot policy are operational policy,
  while the single-root invariant remains unchanged.
