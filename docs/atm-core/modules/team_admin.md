# `atm-core::team_admin`

Owns the retained local team recovery surface:

- discovered-team listing
- local member listing
- `add-member`
- team backup
- team restore

It must not own:

- clap parsing
- daemon orchestration
- runtime spawning or launch coordination

References:

- Product requirements: `docs/requirements.md` §12 and §13
- `REQ-P-TEAMS-001`
- `REQ-P-MEMBERS-001`
- `REQ-CORE-TEAM-001`
- CLI surfaces:
  - `docs/atm/commands/teams.md`
  - `docs/atm/commands/members.md`
