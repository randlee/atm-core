---
name: sprint-review
description: Refresh the phase dependency diagram and write its HTML locally. Optionally view it in Wyvern with --view.
---

# Sprint Review

Run from the working repository or worktree:

```bash
.claude/skills/sprint-review/scripts/sprint-review
# Only when the user requests a viewer:
.claude/skills/sprint-review/scripts/sprint-review --view
```

`--root <bead-id>` and `--index <path>` select a phase when needed.

Every run reads the plan file `<plans_dir>/phase-<p>.jsonl`,
uses them for the dependency graph, queries Beads/ATM only for current state,
and regenerates the SVG inside a self-contained HTML page.

It writes `<plans_dir>/phase-<p>/phase-<p>-dag.html` locally; it never commits
or pushes, and never rewrites the plan file.

Without `--view`, do not launch a viewer, render an inline image, or otherwise
show the diagram. Report only the saved path. With `--view`,
open the HTML in Wyvern if available. Missing or failing Wyvern must not prevent
writing the HTML. Do not substitute Preview or a browser without a request.

The page embeds SVG directly with zoom controls and state tooltips; it needs no
server or installed dependencies to remain readable after download. Wyvern is
only the optional viewer. Rendering requires Python, Node and the pinned
[sprint-report renderer](../sprint-report/renderer/package.json):
`npm ci --prefix .claude/skills/sprint-report/renderer --ignore-scripts`.

For report columns, badge meanings and evidence sources, see
[sprint-report](../sprint-report/SKILL.md). Local exports for development remain
available through `.claude/skills/sprint-report/scripts/sprint-report --dag --output <prefix>`.
