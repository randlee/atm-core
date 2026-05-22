# Phase Z Smoke Findings Ledger

## Purpose

Authoritative findings ledger for `Z.1` smoke results and `Z.2` revalidation.

## Record Schema

Each finding entry must record:

- `finding_id`
- `discovered_in`
- `linked_flow_id`
- `summary`
- `severity`
- `fix_owner`
- `status`
- `z2_disposition`
- `revalidation_result`
- `notes`

## Rules

- only verified `Z.1` findings may appear in this ledger
- `Z.2` fixes only findings recorded here
- if a `Z.1` observation is rejected as non-reproducible, that outcome must
  still be recorded here rather than dropped silently
- newly discovered issues found during `Z.2` that are out of scope for the
  frozen `Z.1` handoff must be recorded here using `status: out_of_scope`
  rather than fixed in the `Z.2` sprint

## Z.1 Findings

| finding_id | discovered_in | linked_flow_id | summary | severity | fix_owner | status | z2_disposition | revalidation_result | notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `Z1-F001` | `Z.1` | `Z1-005` | Fresh team config membership is not sufficient to resolve a roster-backed delivery harness, so the first clean-room `atm send` fails closed instead of delivering to a config-defined recipient. | `blocking` | `arch-ctm` | `open` | `PENDING` | `PENDING` | Reproduced in a disposable `HOME` + `ATM_HOME` environment with a valid `z1-team/config.json` member list. The command failed with `failed to resolve roster-backed delivery harness for z1-recipient@z1-team`. `Z1-006` remained blocked by this root cause. |
| `Z1-F002` | `Z.1` | `Z1-008` | Current-state `mail.db` cannot pass current SQLite schema initialization, so daemon auto-start never publishes IPC and daemon-backed retained commands fail on the copied real-state baseline. | `blocking` | `arch-ctm` | `open` | `PENDING` | `PENDING` | Reproduced on a disposable copy of `~/.claude/teams/atm-dev` plus `~/.atm/db/mail.db` with no live-state writes. `atm doctor`, `list`, `send`, `read`, and direct `atm-daemon` startup all failed with `failed to initialize sqlite schema`; the copied DB still exposes `mail_messages.legacy_message_id` and does not expose `mail_messages.message_id`, which is consistent with the failing current migration batch. |
