# ATM-MEMBERS-HARNESS-NOT-EXPOSED: atm members CLI omits harness field

## Pattern
```
atm members
atm members --json
harness
RosterEntry
RosterHarness
smoke.*preflight
skillrx@hermes
hermes/python-graft
```

## Crates Affected
- atm (CLI command surface: `crates/atm/src/commands/members.rs`, `crates/atm/src/output.rs`)
- atm-core (team_admin projection: `crates/atm-core/src/team_admin/projection.rs`)
- atm-core (boundary contracts: `crates/atm-core/src/boundary/store.rs`)

## Sprint Origin
Discovered 2026-07-28 during hermes smoke-test preflight verification. Cipher-311d reported that smoke-test tooling cannot verify via the CLI surface that a given team member (e.g. `skillrx@hermes`) is running a graft-compatible harness (e.g. `hermes/python-graft`). Cipher confirmed via direct SQLite inspection (diagnostic only, not a usable workaround) that the harness value is correctly stored in the team roster and that `atm teams update-member --harness` can set it — the gap is purely a read/output surface issue on the `atm members` command.

## Status
fixed — harness field is now exposed in `atm members` output (both JSON and plain-text). Fix verified in current codebase: projection layer includes harness (projection.rs:104), plain-text rendering shows harness field (output.rs:530-533), JSON output includes full MemberSummary with harness field (output.rs:517).

## Original Issue (Resolved)
The `RosterEntry` struct (alias for `atm_storage::contract::RosterMember`) is the canonical roster record and includes a `harness: RosterHarness` field with variants: `ClaudeCode`, `CodexCli`, `GeminiCli`, `Opencode`, `Hermes`, `PythonGraft`. The roster harness is correctly persisted in SQLite and can be set via `atm teams update-member --harness`.

The original gap was that `atm members` and `atm members --json` output did not expose the `harness` field, which blocked smoke-test preflight tooling (e.g., Hermes verification that member `skillrx@hermes` has harness `hermes/python-graft`) from using the stable CLI surface.

## Fix Applied
Harness field is now exposed in the public `atm members` output:

1. **Struct layer** (`crates/atm-core/src/team_admin.rs`): `MemberSummary` struct includes `pub harness: RosterHarness` field (line 55)
2. **Projection layer** (`crates/atm-core/src/team_admin/projection.rs:104`): `member_summary_from_roster()` now projects `harness: record.harness` from the source `RosterEntry`
3. **Plain-text rendering** (`crates/atm/src/output.rs:530-533`): `atm members` now displays `harness=<value>` in the output line
4. **JSON rendering** (`crates/atm/src/output.rs:517`): `atm members --json` includes the full `MemberSummary` with harness field (automatic via serde)
5. **Test fixtures** (`crates/atm/src/commands/members.rs`): Test members include harness values

## Fix History
- 2026-07-28: Reported by Cipher-311d during hermes smoke-test preflight — harness field not exposed
- 2026-07-28: Fix implemented — harness added to `MemberSummary` and exposed in both JSON and plain-text output
- 2026-07-28: Verified fixed in current codebase
