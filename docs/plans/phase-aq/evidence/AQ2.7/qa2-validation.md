# AQ2.7 validation evidence

Date: 2026-08-27  
Branch: `feature/aq-2-7-herdr-queue-wake`  
PR: #1056, target `integrate/phase-aq`

## Merge-forward checkpoint

`origin/integrate/phase-aq` was merged at `a3c935639` in merge commit
`64752cb91`. Its sole conflict was a comment-only difference in
`crates/atm-graft/src/runtime/mod.rs`, resolved with integrate's version. The
earlier correction took the integrate side wholesale for the three graft files
named in `merge-forward.md`, plus the dependent CLI caller. This keeps the
retired file-record publication path removed while retaining the AQ2.7
implementation files in Herdr, HTTP runtime, and daemon bootstrap.

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
| `cargo test -p atm-graft` | PASS — 36 tests, including bare-workspace activation |
| `cargo test -p agent-team-mail` | PASS — 194 tests |
| `cargo test --workspace` | PASS — all workspace tests, including the receiver registry retirement guard |

The prior workspace failure in
`atm-architecture::receiver_registry_ownership_has_one_flock_owner_and_no_file_publication`
was caused by the sibling merge reintroducing file-record references. The
integrate-side graft correction resolves that mismatch without weakening or
editing the architectural guard.

## QA-4 retry-partition run

Command:

```text
cargo test -p atm-http-runtime herdr_queue_wake::tests::ac06 -- --nocapture
```

Result: PASS — three tests passed. The blocked and not-present-family
post-claim fixtures retain attempt 0 and their pending markers; the bounded
fixture records release counters 9 and 10, then requeues on outcome 11 with
attempt 1 and a reset counter.

The deterministic blocked-dialog transcript is recorded in
[`ac06-blocked-race-fake-herdr-transcript.md`](ac06-blocked-race-fake-herdr-transcript.md).

AQ3 drains tmux / graft-only remains a downstream integrate verification after
#1054 merges (AQ3 tests …); it is not claimed as complete here.

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
