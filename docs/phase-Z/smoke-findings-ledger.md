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
| `Z1-F001` | `Z.1` | `Z1-005` | Fresh team config membership is not sufficient to resolve a roster-backed delivery harness, so the first clean-room `atm send` needs an explicit ATM roster setup recovery contract instead of an opaque delivery-policy failure. | `blocking` | `arch-ctm` | `closed` | `closed in Z.11 as explicit setup contract` | `PASS` | `Z.11` closed this blocker by replacing the opaque send-path failure with the exact actionable recovery contract: `Repair or reload the team roster before retrying delivery.` and `Use 'atm teams add-member' for all active team members.` The send path still fails cleanly when ATM roster state is empty, and no hidden `config.json` fallback was added. |
| `Z1-F002` | `Z.1` | `Z1-008` | Current-state `mail.db` cannot pass current SQLite schema initialization, so daemon auto-start never publishes IPC and daemon-backed retained commands fail on the copied real-state baseline. | `blocking` | `arch-ctm` | `open` | `reproduced blocker` | `FAIL` | Reproduced again in the `Z.2` copied-state rerun on a disposable copy of `~/.claude/teams/atm-dev` plus `~/.atm/db/mail.db` with no live-state writes. `atm doctor`, `list`, `send`, and `read` all still failed with `failed to initialize sqlite schema`, followed by `failed to connect to daemon local IPC endpoint ... after auto-start`, so `Z.2` cannot enter canary. |
| `Z2-F001` | `Z.2` | `Z1-003` | The clean-room retained roster inspection surface regressed: `atm teams --json` and `atm members --json` now fail with `sqlite-backed retained runtime is unavailable because no default runtime factory is installed` on a row that passed in `Z.1`. | `blocking` | `arch-ctm` | `closed` | `closed in Z.12 via retained roster-store seam and boundary lint gate` | `PASS` | `Z.12` removed the ambient retained-runtime path from `atm teams`, `atm members`, `atm teams add-member`, and `team_admin`, routing those call sites through the approved `RosterStore` seam instead. Clean-room revalidation on `602292e3` proved `atm teams --json`, `atm teams add-member z12-team z12-operator --json`, and `atm members --team z12-team --json` all succeed with no installed default runtime factory. `SCB-RETAINED-001` now fails any reintroduction of the forbidden direct `service_runtime_store::default_runtime()` path in these command-entry surfaces. |
