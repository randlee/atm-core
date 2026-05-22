# Phase Z Smoke Checklist

## Purpose

Authoritative executable smoke matrix for `Z.1` and `Z.2`.

This checklist is frozen at the start of `Z.1`, executed on the approved
real-binary baseline, and rerun without scope drift in `Z.2`.

## Record Schema

Each row must record:

- `flow_id`
- `operator_flow`
- `command_or_entrypoint`
- `expected_result`
- `recovery_or_corner_case`
- `z1_verdict`
- `z2_revalidation_verdict`
- `notes`

## Required Flow Coverage

The frozen checklist must include at least:

- daemon bring-up on the approved baseline under test
- retained CLI command coverage that `Phase Z` treats as ship-critical
- one recovery path for each operator flow where the command or daemon can fail
- one negative-path or corner-case exercise per feature area claimed by `Z.1`
- explicit coverage rows for these retained recovery/corner-case categories:
  - daemon startup / readiness failure or degraded-start behavior
  - notification delivery failure or degraded-notification behavior
  - reconcile interruption, shutdown, or retry-visible behavior
  - retained CLI command error reporting and operator recovery guidance

## Ownership

- created and frozen in `Z.1`
- rerun without widening or narrowing in `Z.2`
- any proposed scope change after `Z.1` freeze must be documented as a separate
  plan correction, not silently edited into the active checklist

## Frozen Baseline Under Test

- release binaries built from `feature/pZ-s1-smoke-bring-up`
- clean-room bring-up lane:
  - disposable `HOME` + `ATM_HOME`
  - disposable `.atm.toml` with `default_team = "z1-team"`
  - disposable `config.json` roster for `z1-team`
- cloned real-state lane:
  - disposable copy of `~/.claude/teams/atm-dev`
  - disposable copy of `~/.atm/db/mail.db`
  - no writes against the live host-scoped ATM state

## Frozen Z.1 Results

| flow_id | operator_flow | command_or_entrypoint | expected_result | recovery_or_corner_case | z1_verdict | z2_revalidation_verdict | notes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `Z1-001` | Build the approved smoke baseline | `cargo build --release -p agent-team-mail -p atm-daemon` | release CLI and daemon binaries build successfully on `integrate/phase-Z` baseline | baseline build proof | `PASS` | `PENDING` | `target/release/atm` and `target/release/atm-daemon` built successfully before the smoke pass. |
| `Z1-002` | Bring up daemon/runtime on a clean-room baseline | `HOME=<tmp> ATM_HOME=<tmp> ATM_TEAM=z1-team ATM_IDENTITY=z1-operator target/release/atm doctor --json` | daemon auto-start succeeds and `doctor` reports ready runtime state | daemon startup / readiness failure or degraded-start behavior | `PASS` | `PENDING` | Clean-room `doctor` succeeded and reported ready runtime state with bootstrap trace and retained observability paths. |
| `Z1-003` | Inspect retained local roster surface on a clean-room baseline | `target/release/atm teams --json` and `target/release/atm members --json` in the clean-room environment | retained team and member inspection commands succeed against the frozen team config | retained CLI command coverage | `PASS` | `PENDING` | `teams --json` and `members --json` succeeded against disposable `z1-team` config state. |
| `Z1-004` | Exercise empty-mailbox retained CLI surface on a clean-room baseline | `target/release/atm list --json`, `read --all --json`, `clear --json`, `log snapshot --json` in the clean-room environment | retained CLI mailbox/log commands succeed on an empty durable-state baseline | retained CLI command coverage | `PASS` | `PENDING` | `list`, `read`, `clear`, and `log snapshot` all succeeded on the disposable empty-store baseline. |
| `Z1-005` | Send the first message to a config-defined recipient on a clean-room baseline | `target/release/atm send z1-recipient \"hello z1\" --requires-ack --json` | config-defined roster member resolves to a delivery harness and the message persists/delivers successfully | notification delivery failure or degraded-notification behavior | `FAIL` | `PENDING` | Send failed closed with `failed to resolve roster-backed delivery harness for z1-recipient@z1-team`; fresh `config.json` membership alone did not seed the SQLite roster store. |
| `Z1-006` | Exercise degraded notification after a successful durable send | same clean-room send lane with a failing `[[atm.post_send_hooks]]` rule for `z1-recipient` | durable send succeeds and degraded notification is surfaced as a warning/evidence artifact when the post-send hook fails | notification delivery failure or degraded-notification behavior | `FAIL` | `PENDING` | This row was blocked by `Z1-005`: the send path never reached notification/post-send-hook execution because roster-backed harness resolution failed first. |
| `Z1-007` | Verify retained CLI validation and recovery guidance | `ATM_IDENTITY=z1-recipient target/release/atm ack \"\" \"ack from z1\" --json` in the clean-room environment | command fails with actionable validation/recovery guidance rather than silent misuse | retained CLI command error reporting and operator recovery guidance | `PASS` | `PENDING` | Invalid `ack` invocation failed with explicit validation behavior instead of mutating state silently. |
| `Z1-008` | Bring up the current real-state durable baseline without touching live host data | cloned `atm-dev` state under disposable `HOME`/`ATM_HOME`; run `target/release/atm doctor --json`, `list --json`, `send ... --json`, `read --all --json` | daemon starts, publishes IPC, and serves daemon-backed retained commands on a copied current-state baseline | daemon startup / readiness failure or degraded-start behavior | `FAIL` | `PENDING` | Every daemon-backed command failed: first `failed to initialize sqlite schema`, then `failed to connect to daemon local IPC endpoint ... after auto-start`. Direct `target/release/atm-daemon` startup reproduced the same schema-init failure. |
| `Z1-009` | Exercise reconcile/runtime retry-visible smoke coverage | daemon-backed send/read lifecycle on either the clean-room or cloned real-state lane | retry-visible daemon/runtime behavior is observable while the durable send/read path succeeds | reconcile interruption, shutdown, or retry-visible behavior | `FAIL` | `PENDING` | No `Z.1` lane reached a successful daemon-backed durable send/read cycle once reconcile/runtime ownership mattered: the clean-room lane stopped at roster-harness resolution, and the copied real-state lane stopped at SQLite schema initialization. |
