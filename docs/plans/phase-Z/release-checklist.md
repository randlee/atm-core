# Phase Z Release Checklist

## Purpose

Authoritative final executable validation and release checklist for `Z.4`.

## Record Schema

Each checklist row must record:

- `validation_id`
- `flow_or_gate`
- `expected_result`
- `verdict`
- `evidence`
- `notes`

## Required Gate Coverage

The final release checklist must include:

- final rerun of the approved executable validation set, which consists of:
  - the promoted `Z.1` / `Z.2` smoke coverage represented by
    `docs/plans/phase-Z/smoke-checklist.md`
  - the promoted `Z.3` operator-facing canary coverage represented by
    `docs/plans/phase-Z/canary-dogfood-checklist.md`
- confirmation, via `docs/plans/phase-Z/canary-findings-ledger.md`, that every
  `Z.3` finding is either fixed or explicitly deferred and that every deferred
  row records `team-lead` approval before the release verdict may become final
- confirmation that the release verdict in `docs/plans/phase-Z/readiness.md`
  references this checklist result

## Final Checklist Result

| validation_id | flow_or_gate | expected_result | verdict | evidence | notes |
| --- | --- | --- | --- | --- | --- |
| `REL-001` | release build baseline | `cargo build --release` passes on the closeout branch | `PASS` | release smoke binaries rebuilt successfully before validation on `feature/pZ-smoke-atm-graft @ 84935774` | release executable baseline rebuilt successfully before sign-off |
| `REL-002` | workspace regression suite | `cargo test --workspace` passes on the closeout branch | `PASS` | `cargo test --workspace` PASS on `feature/pZ-smoke-atm-graft @ 84935774` | workspace tests stayed green after the normal-lane retained-log contract fix |
| `REL-003` | repository lint gates | `python3 .just/run_lint.py all` passes on the closeout branch | `PASS` | `python3 .just/run_lint.py all` PASS on `feature/pZ-smoke-atm-graft @ 84935774` | release closeout branch remains lint-clean |
| `REL-004` | fast smoke lane | `just smoke fast` / `python3 scripts/smoke/run.py fast --write-artifacts` passes | `PASS` | `reports/smoke/smoke-fast.md` (`pass=7 fail=0 skip=0`) | clean-room happy-path release smoke stayed green |
| `REL-005` | normal smoke lane | `just smoke` / `python3 scripts/smoke/run.py normal --write-artifacts` passes with no retained-log severity regressions outside the accepted invalid-ack contract | `PASS` | `reports/smoke/smoke.md` (`pass=8 fail=0 skip=0`) | the normal-lane analyzer now allows the one expected `ATM_MESSAGE_VALIDATION_FAILED` invalid-ack record and still rejects any unexpected warning/error output |
| `REL-006` | thorough smoke lane | `just smoke thorough` / `python3 scripts/smoke/run.py thorough --write-artifacts` passes | `PASS` | `reports/smoke/smoke-thorough.md` (`pass=13 fail=0 skip=0`) | thorough smoke now includes the real same-host `atm-graft` ICD lane and remains green |
| `REL-007` | frozen `Z.3` canary findings closure | every validated `Z.3` finding is fixed or explicitly deferred | `PASS` | `docs/plans/phase-Z/canary-findings-ledger.md` | `Z.17` promoted no validated `Z.3` canary findings into `Z.4` |
| `REL-008` | newly discovered `Z.4` issues are recorded honestly | any new out-of-scope release blocker is recorded before verdict finalization | `PASS` | `docs/plans/phase-Z/canary-findings-ledger.md` row `Z4-OOS-001` | the smoke-harness contract defect was recorded honestly and then fixed in `Z.4`; no unresolved out-of-scope release blocker remains |
| `REL-009` | coverage-report prerequisite | `Z.23` is complete before final sign-off | `PASS` | `docs/plans/phase-Z/readiness.md` row `Z.23` | coverage reporting line is closed and available for release evidence |
| `REL-010` | retained-log maintenance prerequisite | `Z.24` is complete before final sign-off | `PASS` | `docs/plans/phase-Z/readiness.md` row `Z.24` | retained-log maintenance adoption is closed on the accepted `1.1.0` observability line |

Final checklist verdict: `PASS`

- final release-signoff gates are green on `feature/pZ-smoke-atm-graft @ 84935774`
- `docs/plans/phase-Z/readiness.md` may record a release-ready verdict for this closeout line
