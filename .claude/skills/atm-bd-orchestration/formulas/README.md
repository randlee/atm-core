# Bead group formulas

`scripts/bead-groups` pours these with the sc-compose beads pour (today
`scripts/sc-compose-pour-mock`), then adds the edges in `<name>.relations.json`.
A file of the same name in `<repo>/.atm-bd/formula/` overrides the one here.

| Formula | Attaches under | Poured ids | Shape |
|---|---|---|---|
| `sprint-group` | the sprint container | `<sprint>.group-{dev,sanity,qa}` | dev <- sanity <- qa |
| `finding-group` | the same sprint container | `<sprint>.<finding_ref>-r<round>-{fix,sanity,qa}` | fix <- sanity <- qa |

Under the default E1 option C, sanity blocks on dev (or fix) and qa blocks on
sanity; qa also `validates` dev (or fix). A sprint's dev bead `blocks` on the
initial sanity bead of each predecessor in the plan file. A blocking finding
has no bead of its own: its fix bead is the dev's task, carries `finding_ref`,
`severity`, `reviewer`, `remedy`, `filed_by`, `found_at_commit`,
`requirements` and `adrs`, and is `discovered-from` its `filed_by` qa bead.
Important and minor findings are not poured; QA files them as plain finding
beads against the phase or feature.

No poured bead has an assignee: each carries `difficulty`, and the lead picks
the agent at dispatch. Each carries its stage label (`stage:dev` or
`stage:fix`, `stage:dev-sanity`, `stage:qa`), which routes it in the lead's Loop.

## Closers

bd refuses to close a bead with an open blocker or open child, so the edges
enforce:

| Bead | Closed by | When |
|---|---|---|
| dev, fix | the dev | the work is committed |
| sanity | dev-sanity | PASS only. On FAIL it reopens the dev or fix bead and leaves sanity open; sanity is blocked again by its edge |
| qa | quality-mgr | after it has poured a finding-group for each blocking finding (`bead-groups --findings`; refused once the `filed_by` QA bead is closed) and filed the important and minor findings |
| sprint container | the team lead | every group bead under it is closed |
