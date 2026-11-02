# Sprint bo-13: typed refusal on the hook wire, and full hook results in the audit log

Repo: **atm-core** (not atm-bd). Branch `sprint/bo-13-hook-wire-audit` off `develop`. It does not depend on bo-9..12. Owned paths: `crates/atm-core/src/hook/reply.rs`, `crates/atm-http-runtime/src/hook_filter.rs` and their tests.

## Goal
- Rand: every hook return value is recorded in an event log for audit.
- Today `log_run` records only the outcome and the action count, and a refused reply is `{message}` with extra keys rejected (`crates/atm-core/src/hook/reply.rs:221-261`).

## Deliverables
1. The refused reply grammar accepts an optional structured refusal `{code, message, fix, context}`. `{message}` alone stays valid; unknown keys are still rejected.
2. `log_run` records the full typed reply:
   - the outcome;
   - the refusal code/message/fix/context;
   - the ordered actions (command, as, request);
   - the hook path, invocation id and task id.
3. No new audit trait, sink or receipt type.

## Acceptance
- [ ] Parser tests: `{message}`; `{message, code, fix, context}`; an unknown key, rejected.
- [ ] One `log_run` test per outcome (not-applicable, pass-through, refused with and without code, handled with two actions) asserts the recorded event fields exactly.
- [ ] An existing hook (no code field) still parses and logs.

## Validation
`just lint`, `just test`.
