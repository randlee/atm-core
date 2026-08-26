---
phase: AO2
sprint: AO2.5.4
title: Mandatory clean-baseline snapshot and restore for physical benchmarks
branch: future-dev-worktree
integration_branch: integrate/phase-ao2
status: complete
depends_on:
  - AO2.5.1-benchmark-account-contract
  - AO2.5.2-benchmark-account-preflight
  - AO2.5.3-verified-snapshot-transaction
  - PR-977-primary-database-refusal
blocks:
  - AO2.6-admission-writer-batching-regression
  - AO2.7-m5-tcp-benchmark-parity
  - AO2.8-windows-tcp-benchmark-parity
---

# AO2.5.4 — Mandatory clean-baseline snapshot and restore for physical benchmarks

## Decision

`just benchmark` must run only under the already-validated disposable
benchmark OS account and must create a verified SQLite snapshot of that
account's clean canonical database before creating any benchmark roster or
timed workload. Every exit path after snapshot publication must stop the
benchmark daemon, restore that exact snapshot, verify the restored database,
and retain phase evidence. The interactive user's ATM database is never a
source, destination, fixture, backup, or restore target.

The snapshot is taken before the roster so the post-run account is restored to
the same clean state with no benchmark teams, messages, or receipts. A
post-restore loopback `send` is forbidden: it would contaminate the very
baseline this sprint proves. A read-only doctor and database-fact verification
provide the post-restore health evidence instead.

## Grounded current state

- `scripts/smoke/benchmark_account.py` already owns the disposable-account
  manifest and fail-closed identity/root validation.
- `scripts/smoke/benchmark_snapshot.py` already owns SQLite-backup snapshot
  creation, manifest/hash/`quick_check` verification, and account-local atomic
  restore.
- `scripts/smoke/run_admission_capacity.py` currently invokes account
  preflight but does not invoke the snapshot module around each run.
- PR #977 already refuses the retired managed-daemon/primary-user benchmark
  mode before daemon, SQLite, or filesystem mutation.

This sprint wires existing safety primitives into the benchmark lifecycle. It
does not create a new daemon, introduce an alternate `HostRuntimeScope`, or
depend on the deferred daemon-switch overlay work.

## Required lifecycle

The runner must make the following state machine explicit in raw evidence:

1. Validate the benchmark-account manifest before creating an `ATM_HOME`,
   opening SQLite, starting a daemon, or deleting any path.
2. Create the normal disposable runtime, start the released Tokio/Axum daemon,
   and obtain a read-only healthy doctor result.
3. Stop/quiesce only that owned benchmark daemon and prove SQLite sidecars are
   absent; create and verify a clean-baseline snapshot. This occurs before
   roster creation.
4. Restart the same owned daemon and doctor-check it. Create the unique
   benchmark roster, then run the existing public HTTP profile unchanged.
5. Stop/reap the owned daemon. Restore the exact verified snapshot, revalidate
   its hash/SQLite facts, restart the owned daemon, and perform read-only
   doctor/database-fact checks proving the clean baseline is active.
6. Stop/reap the owned daemon and remove only the per-run temporary runtime
   directory. Retain the completed snapshot and raw evidence under the
   benchmark account for recovery/audit.

No timed sample may include snapshot, validation, daemon start/stop, roster
setup, restore, cleanup, hashing, or fsync work. The timed request profile and
its transport semantics must remain byte-for-byte and behaviorally unchanged.

## Failure and recovery contract

- A failed preflight must have no side effect.
- A failed snapshot must prevent roster creation and timed work; incomplete
  staging material is retained and cannot be restored.
- A profile failure still enters the stop-and-restore path.
- If stop, sidecar absence, restore, post-restore verification, or cleanup
  fails, the command fails non-zero, retains raw evidence and staged material,
  and never deletes a last verified snapshot.
- The runner must never attempt a compensating action on the interactive
  account, even if the benchmark-account restore fails.

Each new evidence failure must name a stable phase (`preflight`, `snapshot`,
`profile`, `stop`, `restore`, `post_restore_verify`, or `cleanup`), preserve
its underlying cause, and state the safe recovery action. This is the required
`RBP-001` error-context/recovery review point for the Python operator surface;
opaque catch-all failure text is not sufficient.

## Implementation boundaries

Allowed files are the benchmark Python runner, its unit tests, raw-evidence
schema/validation, and benchmark documentation. The implementation must call
the existing `benchmark_account` and `benchmark_snapshot` public APIs rather
than duplicate SQLite backup, hash, manifest, or restore logic.

Out of scope:

- Rust daemon, Tokio/Axum router, SQLite writer, TLS, and HTTP pipeline code;
- daemon-switch overlay work, managed-service mutation, or a child daemon for
  the interactive account;
- changes to the timed benchmark request, connection, concurrency, message,
  roster, or comparison policy;
- whole-host backup or recovery of the interactive ATM database.

## Work items and dependency graph

1. Add a small lifecycle owner in `run_admission_capacity.py` that records
   snapshot identifier and phase monotonic timestamps, and executes the
   required stop/snapshot/restart and stop/restore/restart transitions.
2. Extend raw evidence with account identity (redacted as already required),
   snapshot ID, verification facts, and each setup/teardown duration. The
   compact throughput report must not treat those durations as samples.
3. Add deterministic fault-injection tests for preflight refusal, snapshot
   failure, profile failure, daemon-stop failure, restore failure, and success.
   Each test asserts the interactive root was not inspected or mutated.
4. Add a live disposable-account smoke that proves snapshot-before-roster and
   restore-to-clean-baseline, then submit the raw artifact for QA.

Items 1–3 are one atomic implementation/review unit. Item 4 starts only after
the code review passes.

## Acceptance criteria

| Case | Required proof |
| --- | --- |
| Normal run | A snapshot ID is published before roster creation; restore succeeds after the profile. |
| Profile failure | The owned daemon is stopped and the same snapshot is restored. |
| Restore failure | Non-zero exit, retained staging/evidence, no destructive fallback. |
| Interactive account | Refusal occurs before SQLite, daemon, or filesystem mutation. |
| Timing isolation | Raw phase timestamps prove no setup/restore work overlaps a timed sample. |
| Clean baseline | Post-restore doctor plus read-only database facts show the pre-roster snapshot is active. |

Required gates are focused Python tests, `just lint`, `just test`, a static
search proving no primary-root rename/remove/replace path is reachable from
the runner, and the live disposable-account smoke. QA receives the exact
commit and raw evidence artifact.

## Rollback

Rollback is a normal code revert. PR #977's primary-database refusal is
independent and must remain. An interrupted benchmark account is recovered by
stopping only its owned daemon and invoking the existing verified-snapshot
restore procedure; operators must not copy, rename, or restore the interactive
account as part of this recovery.

## Current operator procedure

For any new benchmark run, follow the canonical
[`benchmark-run` skill](../../../.claude/skills/benchmark-run/SKILL.md).
