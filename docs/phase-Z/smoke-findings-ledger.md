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
| `Z1-F001` | `Z.1` | `Z1-005` | Fresh team config membership is not sufficient to resolve a roster-backed delivery harness, so the first clean-room `atm send` fails closed instead of delivering to a config-defined recipient. | `blocking` | `arch-ctm` | `open` | `reproduced blocker` | `FAIL` | Reproduced again in the `Z.2` clean-room rerun with a disposable `HOME` + `ATM_HOME` environment and a valid `z1-team/config.json` member list. The command still failed with `failed to resolve roster-backed delivery harness for z1-recipient@z1-team`. `Z1-006` remained blocked by this root cause, so `Z.2` cannot enter canary. |
| `Z1-F002` | `Z.1` | `Z1-008` | Current-state `mail.db` cannot pass current SQLite schema initialization, so daemon auto-start never publishes IPC and daemon-backed retained commands fail on the copied real-state baseline. | `blocking` | `arch-ctm` | `open` | `reproduced blocker` | `FAIL` | Reproduced again in the `Z.2` copied-state rerun on a disposable copy of `~/.claude/teams/atm-dev` plus `~/.atm/db/mail.db` with no live-state writes. `atm doctor`, `list`, `send`, and `read` all still failed with `failed to initialize sqlite schema`, followed by `failed to connect to daemon local IPC endpoint ... after auto-start`, so `Z.2` cannot enter canary. |
| `Z2-F001` | `Z.2` | `Z1-003` | The clean-room retained roster inspection surface regressed: `atm teams --json` and `atm members --json` now fail with `sqlite-backed retained runtime is unavailable because no default runtime factory is installed` on a row that passed in `Z.1`. | `blocking` | `arch-ctm` | `open` | `blocking regression discovered in rerun` | `FAIL` | This regression was discovered during the frozen `Z.2` rerun of `Z1-003`. Because a previously passing retained-command row now fails, `Z.2` must stop before `Z.3`; this row needs triage before any canary entry decision can become `proceed`. |
