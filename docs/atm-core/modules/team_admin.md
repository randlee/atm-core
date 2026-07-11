# `atm-core::team_admin`

Owns the retained local team recovery surface:

- discovered-team listing
- local member listing
- `add-member`
- `update-member`
- team backup
- team restore

It must not own:

- clap parsing
- daemon orchestration
- runtime spawning or launch coordination

## Retained Exceptional Write Paths

The following two private functions in `crates/atm-core/src/team_admin.rs` perform
file-system writes and are **approved exceptions** to the hard-write boundary
consolidation rule established in Phase Y Sprint 3.

### `ensure_inbox_exists` (lines 313, 455)

```
fn ensure_inbox_exists(inbox_path: &Path) -> Result<bool, AtmError>
```

Called from `add_member` during member creation. Creates the inbox
file for a new team member if it does not already exist. This is an initial
provisioning write: it runs exactly once per member, only during `add-member`,
and is guarded by an existence check (`inbox_path.exists()` returns early). It
is **not** part of the normal send / ack / clear message-flow.

### `write_team_config` (lines 333, 486)

```
fn write_team_config(team_dir: &Path, config: &TeamConfig) -> Result<(), AtmError>
```

Called from the retained local team repair paths after ATM roster truth is
mutated. Persists the updated compatibility `config.json` projection after
`add-member`, `update-member`, or restore operations. It is **not** invoked on
the normal send / ack / clear path.

### Rationale

Both functions handle **initial setup and configuration writes** — specifically
inbox provisioning and team config persistence — which are structurally distinct
from the message-flow rewrites targeted by the boundary consolidation. They are
the **only approved owner paths** in `team_admin` for direct file-system write
operations post-consolidation. Any future write added to this module must be
reviewed against this boundary and justified here before merging.

References:

- Product requirements: `docs/requirements.md` §12 and §13
- `REQ-P-TEAMS-001`
- `REQ-P-MEMBERS-001`
- `REQ-CORE-TEAM-001`
- CLI surfaces:
  - `docs/atm/commands/teams.md`
  - `docs/atm/commands/members.md`
- [inbox-write-path-audit.md §5](../../../docs/plans/phase-Y/inbox-write-path-audit.md) — retained exceptional write paths policy
