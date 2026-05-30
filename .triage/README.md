# QA Triage System

Local-only tracking (gitignored). Records finding history, occurrence locations, and fix status across all active sprint branches.

## Workflow

### 1. Intake (team-lead)
- QA verdict arrives with finding list
- For each finding, check `.triage/findings/<id>.md` — prior record may exist

### 2. Parallel Sweep (team-lead spawns N qa-triage agents)
- One agent per finding, run concurrently
- Input: `{finding_id, grep_pattern, worktrees[], prior_record_path}`
- Model: Haiku (read-only grep, no reasoning needed)

### 3. Agent Sweep (qa-triage agent)
- Read prior `.triage/findings/<id>.md` if it exists
- Grep each worktree path for the pattern
- Determine fix status per branch: pattern absent = fixed
- Update `.triage/findings/<id>.md` occurrence table
- Update `.triage/by-crate/<crate>.md` index
- Return JSON:
  ```json
  {
    "finding_id": "FTQ-001",
    "occurrences": [{"branch": "R.17", "file": "...", "line": 28, "fixed": false}],
    "promote_to": "R.17",
    "already_fixed_in": [],
    "record_path": ".triage/findings/FTQ-001.md"
  }
  ```

### 4. Consolidate (team-lead)
- Collect all N triage returns
- Group by `promote_to`
- Drop findings where `already_fixed_in` includes the highest active branch (verify only)
- **Always sweep R.17 regardless of fix status** — even if fixed in R.16, R.17 needs full occurrence check

### 5. Fix Dispatch (team-lead → arch-ctm)
- One dev-template ticket with complete hit list from triage records
- arch-ctm reads `.triage/` records for full context

## Finding Record Schema

```markdown
# <ID>: <Title>

## Pattern
grep patterns to locate this finding

## Crates Affected
list of crates

## Sprint Origin
first sprint where reported

## Status
open | fixed-partial | fixed-all

## Occurrences
| Branch | File | Line | Snippet | Fixed |
|--------|------|------|---------|-------|

## Fix History
chronological fix log

## QA Round History
which rounds reported this finding
```

## Folder Structure

```
.triage/
  README.md              — this file
  findings/              — one file per finding ID
    FTQ-001.md
    FTQ-002.md
    CI-WIN-001.md
    RBP-F001.md
    RBP-F002.md
  by-crate/              — index of findings per crate
    atm-daemon.md
    atm.md
    atm-core.md
    atm-rusqlite.md
```

## Triage Rules

- **Always sweep R.17** — finding fixed in R.16 still requires R.17 sweep for all occurrences
- **Promote to highest** — if finding exists in R.15 and R.17, fix target is R.17
- **Carry-forward detection** — if finding already in record with `open` status, it's a carry-forward; include prior context in fix ticket
- **Crate index** — update `by-crate/<crate>.md` on every new finding or occurrence update
