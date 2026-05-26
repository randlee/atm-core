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
    `docs/phase-Z/smoke-checklist.md`
  - the promoted `Z.3` operator-facing canary coverage represented by
    `docs/phase-Z/canary-dogfood-checklist.md`
- confirmation, via `docs/phase-Z/canary-findings-ledger.md`, that every
  `Z.3` finding is either fixed or explicitly deferred and that every deferred
  row records `team-lead` approval before the release verdict may become final
- confirmation that the release verdict in `docs/phase-Z/readiness.md`
  references this checklist result

## Final Checklist Result

| validation_id | flow_or_gate | expected_result | verdict | evidence | notes |
| --- | --- | --- | --- | --- | --- |
| `REL-001` | release build baseline | `cargo build --release` passes on the closeout branch | `PASS` | `cargo build --release` PASS on `feature/pZ-smoke-atm-graft @ 244b69ea` | release executable baseline rebuilt successfully before sign-off |
| `REL-002` | workspace regression suite | `cargo test --workspace` passes on the closeout branch | `PASS` | `cargo test --workspace` PASS on `feature/pZ-smoke-atm-graft @ 244b69ea` | workspace tests stayed green after the `atm-graft` thorough-smoke additions |
| `REL-003` | repository lint gates | `python3 .just/run_lint.py all` passes on the closeout branch | `PASS` | `python3 .just/run_lint.py all` PASS on `feature/pZ-smoke-atm-graft @ 244b69ea` | release closeout branch remains lint-clean |
| `REL-004` | fast smoke lane | `just smoke fast` / `python3 scripts/smoke/run.py fast --write-artifacts` passes | `PASS` | `reports/smoke/smoke-fast.md` (`pass=7 fail=0 skip=0`) | clean-room happy-path release smoke stayed green |
| `REL-005` | normal smoke lane | `just smoke` / `python3 scripts/smoke/run.py normal --write-artifacts` passes with no retained-log severity regressions | `FAIL` | `reports/smoke/smoke.md` (`pass=7 fail=1 skip=0`) | `FAST-LOG-002` fails because the normal lane logs the expected invalid-ack recovery path as `ATM_MESSAGE_VALIDATION_FAILED` at `Error` severity |
| `REL-006` | thorough smoke lane | `just smoke thorough` / `python3 scripts/smoke/run.py thorough --write-artifacts` passes | `PASS` | `reports/smoke/smoke-thorough.md` (`pass=13 fail=0 skip=0`) | thorough smoke now includes the real same-host `atm-graft` ICD lane and remains green |
| `REL-007` | frozen `Z.3` canary findings closure | every validated `Z.3` finding is fixed or explicitly deferred | `PASS` | `docs/phase-Z/canary-findings-ledger.md` | `Z.17` promoted no validated `Z.3` canary findings into `Z.4` |
| `REL-008` | newly discovered `Z.4` issues are recorded honestly | any new out-of-scope release blocker is recorded before verdict finalization | `FAIL` | `docs/phase-Z/canary-findings-ledger.md` row `Z4-OOS-001` | the new blocker is recorded, but it remains deferred without `team-lead` approval and therefore blocks a final `PASS` release verdict |
| `REL-009` | coverage-report prerequisite | `Z.23` is complete before final sign-off | `PASS` | `docs/phase-Z/readiness.md` row `Z.23` | coverage reporting line is closed and available for release evidence |
| `REL-010` | retained-log maintenance prerequisite | `Z.24` is complete before final sign-off | `PASS` | `docs/phase-Z/readiness.md` row `Z.24` | retained-log maintenance adoption is closed on the accepted `1.1.0` observability line |

Final checklist verdict: `FAIL`

- release sign-off is blocked by `REL-005` / `REL-008`
- `docs/phase-Z/readiness.md` must therefore remain `NOT_READY` pending a
  follow-up fix or an explicit `team-lead` deferral decision
