# Lock Release Gate

Branch: `feature/pP-s10-schema-lock-fix`

Scope: `PP-S10-B` release gate only. This is not another hardening sprint.

Verdict: `PASS` for interim release use.

## Checklist

- [x] Stale/orphaned lock artifact does not wedge command flow.
- [x] Write contention fails or recovers in bounded time.
- [x] Read-only paths work under contention.
- [x] Crash/restart-style stale lock recovery does not rely solely on the 5-minute cron sweep.
- [x] `send -> ack -> clear` remains operable after a lock fault.

## Evidence

### 1. Stale/orphaned lock artifact does not wedge command flow

Evidence:
- `crates/atm-core/src/mailbox/lock.rs`
  - `acquire(...)` calls `evict_stale_lock_sentinel(...)` on each acquisition attempt before waiting on contention.
  - `sweep_stale_lock_sentinels(...)` exists as secondary cleanup, not the only recovery path.
- unit tests in `crates/atm-core/src/mailbox/lock.rs`
  - `evict_stale_lock_sentinel_removes_dead_pid_file`
  - `sweep_stale_lock_sentinels_removes_only_lock_files_with_dead_pids`
  - `sweep_stale_lock_sentinels_removes_rotated_dead_pid_sentinels_only`
  - `sweep_stale_lock_sentinels_skips_malformed_rotated_sentinels`

Assessment:
- Dead-PID stale sentinels are removed inline on acquisition, so an orphaned sentinel is not left to block progress until cron runs.

### 2. Write contention fails or recovers in bounded time

Evidence:
- `cargo test -p agent-team-mail-core --test mailbox_locking --quiet`
- integration tests in `crates/atm-core/tests/mailbox_locking.rs`
  - `send_times_out_under_bounded_lock_contention`
  - `send_reports_non_contention_lock_failures_without_timeout`
  - `concurrent_ack_on_overlapping_inbox_sets_completes_without_deadlock`
  - `concurrent_send_with_ack_and_clear_completes_without_deadlock_or_data_loss`
- unit tests in `crates/atm-core/src/mailbox/lock.rs`
  - `acquire_reports_mailbox_lock_timeout_code`
  - `acquire_many_sorted_uses_total_timeout_budget`
  - `acquire_many_sorted_releases_prior_guards_on_failure`

Assessment:
- Mutating paths either complete or return `MailboxLockTimeout` / `MailboxLockFailed` within a bounded window.

### 3. Read-only paths work under contention

Evidence:
- `cargo test -p agent-team-mail-core --test mailbox_locking --quiet`
- integration tests in `crates/atm-core/tests/mailbox_locking.rs`
  - `clear_dry_run_does_not_wait_on_mailbox_lock`
  - `read_possible_write_only_locks_when_display_mutation_is_required`

Assessment:
- Dry-run clear stays non-blocking under contention.
- Read only acquires locks when display mutation is required; no-mutation reads remain operable while a lock is held.

### 4. Crash/restart-style recovery does not rely solely on the cron sweep

Evidence:
- `crates/atm-core/src/mailbox/lock.rs`
  - `acquire(...)` performs inline stale-sentinel eviction before retrying contention.
  - `evict_stale_lock_sentinel(...)` distinguishes dead-PID sentinels from live holders.
- unit tests in `crates/atm-core/src/mailbox/lock.rs`
  - `evict_stale_lock_sentinel_removes_dead_pid_file`
  - `dropping_guard_tolerates_read_only_cleanup_failure`

Assessment:
- There is no dedicated kill-holder integration test in this gate.
- However, the runtime recovery path is not cron-only: a new command attempts stale-sentinel eviction itself before waiting on the lock.
- That is sufficient for an interim release gate, but it remains weaker than the future SQLite replacement.

### 5. `send -> ack -> clear` remains operable after a lock fault

Evidence:
- `cargo test -p agent-team-mail-core --test mailbox_locking --quiet`
- integration tests in `crates/atm-core/tests/mailbox_locking.rs`
  - `concurrent_send_with_ack_and_clear_completes_without_deadlock_or_data_loss`
  - `multi_source_read_and_clear_complete_without_deadlock`
  - `clear_remove_locked_inbox_seam_fails_closed_without_mutating_surviving_state`

Assessment:
- Command flow remains operable through mixed send/ack/clear activity.
- Failure paths fail closed without corrupting surviving state.

## Schema-Fix Regression Note

The `PP-S10-A` schema fix did not modify mailbox lock code. After that change, the lock suite still passed:

- `cargo test -p agent-team-mail-core --test mailbox_locking --quiet`

I attempted a direct `origin/develop` comparison run, but clean `develop` currently fails to compile due unrelated `send/hook.rs` config-field errors. So this gate establishes:

- no observed lock regression from the schema fix on this branch
- no clean same-suite `develop` baseline is currently available

## Release Decision

`PASS` for interim release use.

Reason:
- the current Phase P line shows bounded lock behavior, inline stale-sentinel recovery, non-blocking read-only paths, and mixed command operability under contention
- this is good enough to relieve the present lock problem as an interim release
- it does not change the longer-term direction away from mailbox-lock-centered correctness
