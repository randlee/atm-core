# phase-bp contract

These files are proven: they render with `sc-compose` 1.7.0 and run against
`bd` 1.3.2. bp-1 copies them into
`plugins/atm-bd-orchestration/skills/plan-hardening/` and keeps every step
id, metadata key, variable name and command they use.

| File | Proof (2026-11-03) |
| --- | --- |
| `skills/plan-hardening/formulas/plan-hardening-iteration.formula.toml.j2` | `sc-compose bead render` renders it to a `.formula.toml`. `bd mol wisp <file>` creates 4 ephemeral beads: the scope and critical steps are both ready with no edge between them, and the fix step needs both and is assigned to the planner. Step metadata (`pass`, `root`, `planner`, `iteration`) and labels survive. The wisp is absent from plain `bd ready` and listed by `bd ready --mol <wisp>`. `bd mol burn` deletes it. |
| `skills/plan-hardening/formulas/plan-hardening-final.formula.toml.j2` | renders, and `bd mol wisp --dry-run` shows ceremony, then fix, then verify |
| `skills/plan-hardening/formulas/plan-hardening-adversary.formula.toml.j2` | renders with a `task` that contains double quotes, and the result parses as TOML. `bd mol wisp --dry-run` creates 2 issues: the root and one `attack` step with `metadata.pass = "adversary:<area>"`. |
| `skills/plan-hardening/templates/plan-hardening-step.xml.j2` | `sc-compose render --strict` with each example vars file: the teammate render has `atm task start` and `atm task close`, the subagent render has no `atm` command, and both claim and close the step bead. A runner other than `teammate` or `subagent` fails the strict render (exit 2, no output file). |

Findings from the same spike that the skill relies on:

- `bd mol squash <wisp>` writes a digest that keeps each step's close reason,
  but not its metadata. A step records its verdict and filed fix-bead ids in
  its close reason for that reason.
- `bd mol wisp` accepts a formula file path, so the rendered formula never
  has to go into `.beads/formulas/`.
