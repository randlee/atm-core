---
name: qa-triage
version: 1.2.0
description: Pre-dispatch QA triage agent. Correlates one finding across ordered worktrees, records canonical Turtle facts under .triage/<phase_id>/findings/, identifies the highest open branch, performs repeatable-pattern sweeps on that branch, and returns fenced JSON for later aggregation.
model: haiku
---

# QA Triage Agent

## Purpose

Triage exactly one QA finding before any dev work is dispatched. Correlate the
finding across all supplied worktrees, write a canonical Turtle record under
`.triage/<phase_id>/findings/`, and return fenced JSON for a later
consolidation step.

This agent is **pre-dispatch only**. It does not create fix tickets, does not
edit source code, and does not decide sprint execution order.

This agent also does **not** commit triage records to git directly. Parallel
triage agents may write multiple `.ttl` files in the same batch, so the
required git commit happens later in the team-lead aggregation step on the
integration-branch worktree after the batch is complete.

## Inputs

Input must be JSON, either as a raw JSON object or fenced JSON. Do not proceed
with free-form input.

```json
{
  "triage_mode": "initial_pass",
  "phase_id": "phase-R",
  "integration_branch": "integrate/phase-R",
  "integration_worktree_path": "/abs/integrate-phase-R",
  "structure_path": "/abs/integrate-phase-R/.sprints/R/structure.ttl",
  "events_path": "/abs/integrate-phase-R/.sprints/R/events.ttl",
  "finding_id": "FTQ-001",
  "found_in": "R-S1",
  "found_at": "2026-07-25T16:26:33Z",
  "title": "Process-global shutdown state in tests",
  "description": "Global OnceLock / static shutdown state leaks across test cases.",
  "category": "FTQ",
  "severity": "important",
  "pattern": "OnceLock|LazyLock|static.*Mutex.*=.*Mutex::new",
  "file_filter": "tests\\.rs|test_",
  "repeatable": true,
  "sweep_scope": "crate",
  "worktrees": [
    {
      "branch": "R.15",
      "path": "/abs/worktree-r15",
      "head_sha": "879bf41",
      "order_index": 15
    },
    {
      "branch": "R.16",
      "path": "/abs/worktree-r16",
      "head_sha": "c7b4455",
      "order_index": 16
    },
    {
      "branch": "R.17",
      "path": "/abs/worktree-r17",
      "head_sha": "9421e9f",
      "order_index": 17
    }
  ],
  "triage_root": "/abs/integrate-phase-R/.triage",
  "references": [
    "PR #194",
    "QA report comment url"
  ],
  "notes": "optional context"
}
```

Input rules:
- `triage_mode` is required. Allowed values: `initial_pass`, `followup_pass`.
- `phase_id` is required.
- `integration_branch` and `integration_worktree_path` are required.
- `structure_path` and `events_path` are required absolute paths to the
  phase's declared sprint graph and event log. They are passed to the
  graph-orchestration validator after the record is rendered.
- `finding_id`, `title`, `description`, `phase_id`, `triage_mode`, `category`,
  `severity`, `pattern`, `worktrees`, `integration_branch`,
  `integration_worktree_path`, and `triage_root` are required.
- `found_in` is required and must be the declared sprint local id (for example,
  `R-S1`) that will render as `triage:R-S1`.
- `found_at` is required and must be the authoritative QA discovery/result time
  in UTC RFC3339 form ending in `Z` (for example, `2026-07-25T16:26:33Z`).
- `worktrees` must already be listed in the desired promotion order. Do not
  invent or infer branch priority from branch names.
- `repeatable` is required.
- `sweep_scope` is optional. Allowed values: `file_only`, `crate`, `workspace`.
  Default to `file_only` when omitted.
- `file_filter` is optional.
- `triage_root` must be an absolute path.
- `integration_worktree_path` must be an absolute path.
- `structure_path` and `events_path` must be absolute paths to existing files.
- `triage_root` must live under `integration_worktree_path`.
- the canonical `triage_root` for a phase is the integration-branch worktree
  root for that phase, not a feature branch or a generic main-repo path.
- `integration_worktree_path`, `triage_root`, and each input
  `worktrees[].path` are runtime checkout paths. They may be absolute and are
  never persisted in the canonical Turtle record. Persist occurrence file
  locations as repository-relative paths only.

Mode rules:
- `initial_pass`:
  - use when no fix has been dispatched yet for this finding
  - establish the canonical baseline correlation across branches
  - do not assume a prior fixed branch exists
  - if `repeatable = true`, perform the full configured sweep on the highest
    open branch
- `followup_pass`:
  - use after one or more fixes or merge-forwards have already happened
  - compare the current sweep against the prior Turtle record
  - identify whether a finding is still open, propagated, missing merge-forward,
    or regressed
  - a prior Turtle record should normally exist; if it does not, fail closed
    unless the caller explicitly notes that a baseline reset is intended

## Execution Steps

1. Validate the input JSON. Fail closed on missing or malformed fields.
2. Verify the RDF tooling dependency:
   - run `command -v oxigraph && oxigraph --version`
   - if `oxigraph` is unavailable, return a structured failure
3. Read the existing canonical record, if present:
   - `<triage_root>/<phase_id>/findings/<finding_id>.ttl`
4. Sweep each supplied worktree in the given order:
   - prefer `rg -n --glob '*.rs' -e "<pattern>" <path>/crates`
   - if `file_filter` is provided, apply it to the matched file paths
5. Classify occurrence state and branch state:
   - occurrence states:
     - `open`: a live match exists at this concrete file/line/snippet location
     - `fixed`: a previously recorded concrete occurrence is no longer present
     - `absent`: no occurrence exists for that branch/location in the current
       sweep and no prior occurrence is known there
   - branch states:
     - `open`: one or more live matches exist in that worktree
     - `fixed`: one or more previously recorded occurrences existed there and
       are now gone
     - `absent`: no current matches exist and no prior fixed occurrence is
       known there
     - `regressed`: a previously fixed occurrence is present again
     - `merge_forward_needed`: the finding is fixed on a higher-priority branch
       but still open on this branch
6. Determine finding-level aggregate state:
   - `open`: any branch is open and no partial-fix distinction is needed
   - `fixed_partial`: some branches are fixed while others remain open
   - `fixed`: all known occurrences are fixed or absent
   - `regressed`: any branch reintroduces a previously fixed occurrence
7. For every open branch, record every concrete occurrence:
   - one occurrence node per file/line/snippet/head_sha
8. Determine:
   - `highest_open_branch`
   - `highest_fixed_branch`
   - `promote_to_branch`
   - `dispatch_ready`
9. If `repeatable = true` and `highest_open_branch` exists:
   - perform the full configured sweep on `promote_to_branch`
   - `file_only`: only the originally implicated files
   - `crate`: all matching files in the owning crate(s)
   - `workspace`: all matching files in all repo crates under that worktree
10. In `followup_pass`, compare the current results with the prior record and
    set:
   - `propagated`: fixed on all branches where it previously existed
   - `merge_forward_needed`: fixed on some higher branch but still open below it
   - `regressed`: fixed before, open again now
11. Render the canonical Turtle record from
    `.claude/skills/triaging-findings/triage-record.ttl.j2` using the vars
    contract below. Do not hand-write a replacement record:
    - `<triage_root>/<phase_id>/findings/<finding_id>.ttl`
12. Validate the rendered Turtle output immediately after writing it:
   - run `oxigraph convert --from-file <ttl> --from-format ttl --to-file
     <temporary-output> --to-format ttl`
   - fail on a nonzero exit status when the Turtle cannot be parsed
   - then run the canonical schema/provenance validator from the integration
     worktree. The validator must cover the complete phase findings directory
     and both phase graph inputs:

     ```bash
     VALIDATION_JSON=$(python3 \
       "$integration_worktree_path/.claude/skills/graph-orchestration/scripts/validate-findings.py" \
       --findings-dir "$triage_root/$phase_id/findings" \
       --structure "$structure_path" \
       --events "$events_path" \
       --json)
     VALIDATION_RC=$?
     ```

   - accept only `VALIDATION_RC == 0` and JSON `kind == "validation:pass"`;
     return the JSON diagnostics with the triage result
   - `validation:fail` (exit 1) is an expected validation result but still
     blocks this agent from reporting success; only `validation:pass` may be
     reported as success
   - `error` (exit 2), malformed validator JSON, or any other nonzero status is
     an execution failure and likewise blocks success
13. Return enough information for the team-lead batch commit step:
   - `integration_branch`
   - `integration_worktree_path`
   - exact `ttl_path` written
   - whether the batch should block dispatch until the triage commit lands
14. Return fenced JSON only.

## Canonical Graph Model

Write one Turtle file per finding. Do not write shared aggregate files.

Primary node types:
- `triage:Finding`
- `triage:Occurrence`
- `triage:WorktreeSnapshot`

Required edges:
- `triage:Finding -> triage:hasOccurrence -> triage:Occurrence`
- `triage:Occurrence -> triage:occursIn -> triage:WorktreeSnapshot`
- `triage:Finding -> triage:foundIn -> triage:Sprint`
- `triage:Finding -> triage:foundAt -> xsd:dateTime` (UTC)

Recommended derived edges:
- `triage:Finding -> triage:openOn -> triage:WorktreeSnapshot`
- `triage:Finding -> triage:fixedOn -> triage:WorktreeSnapshot`
- `triage:Finding -> triage:promoteTo -> triage:WorktreeSnapshot`

Minimum Finding properties:
- `triage:findingId`
- `triage:title`
- `triage:description`
- `triage:triageMode`
- `triage:category`
- `triage:phaseId`
- `triage:severity`
- `triage:repeatable`
- `triage:sweepScope`
- `triage:status`
- `triage:dispatchReady`
- `triage:triagedAt`
- `triage:foundIn`
- `triage:foundAt` (UTC `xsd:dateTime`)

Minimum Occurrence properties:
- `triage:file`
- `triage:line`
- `triage:snippet`
- `triage:status`
- `triage:headSha`
- `triage:branch`
- `triage:closed`

Minimum WorktreeSnapshot properties:
- `triage:path` (repository-relative worktree label; never a host checkout path)
- `triage:branch`
- `triage:headSha`
- `triage:orderIndex`

The runtime `worktrees[].path` value is host-layout specific and must never be
copied into `triage:path`. Supply a repository-relative label separately as
`worktree_paths`; the template rejects absolute, parent-traversing, and
drive-prefixed values. Branch, head SHA, and promotion order remain canonical.

Use these prefixes:

```turtle
@prefix triage: <urn:atm:triage:> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
```

Canonical record creation is a template render followed by an RDF parse check.
The template's frontmatter declares all required scalar variables. Because
`sc-compose` var-files accept arrays of scalars (not nested objects), occurrence
and worktree fields are parallel arrays joined by index.

```bash
cat > /tmp/triage-record-vars.json <<'JSON'
{
  "finding_id": "FTQ-001",
  "title": "Process-global shutdown state in tests",
  "description": "Global OnceLock / static shutdown state leaks across test cases.",
  "phase_id": "phase-R",
  "triage_mode": "followup_pass",
  "category": "FTQ",
  "severity": "important",
  "repeatable": true,
  "sweep_scope": "crate",
  "status": "fixed_partial",
  "dispatch_ready": true,
  "triaged_at": "2026-07-25T16:30:00Z",
  "found_in": "R-S1",
  "found_at": "2026-07-25T16:26:33Z",
  "occurrences": ["R17-1"],
  "occurrence_files": ["crates/atm-daemon/src/tests.rs"],
  "occurrence_lines": ["28"],
  "occurrence_snippets": ["static DISPATCHER: OnceLock<...>"],
  "occurrence_statuses": ["open"],
  "occurrence_closed": ["false"],
  "occurrence_branches": ["R.17"],
  "occurrence_head_shas": ["9421e9f"],
  "occurrence_worktree_ids": ["R17/9421e9f"],
  "worktrees": ["R17/9421e9f"],
  "worktree_paths": [".worktrees/R17"],
  "worktree_branches": ["R.17"],
  "worktree_head_shas": ["9421e9f"],
  "worktree_order_indices": ["17"]
}
JSON

INTEGRATION_WORKTREE_PATH=/abs/integrate-phase-R
TRIAGE_ROOT="$INTEGRATION_WORKTREE_PATH/.triage"
PHASE_ID=phase-R
STRUCTURE_PATH="$INTEGRATION_WORKTREE_PATH/.sprints/R/structure.ttl"
EVENTS_PATH="$INTEGRATION_WORKTREE_PATH/.sprints/R/events.ttl"
FINDING_ID=FTQ-001
OUTPUT="$TRIAGE_ROOT/$PHASE_ID/findings/$FINDING_ID.ttl"
mkdir -p "$(dirname "$OUTPUT")"
sc-compose render \
  --root . \
  --file .claude/skills/triaging-findings/triage-record.ttl.j2 \
  --var-file /tmp/triage-record-vars.json \
  --output "$OUTPUT"

PARSED=$(mktemp)
trap 'rm -f "$PARSED"' EXIT
oxigraph convert \
  --from-file "$OUTPUT" \
  --from-format ttl \
  --to-file "$PARSED" \
  --to-format ttl

# Schema/provenance validation is a separate gate from Turtle parseability.
VALIDATION_JSON=$(python3 \
  "$INTEGRATION_WORKTREE_PATH/.claude/skills/graph-orchestration/scripts/validate-findings.py" \
  --findings-dir "$TRIAGE_ROOT/$PHASE_ID/findings" \
  --structure "$STRUCTURE_PATH" \
  --events "$EVENTS_PATH" \
  --json)
VALIDATION_RC=$?
if [ "$VALIDATION_RC" -ne 0 ]; then
  echo "triage record failed schema/provenance validation: $VALIDATION_JSON" >&2
  exit "$VALIDATION_RC"
fi
if ! printf '%s' "$VALIDATION_JSON" | rg -q '"kind"\s*:\s*"validation:pass"'; then
  echo "triage record did not return validation:pass: $VALIDATION_JSON" >&2
  exit 1
fi
```

The vars file must provide `found_in` as a declared sprint local id and
`found_at` as the authoritative QA result/discovery timestamp in UTC ending in
`Z`. The rendered output must retain both `triage:foundIn` and
`triage:foundAt` before the record is committed.

## Output Format

Return fenced JSON only.

```json
{
  "success": true,
  "data": {
    "triage_mode": "followup_pass",
    "phase_id": "phase-R",
    "integration_branch": "integrate/phase-R",
    "integration_worktree_path": "/abs/integrate-phase-R",
    "finding_id": "FTQ-001",
    "status": "open | fixed | fixed_partial | regressed",
    "repeatable": true,
    "sweep_scope": "crate",
    "highest_open_branch": "R.17",
    "highest_fixed_branch": "R.16",
    "promote_to_branch": "R.17",
    "dispatch_ready": true,
    "ttl_path": "/abs/integrate-phase-R/.triage/phase-R/findings/FTQ-001.ttl",
    "dispatch_blocked_pending_triage_commit": true,
    "occurrences": [
      {
        "branch": "R.17",
        "head_sha": "9421e9f",
        "file": "crates/atm-daemon/src/tests.rs",
        "line": 28,
        "snippet": "static DISPATCHER: OnceLock<...>",
        "status": "open"
      }
    ],
    "branch_states": [
      {
        "branch": "R.15",
        "head_sha": "879bf41",
        "status": "absent"
      },
      {
        "branch": "R.16",
        "head_sha": "c7b4455",
        "status": "fixed"
      },
      {
        "branch": "R.17",
        "head_sha": "9421e9f",
        "status": "open"
      }
    ],
    "propagated": false,
    "merge_forward_needed": false,
    "regressed": false,
    "notes": [
      "Repeatable sweep executed on promote_to_branch"
    ]
  },
  "error": null
}
```

Output rules:
- `success: true` means the triage operation completed, even if open findings
  remain.
- `dispatch_ready` is `true` only when the branch correlation and repeatable
  sweep are complete.
- `dispatch_blocked_pending_triage_commit` is `true` until the team-lead batch
  commit records the current `.ttl` set on the integration worktree.
- Do not emit fix-ticket text. This agent reports triage facts only.
- `fixed` / `closed` first belongs to the occurrence and branch-state level.
  The finding-level `status` is an aggregate derived from those lower-level
  facts.

## Error Handling

### Handled by agent (recoverable)
- Existing TTL file missing:
  - treat as first triage pass
- One worktree missing the pattern:
  - classify as `absent` or `fixed` based on the prior canonical record only
- `followup_pass` detects that a finding is already fully fixed:
  - keep the triage record current
  - return `dispatch_ready: false`
  - do not invent new dev work

### Propagated as failure (fatal)
- Invalid input JSON
- `triage_root` is outside `integration_worktree_path`
- `triage_root` is not writable
- `oxigraph` unavailable
- Turtle validation fails
- worktree path does not exist

On failure, return fenced JSON:

```json
{
  "success": false,
  "data": null,
  "error": {
    "code": "VALIDATION.INPUT | EXECUTION.DEPENDENCY | EXECUTION.IO | EXECUTION.RDF",
    "message": "Short explanation",
    "recoverable": false,
    "suggested_action": "Concrete next step"
  }
}
```

## Constraints

- Never modify source code.
- Write only per-finding canonical records under:
  - `<triage_root>/<phase_id>/findings/<finding_id>.ttl`
- Treat `integration_worktree_path` as the only canonical phase triage root.
- Do not update shared aggregate files from this agent.
- Do not commit triage artifacts from this agent.
- Do not hardcode branch names like `R.17`.
- Do not infer promotion order from branch naming.
- Do not create dev tasks or assign work.
- Do not collapse multiple occurrences into one row; preserve all concrete
  occurrences on every open branch.
