---
name: sprint-report
description: Generate a sprint status table or dependency DAG from live beads. The DAG HTML is written locally; --view optionally opens Wyvern.
---

# Sprint Report Skill

## Usage

`--table` is the default mode; use `--detailed` for one block per sprint.

Run the repository-local report command from the checkout or worktree being used:

```bash
.claude/skills/sprint-report/scripts/sprint-report --table
```

Use `--detailed` for one block per sprint. The command reads the committed
plan file `<plans_dir>/phase-<p>.jsonl`, then refreshes bead state and PR/CI state.
Rows are never hand-typed.

## Dependency diagram

Use `--dag` to refresh the diagram without opening a viewer, or
`--view` to also open its HTML artifact in Wyvern when available:

```bash
npm ci --prefix .claude/skills/sprint-report/renderer --ignore-scripts
.claude/skills/sprint-report/scripts/sprint-report --dag
.claude/skills/sprint-report/scripts/sprint-report --view
```

The dedicated [`sprint-review`](../sprint-review/SKILL.md) command always
writes the HTML; its `--view` flag is the only way it opens the diagram. No viewer is
opened by default. Wyvern runs detached in the background, with output sent to
a log, so the agent remains available. Missing or failing Wyvern does not
prevent writing the HTML; no alternative viewer is launched automatically.

By default, DAG generation writes `<plans_dir>/phase-<p>/phase-<p>-dag.html`
locally; it never commits or pushes. It reads the committed plan file and
never rewrites it from Beads state. The HTML embeds the SVG directly, including
state tooltips and zoom controls, without external dependencies.

Local render intermediates live under
`scratchpad/phase-<p>-dag/phase-<p>-dag`: `.svg`, `.html`, `.png`, `.dot`,
`-layout.svg`, `-data.json`, `-state.json`, and `-icons.json`. These scratch files are
not committed. State-only refreshes reuse the existing layout.

To skip writing next to the plan, pass `--output <prefix>` to
`.claude/skills/sprint-report/scripts/sprint-report --dag` or `--view`. The legacy `--dag --open` option explicitly
opens the PNG in Preview on macOS (default image viewer elsewhere); do not use
it for `/sprint-review`. Diagram modes are mutually exclusive with `--table`
and `--detailed`. `--root` and `--index` work in every mode. Diagram generation
requires Python, Node, `bd` and `atm`;
it does not query GitHub PRs or invoke `sc-compose`.

Each sprint's poured `<container>.group-dev` and `<container>.group-sanity`
are verified against live `blocks` edges. No finding, fix, or sprint QA beads
are drawn, and no dependency is inferred from index order or PR stacks. Every
displayed arrow is a real bead dependency, drawn **prerequisite → dependent**:
work → its sanity gate → downstream work. Missing or duplicate sanity gates
and dependency cycles stop generation with an error.

Green checks require recorded dev completion and an explicit sanity PASS;
sanity gates display a green check plus their completed iteration count.
Counts use the maximum recorded `.sc` iteration and completed ATM events,
including failed runs. Zero means no recorded runs; `?` means unavailable.
Pending, active, blocked, open sanity findings, and an explicit
user override have distinct states. A PASS predating the latest dev completion,
or unavailable completion evidence, is marked unconfirmed rather than green.
The SVG tooltips and `-icons.json` explain each state. These checks do **not**
mean QA approval or merge readiness.

The bottom badge shows the block's associated QA state using the same QA
rules as the table: assigned, active, or passed. Failed QA (or any remaining
open QA findings) displays red `b:i:m` counts across all QA rounds instead of
an icon. A failed verdict remains `0:0:0` when all findings are closed until
a later QA records PASS. Undispatched QA has no bottom badge. Sanity gates have no QA badge unless a QA bead explicitly
validates that gate. QA badges are overlays and do not change the DAG layout.

## Data sources

The plan file is the source of graph truth. Each line is
`{"sprint": "<name>", "depends_on"?: [...]}`; the sprint container is
`atm-<sprint>` and its poured dev and sanity beads
`<container>.group-dev` and `<container>.group-sanity`. Beads supply only live state and content.
The report never derives or persists plan edges from a Beads snapshot.

The report reads phase identity and integration branch from the root bead,
and sprint names, titles, stack layers, and branches from live sprint containers.
Dependency order comes from the canonical plan. Table ordering follows current bead layer then sprint
number. It verifies the indexed sanity pairing, derives QA beads from live
graph edges, and counts open findings across QA rounds. Paginated `gh api`
pull-request results match each sprint container's branch, then `gh pr view` fetches
selected PR checks. The integration row matches the root bead's integration
branch; the detailed block names the integration PR's own base (`→ <base>`),
and no target when there is no PR.

The table QA cell contains one icon: 📥 assigned, 🌀 in progress, ✅ pass,
or 🚩 findings (a FAIL verdict or open findings). A sprint without a QA bead
has an empty QA cell. Absorbed work is excluded from the sprint index and all
report views; its historical bead is not a separate sprint.
The verdict comes from QA metadata or
the `PASS:`/`FAIL:` close-reason prefix. This is the authoritative
round/verdict/open-finding presentation for the detailed report:
`R<round> <verdict> (<open> open)`.

The FIND column shows open QA findings as `b:i:m` (blocking:important:minor),
using finding severity metadata or labels across all QA rounds for the sprint.
A newer round never hides open findings from an older round. It is empty before QA dispatch,
and `0:0:0` when a dispatched QA has no open findings.
The S column shows the highest completed `iteration` for the sprint's sanity
task from `.sc/sanity-log/phase-<p>.jsonl` and the renamed historical
`.sc/sanity-log/sanity-llm.jsonl` in the primary checkout (located through
Git's common directory). Filter the historical file by record `phase`, or
its sprint-derived phase when `phase` is absent. Take the maximum iteration
across both files; paired reviewer rows do not count as additional runs. This cumulative value includes completed
PASS and FAIL runs; counting log lines would undercount older runs. When the
log has no record for a sanity task, count its completed events from
`atm task events <task> --all --json`. Blank S means zero completed runs;
`?` means the history is unavailable, not that the task never ran.
DEV uses 🚩 for sanity findings and 🔨 while those findings are being fixed;
a missing sanity bead also uses 🚩. DEV uses 🚧 for explicit blocked status
or unfinished dependencies reported by `bd blocked --json`.
DEV is ✅ only when dev and sanity beads are both closed, sanity explicitly
records PASS (verdict metadata or a PASS close-reason prefix), and no open
sanity findings remain. Closed beads alone do not establish a sanity pass;
a closed sanity bead without an explicit PASS is 🚩.
CI uses 🚧 when merge is blocked and
🚀 when GitHub reports an open, non-draft PR as clean and mergeable with
completed passing checks, DEV is ✅, QA is closed with PASS, and no open QA
findings remain. Green CI alone is ✅, never a merge-readiness claim.
Failed checks take precedence as ❌.

## Render command

The template path is relative to the main repository root. The script invokes:

```bash
sc-compose render --file .claude/skills/sprint-report/report.md.j2 --var-file <json>
```

To render manually, write the variables JSON produced by the script to a
temporary file and run the same command from the repository root.

## Table variables

```json
{
  "mode": "table",
  "sprint_rows": "| d-12 | ✅ | 3 | 🚩 | 2:10:4 | 🏁 | #233 |",
  "integration_row": "| **integrate/phase-d** | | | | | 🌀 | — |"
}
```

## Detailed variables

```json
{
  "mode": "detailed",
  "sprint_rows": "Sprint: d-12  types 2.0 contract\nDEV: ✅ (closed)\nS: 3\nQA: R1 FAIL (16 open)\nFIND: 2:10:4\nCI: 🏁\nPR: #233",
  "integration_row": "Integration: integrate/phase-d → main\nCI: 🌀\nPR: #240"
}
```

## Icon reference

| State | DEV | QA | CI |
|-------|-----|----|----|
| Assigned | 📥 | 📥 | |
| In progress | 🌀 | 🌀 | 🌀 |
| Done/pass | ✅ | ✅ | ✅ |
| Findings | 🚩 | 🚩 | |
| Fail | | | ❌ |
| Fixing | 🔨 | | |
| Blocked | 🚧 | | 🚧 |
| Merged | | | 🏁 |
| Ready to merge | | | 🚀 |
