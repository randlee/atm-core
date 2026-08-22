# ADR-052 — Benchmark Account Isolation and Snapshot Policy

| Field | Value |
| --- | --- |
| Status | Accepted |
| Scope | Physical benchmark execution and benchmark-account recovery |
| Relates to | ADR-026, REQ-P-BENCHMARK-001, REQ-CORE-TRANSPORT-005B |

## Context

ADR-026 gives each OS user exactly one non-configurable
`HostRuntimeScope`, including one daemon endpoint, locks, and durable SQLite
root. `ATM_HOME` is configuration discovery only and cannot choose another
runtime or durable-state root.

The former physical admission benchmark nevertheless supported a
managed-host path that renamed the interactive user's canonical
`~/.atm/db` directory, substituted an empty database, then attempted to put
the directory back. That directory swap caused the 2026-08-21 M4 data-loss
incident. PR #977 retired the path and refuses it before daemon-switch or
state mutation.

The retained `atm teams backup` command captures selected-team recovery
material. It is not a verified whole-host SQLite backup and does not make the
interactive database safe to replace for benchmarking.

## Decision

Physical benchmarks run only under a dedicated, disposable benchmark OS
account. That account is a separate ADR-026 `HostRuntimeScope` by virtue of
being a separate OS user; it is not an alternate scope for the interactive
user. The account name is supplied by an operator/bootstrap workflow and is
never a hardcoded identity or hostname.

The benchmark runner must validate an account-local manifest against the
executing process before it can touch state or start a daemon. The manifest
binds the benchmark workflow to the account's UID, home, canonical durable
state path, and an account-local bootstrap token. Absence, malformed content,
symlink traversal, ownership mismatch, or a mismatched executing account is a
fail-closed preflight error.

Only the benchmark account's canonical durable state may be reset or restored.
Before destructive work, the runner creates a SQLite-consistent snapshot with
the SQLite backup API, verifies integrity and manifest metadata, and publishes
the manifest only after successful verification. A restore re-verifies the
published snapshot, stages changes only in the benchmark account's durable
state parent, and records machine-readable recovery evidence. Incomplete
staging material is diagnostic evidence, never a restore candidate.

The interactive account's canonical durable root is never a physical
benchmark fixture. A benchmark must refuse before SQLite open, daemon-switch,
rename, replacement, deletion, or daemon launch when it is not operating as
the validated benchmark account. It must not solve isolation with `ATM_HOME`,
an endpoint override, a symlink, a workspace path, or an additional daemon.

Snapshot, reset, daemon startup, post-run restore, and verification run
outside timed samples. Their evidence is retained separately; a result is
invalid if any such work is included in the measured profile.

This decision does not define a general backup/recovery command for the
interactive account. That capability, if needed, requires its own operator
requirements and ADR; team backup remains a selected-team recovery surface.

## Consequences

- A physical benchmark cannot clobber a live interactive ATM database.
- The ordinary released CLI and Tokio/Axum `atm-http-runtime` daemon continue
  to use the normal account-scoped endpoint and state path.
- Benchmark restoration is recoverable and inspectable without pretending
  that a directory rename is a backup.
- Benchmark setup cannot be confused with application throughput, preserving
  comparability of plaintext and mTLS evidence.

## Rejected alternatives

1. **Temporary `ATM_HOME` or an alternate database argument.** Rejected:
   ADR-026 intentionally prohibits alternate runtime roots for one OS user.
2. **Directory rename/copy as a primary-state backup.** Rejected: it is not a
   verified SQLite snapshot and can leave state absent or stale after failure.
3. **Use `atm teams backup` before replacing the database.** Rejected: it
   backs up selected-team recovery data, not the whole host database.
4. **Benchmark the interactive account but restore afterward.** Rejected:
   recovery material is not authorization to disrupt a live account.

## Required evidence

- The interactive-account preflight refuses before any daemon or SQLite
  operation.
- A valid benchmark account can snapshot, verify, reset, restore, and prove
  paired daemon health plus loopback delivery without touching another
  account's state.
- An interrupted snapshot cannot be restored and leaves the last verified
  snapshot available.
- Benchmark reports separately identify setup/restore durations and the timed
  admission profile.
