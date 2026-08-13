# Triage TTL Repair Guide

This is the single source of truth for repairing authoritative phase-tracking
data used by graph orchestration and `/sprint-report`.

## Authoritative location

Repair data only in the named `integrate/phase-*` worktree. Never repair a
sprint worktree, invent a substitute value in a report, or copy data from an
old report. The structured `remediations` object emitted by the report names
the source path and target integration branch.

## Repair loop

1. Read every `remediations[]` item from the report error envelope.
2. In its `target_branch` integration worktree, make the stated source repair.
3. Validate the changed source:

   ```bash
   python3 .claude/skills/graph-orchestration/scripts/validate-findings.py \
     --findings-dir .triage/<project-phase>/findings \
     --structure .sprints/<PHASE>/structure.ttl \
     --events .sprints/<PHASE>/events.ttl --json
   ```

4. Commit and push through the normal review/QA path for that integration
   branch.
5. Re-run `/sprint-report`. Do not render or dispatch until it exits zero.

## Source-specific repairs

### `.sprints/<PHASE>/structure.ttl`

Each sprint needs one unique integer `triage:order`, one `triage:criteria`,
and one declared branch for report/PR state. Add a missing branch to the
existing sprint resource; do not derive it from the criteria filename:

```turtle
triage:<PHASE>-S<n> a triage:Sprint ;
    triage:inPhase triage:Phase<PHASE> ;
    triage:order <n> ;
    triage:criteria "docs/plans/phase-<phase>/sprint-<PHASE><n>.md" ;
    triage:branch "feature/<documented-branch>" .
```

### `.sprints/<PHASE>/events.ttl`

Events are append-only. Restore a missing file from the integration history or
append the correct `Assignment` / `Completion` event with its source
provenance; do not rewrite historical events.

### `.triage/<project-phase>/findings/*.ttl`

Repair malformed Turtle and validator errors in the named finding record.
Preserve finding and occurrence history; use the triaging-findings workflow
for status changes rather than deleting records.

### QA evidence master

Restore or add the authoritative QA run at the path named by the remediation.
The record must identify its sprint and final QA verdict. Do not replace it
with a hand-written value in the rendered report.

### GitHub observation

GitHub state is observed, not copied into TTL. Restore the configured `origin`
or `gh` authentication/connectivity and rerun the report. If no PR exists for
a declared branch, create or reconcile the real PR rather than fabricating
report fields.
