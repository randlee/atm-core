# Importing a Markdown Plan

Turns an existing markdown plan into beads so it can run under
`atm-bd-orchestration`. Use it for either:

- **one plan**: a single sprint doc added to a phase that is already in beads;
- **a whole phase**: the phase plan (`docs/plans/phase-<x>/plan-phase-<x>.md`)
  and every sprint doc beside it.

Once imported, the beads are the plan. The markdown files are left as they
are, as history: do not edit, move or delete them, and never run the phase
from both.

The import is a translation, never a rewrite. Every bead field comes from the
markdown. When the markdown lacks something the workflow needs, the import
**stops and reports it** (see Checks); it does not guess. Missing information
goes back to the plan's author to supply, with the exact list of gaps.

## Procedure

0. **Check the database.** Run `bd doctor --json`. Any check with
   `"status": "error"` stops the import: report it to lead. Do not import
   into a database that doctor rejects. `validate-plan` runs doctor again at
   step 5 and step 10.
1. **Find the root.** `bd list -l phase-<x> --type feature -n 0` (or `epic`). If
   the phase root already exists, do not render a new one. Import the
   sprints under it, and gate them with
   `--root <id> --phase <x>` (step 5). Check that its description,
   design and acceptance criteria hold what the phase plan says; report
   anything missing.
2. **Read** the phase plan (whole-phase import) and each sprint doc to import.
   Build one vars file per bead from the mapping below:
   - a whole-phase import produces the root (`plan-root.json.j2`), plus
     `sprint-bead.json.j2` and `dev-sanity-bead.json.j2` for each sprint;
   - a one-plan import produces only the sprint's two beads.
3. **Check** the vars against every row in Checks except the id-exists row
   (step 6), and collect all the gaps before reporting any of them. If any blocking row fails, stop and send the
   list to lead: `atm send <lead> --stdin` with the file, the field and what
   is missing. Nothing is imported.
4. **Render** each bead strictly and collect the result as JSONL:

   ```bash
   sc-compose render --file .claude/skills/atm-beads/templates/<t>.json.j2 \
     --var-file <scratch>/<bead>-vars.json --strict --output <scratch>/<bead>.json \
     && jq -e -c . <scratch>/<bead>.json >> <scratch>/plan.jsonl
   ```

   A failed render must stop you: never pipe a render straight into `jq`,
   which exits 0 on empty input and drops the bead silently.

5. **Gate** the rendered plan. Run from the repository root:

   ```bash
   .claude/skills/atm-beads/scripts/validate-plan --file <scratch>/plan.jsonl
   ```

   Add `--root <id> --phase <x>` when the root is not in the file. The script
   runs `bd doctor`, `check-plan.jq` (fields, labels, graph, stack, and
   `requirements`/`adrs` present as ids or `["NONE"]`), the check that every
   REQ/ADR id exists in its governing document, and the check that every
   assignee is an ATM member. Exit 5 lists the problems, and every one of
   them stops the import. A missing integration branch is only a warning.
6. **Check that no id exists yet.** `bd import` upserts: an existing id is
   overwritten, not refused.

   ```bash
   jq -r .id <scratch>/plan.jsonl | while read -r id; do
     bd show "$id" >/dev/null 2>&1 && echo "EXISTS $id"
   done
   ```

   Any `EXISTS` line stops the import. Re-importing is not a way to update a
   bead; use `bd update` instead.
7. **Dry run**, then import:

   ```bash
   bd import --dry-run -i <scratch>/plan.jsonl
   bd import -i <scratch>/plan.jsonl
   ```

8. **Record the source** on each imported sprint bead:
   `bd update <bead> --append-notes "imported from <doc path>@<git short sha>"`.
9. **Wire the plan gate** right away, before any readiness check. Nothing
   is dispatched until this is done. Create the plan-review bead as in
   `atm-bd-orchestration` "Plan Gate", step 2:
   - whole phase: `<root>-plan-qa`, blocking every root sprint;
   - one plan into a running phase: `<root>-plan-qa` is already closed, so
     create `<root>-plan-qa-<n>` (the next free number), blocking every new
     dev bead.
   Export the sprint index and commit it with the plan: `.claude/skills/atm-beads/scripts/export-sprint-index --root <root> --out docs/plans/phase-<x>/sprints.json`, then `git add docs/plans/phase-<x>/sprints.json && git commit`.
10. **Verify** with `.claude/skills/atm-beads/scripts/validate-plan --root
    <root>`, then check the graph:
    - `bd ready -l phase-<x> -n 0` lists the plan-review bead and no dev bead
      from this import;
    - `bd ready --explain` shows every other dev bead blocked by the
      plan-review bead or by the sanity check beads of its prerequisites.
11. **Sync**: run `bd sync` so the Dolt remote has the plan (see
    `atm-bd-orchestration` "Sync").

The plan then goes to plan review (`atm-bd-orchestration` "Plan Gate",
step 3). Once it passes, `bd ready` lists exactly the root sprints
(`relation: root`, or `parallel_safe` with no prerequisites).

Keep `<scratch>` outside the repository.

## Mapping

### Phase root (whole-phase import only)

| Bead var | Markdown source |
| --- | --- |
| `phase` | frontmatter `phase`, lower-cased (`D` → `d`) |
| `id` | `<prefix>-phase-<x>` (`atm-phase-bd`) |
| `plan_scope` | `feature` for a phase under the Development epic |
| `parent` | the Development epic (`atm-c4v`) |
| `title` | H1 without the `Phase <X> — ` prefix |
| `description` | the intro paragraphs and the sprint table |
| `design` | the stream / branch table and "Scope and retained gates" |
| `acceptance_criteria` | the phase-level gates (retained gates, release gates) |
| `integration_branch` | `integrate/phase-<x>` |

### Each sprint

| Bead var | Markdown source |
| --- | --- |
| `sprint` | frontmatter `id`, lower-cased with `.` → `-` (`D.4` → `d-4`) |
| `id` | `<prefix>-<sprint>` (`atm-bd-4`); its sanity check is `<id>-sanity` |
| `parent` | the phase root's id |
| `title` | H1 without the `<id> — ` prefix |
| `assignee`, `model_class` | frontmatter `assignee`, `model_class` (or the sprint table's `agent:model`) |
| `relation` | frontmatter `relation` (`root`, `must_follow`, `parallel_safe`) |
| `blocked_by` | for each `must_follow` parent in `depends_on`: that parent's sanity check bead (`atm-bd-5-sanity`), never the parent's dev bead |
| `closure_type`, `target_boundary` | frontmatter or the "Closure" section |
| `owned_paths` | the "Owned Paths" section, else "Exact Targets", plus `owned_docs` (see Checks) |
| `description` | Goal, Deliverables, Required Work, and Non-closure, in that order, as markdown |
| `design` | Explicit Code Samples ("Public contract"), Exact Targets, and the dependency rationale |
| `acceptance_criteria` | Acceptance Criteria and Required Validation (the commands) |

| `requirements` | frontmatter `requirements`, plus every REQ id the body relies on (`REQ-ATM-CMD-001`, `REQ-CORE-BOUNDARY-001`, `NFR-…`). Exactly `["NONE"]` only when the author says no requirement governs the sprint |
| `adrs` | frontmatter `adrs`, plus every ADR the body relies on (`ADR-011`). Exactly `["NONE"]` only when the author says so |
| `release_train` | frontmatter, when present |
| `branch` | `sprint/<sprint>-<slug>`, with the slug taken from the doc's branch or file name |
| `worktree` | `<repo>-worktrees/<branch>` |
| `stack` | `phase-<x>`: the phase is one append-only stack |
| `layer`, `pr_target` | planned order: layer 1 targets `integrate/phase-<x>`, and layer n targets the branch of layer n−1. Number the layers in the sprint table's order among sprints of the same dependency depth, and by sprint number within a row. These are the plan's intent: layers really stack in completion order, and lead records the actual values at link time |
| sanity check bead | `id` = `<sprint id>-sanity`, `dev_bead` = the sprint id, `assignee` = `scripts/resolve-role dev-sanity` (ask lead when the role is not mapped or the member is not in `atm members`) |

Section headings vary between plans. Map a section by what it holds, not by
its exact title: "Goal and dependency" is the Goal plus the dependency
rationale; "Non-closure" and "This Sprint Does Not Close" are the same
section; "Public contract" is the code samples.

Markdown-only mechanics are dropped rather than carried over:

- merge-forward triggers;
- "PR-completion trigger";
- the per-sprint `base`;
- `status`.

The dependency edges replace them.

## Checks

Run every check before rendering except the id-exists row, which is step 6.
**Blocking** stops the import. **Warn** is reported with the import but
does not stop it.

| Check | Level | What to do |
| --- | --- | --- |
| Any bead id already exists (step 6) | blocking | stop; update the existing bead instead |
| One-plan import: the phase root does not exist, or is not a `feature` or `epic` | blocking | import the phase first |
| A sprint's `status` is not `planned` (in progress, complete) | blocking | ask lead: finish it on the old workflow, or import it as closed |
| `closure_type` or `target_boundary` missing | blocking | ask the author; do not infer them from the goal |
| No owned code paths: no "Owned Paths" and no "Exact Targets" (`owned_docs` alone is not enough) | blocking | ask the author for the file fence |
| Owned paths taken only from the Deliverables list (no "Owned Paths" or "Exact Targets" section) | warn | import them and list them for the author to confirm |
| Two sprints that can run at once (`parallel_safe`, or neither depends on the other) with overlapping owned paths or `owned_docs` | blocking | the author makes one `must_follow` or splits the sprint |
| No requirement ids and no explicit "no requirements" statement, or the same for ADRs | blocking | ask the author for the ids, or for an explicit `NONE` |
| A REQ or ADR id is not in the governing documents (`manifest-get docs.requirements` / `docs.architecture`, content or file name; or the crate's copy), and does not meet [New Ids](planning.md#new-ids) | blocking | ask the author |
| An id with the REQ shape (`REQ-ATM-CMD-001`) used for something else, such as an error code | warn | list it; do not put it in `requirements` |
| The integration branch `integrate/phase-<x>` does not exist on origin | warn | lead creates it before the first dispatch |
| Acceptance criteria or validation commands missing | blocking | ask the author |
| No design content (no code samples, no exact targets) | blocking | ask the author; "no contract change" must be stated by the author with a reason |
| `depends_on` names a sprint that is not in the plan or in beads | blocking | ask the author |
| `must_follow` without a parent, or a dependency cycle | blocking | ask the author |
| Sprint table, branch table and sprint docs disagree (missing doc, extra doc, different agent or relation) | blocking | ask the author which is right |
| `assignee` is not an ATM identity on the team (`atm members`) | blocking | ask lead for the assignee |
| The planned branch or worktree already exists (`git ls-remote`, `git worktree list`) | blocking | ask lead: rename it, or finish that sprint on the old workflow |
| Branch name is not `sprint/<p>-<n>-<slug>`, and no branch exists yet | warn | rename it at import and list the old and new names |
| `model_class` missing | warn | import without it; lead picks at dispatch |
| Frontmatter `base` is `develop` rather than the integration branch | warn | ignore it; the stack's layer 1 targets `integrate/phase-<x>` |
| Requirement or ADR ids named in the body but not in frontmatter | warn | add them to `requirements` / `adrs` and list them |

The report to lead is one line per gap: `<doc>: <field>: <what is missing or
conflicting>`. It lists the blocking gaps first, then the warnings.
