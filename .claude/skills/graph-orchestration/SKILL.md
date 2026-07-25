---
name: graph-orchestration
version: 0.4.0
description: TTL/SPARQL-driven phase orchestration. The orchestrator runs a deterministic query loop — cursor → triage-findings check → dispatch → await Completion → re-query. No agent ever decides phase, cursor position, or done-ness; queries answer all of it. Derived from codex-orchestration; replaces static sprint-doc assignments with a live RDF event log. Adds Assignment events (collision prevention), Completion invalidation (blocking post-Completion finding snaps cursor back), and CLEANUP phase (non-blocking findings after all sprints complete).
repo: atm-core
depends_on:
  quality-management-gh: 1.x
  quality-mgr: 0.x
  req-qa: 0.x
  arch-qa: 0.x
  flaky-test-qa: 0.x
  ruthless-boundary-qa: 0.x
  rust-qa-agent: 0.x
  rust-best-practices-agent: 0.x
  rust-service-hardening-agent: 0.x
  triage-findings: 1.x
---

# Graph Orchestration

Mechanical phase orchestration backed by an append-only RDF event log.
Every orchestration decision is a SPARQL query result. Agents never decide
phase, cursor position, or whether a sprint is done.

## Defaults

| Setting | Default |
|---|---|
| RDF runner | Python 3 + rdflib |
| Phase TTL location | `.sprints/<PHASE>/structure.ttl` + `.sprints/<PHASE>/events.ttl` (relative to repo root) |
| Findings storage | `.triage/*/findings/*.ttl` — managed by triage-findings skill |
| Test command | `just test` |
| Dev assignee | Set per-sprint at dispatch time (j2 variable) |
| QA reviewer set | `req-qa`, `arch-qa`, `rust-qa-agent` every pass; RBP/service-hardening/ruthless on QA-1 or finding recheck |

## Phase Setup

Before starting a phase, create two TTL files:

**`.sprints/<PHASE>/structure.ttl`** — phase identity and sprint list:

```turtle
@prefix triage: <urn:atm:triage:> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

triage:Phase<PHASE> a triage:Phase .

triage:<PHASE>-S1 a triage:Sprint ; triage:inPhase triage:Phase<PHASE> ; triage:order 1 ; triage:criteria "ac/<PHASE>S1.md" .
triage:<PHASE>-S2 a triage:Sprint ; triage:inPhase triage:Phase<PHASE> ; triage:order 2 ; triage:criteria "ac/<PHASE>S2.md" .
# ... one entry per sprint, orders unique and contiguous from 1
```

**`.sprints/<PHASE>/events.ttl`** — empty initially; events (Assignments, Completions)
are appended here as sprints progress. Never edited or deleted.

**`ac/<PHASE>S*.md`** — acceptance criteria documents, one per sprint. The
`triage:criteria` value must be a resolvable path. Create all `ac/` docs before
the phase starts.

Run `validate-structure.sparql` at creation. It must return zero rows.

## Orchestrator Loop

```bash
RESULT=$(next-dev-task F .sprints/F)
PHASE=$(echo "$RESULT" | jq -r .phase)
```

Four outcomes from the cursor query:

| `phase` | Meaning |
|---|---|
| `TRAVERSAL` | A sprint is ready (no in-flight Assignment, no valid Completion) — check triage-findings to pick template |
| `AWAITING` | Cursor is empty but some sprints lack valid Completions — in-flight assignments are being worked |
| `CLEANUP` | All sprints have valid Completions but open non-blocking findings remain |
| `DONE` | All sprints have valid Completions and no open non-blocking findings — phase complete |

After getting a `TRAVERSAL` result, the orchestrator:
1. Appends an Assignment event to events.ttl for this sprint
2. Checks `.triage/` for blocking/important/minor findings (via triage-findings skill)
3. Picks the template: no findings → dev-task.xml.j2; blocking findings present → dev-fix.xml.j2

```
cursor_result = next-dev-task F .sprints/F

if cursor_result.phase == "DONE":
    → phase complete, merge

if cursor_result.phase == "AWAITING":
    → wait for in-flight dev work; poll again after expected delivery

if cursor_result.phase == "CLEANUP":
    → dispatch dev-fix.xml.j2 for open non-blocking findings

# TRAVERSAL path
append Assignment event to events.ttl for cursor sprint
findings = run triage-findings skill for cursor sprint

if findings has blocking:
    → dispatch dev-fix.xml.j2
else:
    → dispatch dev-task.xml.j2
```

**Hard rules:**
- Never cache phase or cursor across events. Re-run `next-dev-task` after every
  Completion or Assignment.
- Never interpret findings in this skill — that is triage-findings' job. If
  the orchestrator is tempted to reason about the graph, the model is broken —
  file a design issue, do not improvise.
- QA runs after every dev Completion. Trigger immediately on Completion receipt.
- A Completion can be invalidated: if QA files a blocking finding with
  foundAt > completedAt, the Completion is invalid and the cursor snaps back.
  Team-lead re-dispatches by appending a new Assignment.
- Default to consolidated CLEANUP on the highest sprint branch. Dispatching
  non-blocking findings back to origin branches causes 3–4× more QA cycles —
  avoid unless there is a specific reason (e.g. a finding is isolated to an
  early sprint with no forward merge path).
- **Team-lead is sole writer of TTL events. Never delegate TTL writes to dev agents.**

## Dev Dispatch (no blocking findings)

`next-dev-task` returns the cursor sprint, its order, and its criteria path.
Populate and send `dev-task.xml.j2`:

| j2 variable | Source |
|---|---|
| `sprint` | `result.vars.sprint` |
| `sprint_order` | `result.vars.sprint_order` |
| `criteria_doc` | `result.vars.criteria_doc` |

## Fix Dispatch (blocking findings present)

Same cursor sprint, but triage-findings reports blocking findings on it.
Use `dev-fix.xml.j2` instead. The findings payload comes from triage-findings,
not from events.ttl.

## QA Gate

After every dev Completion, assign QA to `quality-mgr`. Reviewer selection
depends on which pass this is:

**Every QA pass:**
- `req-qa`
- `arch-qa`
- `rust-qa-agent`

**QA-1 only** (first QA pass on a sprint node):
- `ruthless-boundary-qa`
- `rust-best-practices-agent`
- `rust-service-hardening-agent`

**Conditional:**
- `flaky-test-qa` — only when flaky or long-running CI failures are present in this sprint

**Subsequent passes:** these three run only if they have an open finding from
this sprint node — i.e., they need to verify their own fix was addressed.
Do not re-run them if they filed nothing or all their findings are resolved.

QA rules:
- QA never edits existing findings. Re-assessments file a **new** finding at
  the new severity.
- QA appends findings to `.triage/` via the triage-findings skill — not to
  events.ttl.
- Merge gate: 0 Blocking + 0 Important + 0 Minor, no exceptions.

After QA completes, re-run `next-dev-task` to get the new cursor, then run
triage-findings to determine template selection.

## Appending Events

**Team-lead is the sole writer of TTL events.** Agents (dev, QA) never append to
`.sprints/` files directly. Dev sends an ATM completion message; team-lead
appends triage:Completion and triage:Resolution events, then validates.

Assignments and Completions go into `events.ttl`. They are file-appended and
committed immediately. Use UTC timestamps from the local clock — never from
agent self-report.

```bash
# Assignment — append when team-lead dispatches a sprint to a dev agent
cat >> .sprints/<PHASE>/events.ttl <<'TTL'
triage:a<N> a triage:Assignment ;
    triage:ofSprint triage:Phase<X>-S<n> ;
    triage:assignedTo "arch-ctm" ;
    triage:assignedAt "<UTC>"^^xsd:dateTime .
TTL
git add .sprints/<PHASE>/events.ttl && git commit -m "event: Assignment <PHASE>-S<n>"
# Validate after every append
.claude/skills/graph-orchestration/scripts/next-dev-task F .sprints/F --validate-only

# Completion — append when team-lead receives dev's ATM completion message
cat >> .sprints/<PHASE>/events.ttl <<'TTL'
triage:c<N> a triage:Completion ;
    triage:ofSprint triage:Phase<X>-S<n> ;
    triage:at "<UTC>"^^xsd:dateTime .
TTL
git add .sprints/<PHASE>/events.ttl && git commit -m "event: Completion <PHASE>-S<n>"
# Validate after every append
.claude/skills/graph-orchestration/scripts/next-dev-task F .sprints/F --validate-only

# Resolution — append when team-lead confirms a non-blocking finding is fixed
cat >> .sprints/<PHASE>/events.ttl <<'TTL'
triage:r<N> a triage:Resolution ;
    triage:resolves triage:f<N> ;
    triage:resolvedAt "<UTC>"^^xsd:dateTime .
TTL
git add .sprints/<PHASE>/events.ttl && git commit -m "event: Resolution f<N>"
# Validate after every append
.claude/skills/graph-orchestration/scripts/next-dev-task F .sprints/F --validate-only
```

**Assignment uniqueness**: Each Assignment must include the dev agent name and
be unique per sprint (use a monotonically increasing `<N>` suffix).

**Blocking findings**: Closed by a new valid Completion — no Resolution needed.
Blocking findings posted after a Completion invalidate it (see Completion
Invalidation below).

**Non-blocking findings**: Closed by an explicit Resolution in events.ttl.

Findings live exclusively in `.triage/*/findings/*.ttl` and are managed by the
triage-findings skill. Do not append raw finding data to events.ttl.

## sc-compose Integration

`next-dev-task` returns JSON shaped for direct sc-compose consumption:

```json
{
  "phase": "TRAVERSAL",
  "vars": {
    "sprint": "S7",
    "sprint_iri": "urn:atm:triage:S7",
    "sprint_order": 1,
    "criteria_doc": "ac/S7.md"
  }
}
```

The orchestrator saves `vars` to a temp file and adds non-graph variables.
Template selection (`dev-task.xml.j2` vs `dev-fix.xml.j2`) is made after
consulting triage-findings:

```bash
RESULT=$(next-dev-task F .sprints/F)
PHASE_VAL=$(echo "$RESULT" | jq -r .phase)

if [ "$PHASE_VAL" = "DONE" ]; then
  echo "Phase complete."
  exit 0
fi

# Write graph-derived vars
echo "$RESULT" | jq .vars > /tmp/graph-vars.json
SPRINT=$(echo "$RESULT" | jq -r .vars.sprint)

# Check findings via triage-findings skill (orchestrator step)
# If blocking findings exist, use dev-fix.xml.j2; otherwise dev-task.xml.j2
TEMPLATE="dev-task.xml.j2"   # set by orchestrator after triage-findings check

# Render via sc-compose (orchestrator supplies remaining vars)
sc-compose render \
  --file ".claude/skills/graph-orchestration/$TEMPLATE" \
  --var-file /tmp/graph-vars.json \
  --var task_id="GO-$(date +%s)" \
  --var worktree_path="$WORKTREE_PATH" \
  --var branch="$BRANCH" \
  --var pr_target="$PR_TARGET" \
  --var assignee="arch-ctm"
```

## Completion Invalidation

A Completion is valid only if no blocking finding for that sprint was filed
with `foundAt > completedAt`. If QA files a blocking finding after a
Completion, the Completion is invalidated:

1. `cursor.sparql` snaps back to that sprint (the truly-in-flight filter
   does not block — dev has sent a Completion, so the sprint is not in-flight;
   a new Assignment is required for re-dispatch)
2. Team-lead appends a new Assignment event to events.ttl for the sprint
3. Dev fixes the blocker and sends an ATM completion message; team-lead
   appends a new Completion
4. Dev merges forward into the next sprint's worktree, picking up any
   important/minor findings on the way (Step 4 in the j2 template)

This guarantees QA always has the final word. A sprint is never permanently
"done" while a blocking finding exists postdating its Completion.

## Cleanup Pass

When `next-dev-task` returns `phase: "CLEANUP"`, all sprint Completions are valid
and CI is green, but open important/minor findings remain. Fix ALL of them in a
single pass on the highest sprint branch.

**Why consolidated:** fixing findings on the branch where they were found causes
8–12 dev-QA cycles. Fixing all findings on one merged branch reduces this to 2–3.
This 3–4× speedup is load-bearing — do not deviate.

**Orchestrator steps (in order, before dispatching dev-fix):**

1. Confirm all sprint Completions are valid and CI is green on each sprint branch.
2. Merge sprint branches forward in sequence into the highest-order sprint branch:
   - Merge S1 → S2's branch
   - Merge S2 → S3's branch
   - ... up to S(n-1) → S(n)'s branch
3. Run CI on the merged S(n) branch. Fix any merge conflicts before proceeding.
4. Collect all open important/minor findings via `open-findings-sprint.sparql`.
5. Dispatch ONE dev-fix assignment to S(n)'s worktree with the full findings list.
6. QA reviews the merged branch once.

**Strong default:** Fix important/minor findings on the consolidated highest
sprint branch during CLEANUP. Dispatching findings back to the branch where
they were found causes 3–4× more QA cycles and is rarely worth it. Only
deviate if a finding is completely isolated to an early sprint with no
forward merge dependency and re-merging would be higher churn than fixing
in place.

**CLEANUP branch variables for sc-compose:**
- `worktree_path` = highest-order sprint's worktree
- `branch` = highest-order sprint's branch
- `pr_target` = phase integration branch
- `findings` = full output of `open-findings-sprint.sparql` (all open important/minor)
- `cleanup_mode` = "true"

## Ontology

The `triage:` prefix maps to `urn:atm:triage:`. Classes used by
graph-orchestration:

| Class | Properties | Notes |
|---|---|---|
| `triage:Phase` | — | Phase identity node |
| `triage:Sprint` | `inPhase`, `order`, `criteria` | One per sprint; `order` is unique within a phase |
| `triage:Assignment` | `ofSprint`, `assignedTo`, `assignedAt` | Appended by team-lead when dispatching; must be unique per sprint |
| `triage:Completion` | `ofSprint`, `at` | Appended by team-lead on receipt of dev's ATM completion message; may be invalidated by a later blocking finding |
| `triage:Resolution` | `resolves`, `resolvedAt` | Appended by team-lead when a non-blocking finding is confirmed fixed; blocking findings need no Resolution |

Findings are defined by the triage-findings skill and live in
`.triage/*/findings/*.ttl`. Resolution events referencing those findings are
appended to events.ttl by team-lead.

## Scripts

All scripts live in `.claude/skills/graph-orchestration/scripts/`:

| Script | Purpose |
|---|---|
| `next-dev-task` | Entry point: cursor resolution, returns JSON |
| `query_runner.py` | Python SPARQL runner (rdflib) |
| `cursor.sparql` | Returns cursor sprint (lowest-ordered sprint without a truly in-flight Assignment or valid Completion); parameter: `$PHASE` |
| `open-findings-sprint.sparql` | Returns open non-blocking findings across the phase (used for CLEANUP detection); parameter: `$PHASE` |
| `all-complete.sparql` | Returns sprints lacking a valid Completion; zero rows = all sprints done, proceed to CLEANUP/DONE check; parameter: `$PHASE` |
| `validate-structure.sparql` | Phase structure integrity check — zero rows = valid; parameter: `$PHASE` |
| `test_queries.py` | pytest unit tests for all SPARQL queries (run: `python3 -m pytest scripts/test_queries.py -v`) |

Usage:
```bash
# From repo root — make executable first: chmod +x .claude/skills/graph-orchestration/scripts/next-dev-task
.claude/skills/graph-orchestration/scripts/next-dev-task F .sprints/F
```

Output JSON (TRAVERSAL):
```json
{
  "phase": "TRAVERSAL",
  "vars": {
    "sprint": "PhaseF-S1",
    "sprint_iri": "urn:atm:triage:PhaseF-S1",
    "sprint_order": 1,
    "criteria_doc": "ac/FS1.md"
  }
}
```

Output JSON (AWAITING):
```json
{
  "phase": "AWAITING",
  "vars": {},
  "_incomplete_sprints": ["urn:atm:triage:PhaseF-S1"]
}
```

Output JSON (CLEANUP):
```json
{
  "phase": "CLEANUP",
  "vars": {},
  "_findings_raw": [...]
}
```

Output JSON (DONE):
```json
{
  "phase": "DONE",
  "vars": {},
  "_findings_raw": []
}
```

## Assignment Templates

- `dev-task.xml.j2` — initial dev pass (no blocking findings)
- `dev-fix.xml.j2` — fix pass (blocking findings identified by triage-findings)

QA assignment uses the existing `quality-mgr` prompt directly — no new template.

## Required Message Sequence

Every ATM task message must follow:
1. ACK
2. Work
3. Completion summary (including git push SHA)
4. Completion ACK by receiver
