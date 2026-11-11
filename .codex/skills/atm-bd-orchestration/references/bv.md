# BV: graph analysis for the lead

BV (`bv`, the beads graph analyzer) is the lead's read-only view of the phase
graph. Routine dispatch is the `bd ready` loop: the lead assigns a bead only
while `bd ready` lists it. BV never picks a dispatch, never writes a bead, and
is never a gate or a report artifact. A result is read, acted on through the
normal workflow, or dropped. BV is optional; nothing waits on it.

## Running it

From the repository root:

```bash
.claude/skills/atm-bd-orchestration/scripts/bv-analyze --epic <root>                                         # triage + alerts
.claude/skills/atm-bd-orchestration/scripts/bv-analyze --epic <root> --modes triage alerts plan insights      # wave boundary
.claude/skills/atm-bd-orchestration/scripts/bv-analyze --epic <root> --target <bead>                          # adds <bead>'s blocker chain
.claude/skills/atm-bd-orchestration/scripts/bv-analyze --file <scratch>/plan.jsonl --modes triage plan insights   # before bd import
```

`<root>` is the phase root. The script runs `bd --readonly export --all` into
a fresh temp directory (`export.jsonl`), keeps issue rows only
(`issues.jsonl`), and runs BV on `epic.jsonl`: the root and its `parent-child`
descendants. `--target` runs the blocker chain on `prerequisites.jsonl`: the
target and everything it transitively waits on (any edge but parent-child).
`receipt.json` lists `members`, `prerequisites` and `excluded_dependencies`
(edges crossing the phase boundary); each mode writes `<mode>.json`.

Fail-closed: an export failure, duplicate id, absent root or target, or BV
reading a different, stale or partial source exits 1 with no output. A partial
load names the rejected bead ids and BV's warnings. Fix the cause; there is no
fallback to an older JSONL. Rerun for every decision, never run bare `bv` (it
opens the TUI), and keep the temp directory local (exports hold full bead
text). Read each metric's `.status` first: a `skipped` or `timeout` metric
proves nothing.

## When to run it

- **Before import and at plan review:** after `.claude/skills/atm-beads/scripts/validate-plan --file` passes,
  run `--file`; after import, run `--epic` before the plan-review bead is
  dispatched. This is the only time plan shape changes: the planner fixes the
  plan file before `validate-plan` and `bd import`.
- **Wave boundary:** a sanity PASS releasing dependents, or a QA verdict
  filing findings.
- **Stall:** `bd ready` shows nothing for the phase while beads stay open, a
  bead stays blocked after its prerequisites look done, or a refusal names a
  blocker. Run `--target <stuck bead>`.
- **Phase end:** `.triage.quick_ref.open_count` is 1 (the root) when the
  phase is done; `members` is the descendant set the post-mortem reconciles
  (`post-mortem.md`), and each `excluded_dependencies` edge needs a
  disposition there.

## Decisions

The plan's edges are the minimum set. In a running phase the lead may add a
discovered dependency (`bd dep add <bead> --blocked-by <blocker>`), never
removes a planned edge, and never splits or adds sprints. Work is ordered and
held only with bead edges, gates and `atm task move`; never by telling an
agent not to run a queued task.

| BV shows | Lead action |
| --- | --- |
| a cycle, or a plan too narrow or too serial (before import) | the planner fixes the plan file, then `validate-plan` and import |
| width, critical path or bottleneck shape (after import) | a note for the next phase's planning session; no artifact, no change now |
| a claimed or in-progress bead that is blocked | the assignee refuses it (the template's not-ready refusal); the lead re-assigns the same task id once `bd ready` lists it |
| a blocker chain ending at a missing prerequisite edge | `bd dep add <bead> --blocked-by <blocker>` |
| a slack-0 bead behind same-priority work, or a `priority_mismatch` | a lead decision: `bd update <bead> --priority <n>` with the reason in notes; finding priority comes from severity (`SKILL.md`, Priority) |
| an open sanity bead (`<sprint>.group-sanity`) whose dev bead closed | the `bd ready` loop missed it: dispatch it once `bd ready` lists it, re-using its task id if one exists |
| the plan-review bead, or a `bd gate` bead | the plan gate has not passed, or the user holds the gate; say what it holds |
| a blocker outside the phase (`excluded_dependencies`) | report it to that phase's lead or the user; never pull it in |
| sanity beads stacking up as bottlenecks | tell the user; staffing is the user's call |
| `stale_issue` on an in-progress bead | read the assignee's branch and last report read-only first; quiet is not a stall |

Reading notes: in the 0.9 bead model sprint beads are containers, and the
dev, sanity and QA beads are poured under them; dependents wait on the
predecessor's sanity bead, never its dev bead, so a sanity bead is usually
the top bottleneck. `--robot-plan` `.plan.tracks` covers only beads
actionable now: it is the parallel work available now, not the plan's
parallel width. Critical-path length counts serial steps, not time.

## Expected noise

- `potential_duplicate` alerts on template-rendered beads (every sanity bead,
  plan-review and QA beads).
- Orphans that are the phase root, `stage:plan*` beads, `bd gate` beads or a
  release bead.
- Triage `claimable` flags and emitted commands: dispatch is the `bd ready`
  loop's job; never run what BV emits.
- `--robot-priority` suggestions: inputs, not instructions.
