# AQ2.7 QA-2 validation evidence

Date: 2026-08-27  
Branch: `feature/aq-2-7-herdr-queue-wake`  
PR: #1056, target `integrate/phase-aq`

## Merge-forward checkpoint

The branch was pulled before merge-forward. These merge commits were made
without rebase or force push:

| Input | Result |
|---|---|
| `origin/integrate/phase-aq` | merged into the branch before the AQ2.7 fixes |
| `origin/feature/aq-1-6-graft-receiver-registration-client` | conflict resolved in `graft.rs`, `atm-graft/Cargo.toml`, and `atm-graft/src/runtime/mod.rs`; merge commit `4a8bd241d` |

The checkpoint was pushed before further edits. The final fix commit is
recorded in the completion message and PR.

## Focused QA-2 run

Command:

```text
cargo fmt --all
cargo test -p atm-http-runtime herdr_queue_wake::tests -- --nocapture
```

Result: PASS — 14 tests passed, including all 12 AC-named tests and the
exact `ac04_shutdown_send_stops_pump_before_drain_completes` test.

## Broad gates

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `python3 .just/run_lint.py all` | PASS — all 32 checks, including `sc-boundary` |
| `cargo test -p atm-herdr -p atm-daemon-bootstrap` | PASS — 13 + 30 tests |
| `cargo test --workspace` | BLOCKED by an upstream merge inconsistency described below |

The workspace run passed the CLI/core/storage suites and then failed at
`atm-architecture::receiver_registry_ownership_has_one_flock_owner_and_no_file_publication`.
The test is present in merge parent `842de908f` / `origin/integrate/phase-aq`
and requires AQ1.8's file-record retirement, while the requested AQ1.6 merge
parent `0736e8cf6` still supplies the file-record implementation. The failure
reports three production file-publication references. Resolving that requires
the separate AQ1.8 retirement merge; this AQ2.7 pass does not weaken or edit
that architectural guard.

## Scope and evidence notes

- Shutdown evidence uses a live `watch::Sender`, a gated fake prompt, bounded
  `JoinHandle` wait, and marker inspection after cancellation.
- Retry evidence distinguishes `HerdrPromptFailed` (requeue, attempt +1) from
  absent-target handling (release, no attempt debt).
- Burst and fairness evidence use 17 and 20 real roster members respectively,
  not constant-only assertions.
- No live Herdr process transcript was available in this deterministic test
  run; the adapter-boundary fixtures are the reproducible substitute and are
  explicitly identified as such.
