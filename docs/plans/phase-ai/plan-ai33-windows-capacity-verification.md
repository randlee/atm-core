---
title: AI.33 Windows admission-capacity verification
status: proposed
scope: fastpc4/cwin local verification
relates_to: AI.33, ADR-026, ADR-041, ADR-044
---

# AI.33 Windows admission-capacity verification

## Purpose

Establish whether the public Windows loopback-TCP admission path meets the
same AI.33 one-second capacity gate as the macOS local path. This is a test
and evidence plan, not permission to relax the gate or alter a production ATM
database.

## Preconditions

1. Work from a clean worktree at the exact branch commit under review. Record
   its commit, `atm.exe` version, and `atm-daemon.exe` version.
2. Repair or explicitly report an unavailable `just` executable before
   interpreting test results. The normal commands below must run through the
   repository recipes, not a hand-selected subset.
3. Use the designated clean Windows OS account. The normal user account's
   host-owned `.atm` state is never a benchmark database. If its state must be
   temporarily preserved to reach the clean account, restore it before and
   after the run and record that fact.
4. Confirm no ambient `atm-daemon.exe` is running before any runner-owned
   daemon starts.

## Required sequence

Run in this exact order. Stop on the first failure and retain the resulting
evidence; do not substitute a longer timeout, retry, larger queue, or extra
workers.

| Step | Command | Required evidence |
| --- | --- | --- |
| 1 | `just test` | Complete Windows test result and exact commit. |
| 2 | `just smoke localhost` | Generated HTML/JSON report showing the physical-interface self send/read and required-ack rows. |
| 3 | `ATM_CAPACITY_ISOLATED_OS_USER=1 python scripts/smoke/run_admission_capacity.py` | JSON artifact with ten accepting-peer and ten unavailable-peer one-second intervals. |

The capacity runner starts exactly one release-built branch daemon and uses a
disposable SQLite store. A successful interval has both 1,000 accepted writes
and 1,000 responses in at most one second. All twenty intervals must pass.

## Failure handling

If the release daemon fails to publish readiness, collect before termination:

- process ID, liveness, thread count, CPU time, and loopback listeners;
- host runtime files (`daemon/local-http.json`, owner locks, SQLite files) and
  retained daemon log; and
- bounded stdout/stderr from the runner-owned daemon.

Then terminate only the runner-owned process and restore any preserved user
state. Classify a readiness failure separately from a throughput failure: no
admissions result means no capacity claim is possible.

## Acceptance

The Windows result is PASS only when all three sequence steps pass and the
capacity JSON proves every required interval. The report must name the source
worktree and commit so it cannot be confused with a different daemon/CLI pair.
