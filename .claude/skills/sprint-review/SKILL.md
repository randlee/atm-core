---
name: sprint-review
description: Refresh the phase dependency diagram and push its permanent HTML artifact to the integration branch. Optionally view it in Wyvern with --view.
---

# Sprint Review

Run from the working repository or worktree:

```bash
.claude/skills/sprint-review/scripts/sprint-review
# Only when the user requests a viewer:
.claude/skills/sprint-review/scripts/sprint-review --view
```

`--root <bead-id>` and `--index <path>` select a phase when needed.

Every run reads the canonical tuples in `<plans_dir>/phase-<p>/sprints.jsonl`,
uses them for the dependency graph, queries Beads/ATM only for current state,
and regenerates the SVG inside a self-contained HTML page.

It commits and pushes the rendered `<plans_dir>/phase-<p>/phase-<p>-dag.html`
on the root bead's `integration_branch`; it never rewrites `sprints.jsonl`.

The initial HTML artifact and canonical phase plan are required before plan review.
Later runs update the page at the same path, leaving each version in Git history.
A temporary detached worktree isolates artifact commits from existing worktrees;
only these two files are committed. The integration branch must exist on origin.
A rejected push is reported as failure; never force-push to replace concurrent work.

Without `--view`, do not launch a viewer, render an inline image, or otherwise
show the diagram. Report only the saved path and pushed commit. With `--view`,
open the HTML in Wyvern if available, initially sized to 80% of the screen's
logical width and height. Missing or failing Wyvern must not prevent
artifact publication. Do not substitute Preview or a browser without a request.

The page embeds SVG directly with zoom controls and state tooltips; it needs no
server or installed dependencies to remain readable after download. Wyvern is
only the optional viewer. Rendering requires Python, Node and the pinned
[sprint-report renderer](../sprint-report/renderer/package.json):
`npm ci --prefix .claude/skills/sprint-report/renderer --ignore-scripts`.

For report columns, badge meanings and evidence sources, see
[sprint-report](../sprint-report/SKILL.md). Local exports for development remain
available through `sprint-report --dag --output <prefix>`; `/sprint-review`
always publishes.
