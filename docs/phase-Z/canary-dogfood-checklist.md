# Phase Z Canary And Dogfood Checklist

## Purpose

Authoritative operator-facing checklist for `Z.3`.

## Record Schema

The checklist must freeze:

- the `atm-dev` participant list
- the approved binary baseline under evaluation
- the reporting path used for operator findings
- the operator flows and recovery behaviors each participant is expected to
  exercise

Each checklist row must record:

- `participant`
- `operator_flow`
- `expected_behavior`
- `recovery_behavior`
- `verdict`
- `notes`

## Rules

- this checklist is frozen at the start of `Z.3`
- every checklist row must record one final verdict before `Z.3` closes; any
  row left without a final verdict is blocking for the sprint
- operator reports that do not map back to a checklist row must be added as
  explicit findings with notes explaining the extra coverage

## Frozen Canary Baseline

- accepted binary baseline: `97518da5`
- sprint execution branch: `feature/pZ-s17-smoke-z3-rerun`
- operator-report path: `docs/phase-Z/canary-findings-ledger.md` plus ATM
  status reports to `team-lead`

## Frozen Participant List

- `team-lead`
- `arch-ctm`

## Verdict Rows

| participant | operator_flow | expected_behavior | recovery_behavior | verdict | notes |
| --- | --- | --- | --- | --- | --- |
| `team-lead` | task dispatch and coordination handoff | `arch-ctm` receives and acts on task/merge notices from `team-lead` on the accepted baseline | if a dispatch issue occurs, capture the exact ATM/read failure and promote it to the canary findings ledger | `PASS` | `atm log snapshot --json` shows recent `team-lead -> arch-ctm` send outcomes and matching `arch-ctm` read outcomes on `atm-dev` during `Z.17` execution |
| `team-lead` | direct teammate receive path | teammate-targeted canary send to `team-lead` persists successfully on the accepted baseline | if inbox delivery or retention fails, promote a validated finding for `Z.4` | `PASS` | `atm send team-lead "z17 canary baseline check from arch-ctm" --requires-ack --json` returned `outcome: sent` with message id `d9b986cd-3741-40a7-bd96-d8439414bd1c` |
| `arch-ctm` | retained command surface | `atm doctor --json`, `atm teams --json`, and `atm members --team atm-dev --json` complete on the accepted baseline | warning-only `doctor` output remains actionable; runtime/command failure would be promoted | `PASS` | `atm doctor --json` completed warning-only with no errors; `atm teams --json` and `atm members --team atm-dev --json` succeeded on the live `atm-dev` roster |
| `arch-ctm` | read/report loop | `atm read --all --json` and `atm log snapshot --json` expose the current mailbox and retained ATM command activity on the accepted baseline | if read/reporting fails, record the exact command failure in the canary findings ledger | `PASS` | both retained read surfaces succeeded during `Z.17`; `atm log snapshot --json` captured the baseline send/read activity used for this canary record |
