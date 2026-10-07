---
name: qa-report
description: Show the two QA metrics logs (per-round event log and cumulative phase-stats) from .sc/qa-log/.
---

# QA Report

Displays the two QA metrics logs quality-mgr appends to on every QA-bead
close. Read-only; writes nothing.

## Arguments

- `/qa-report`: display exactly the 10 most recent rows from each log.
- `/qa-report N`: display exactly the N most recent rows from each log,
  where N is a positive integer.
- `/qa-report --all`: display all rows from each log.

All forms report the current phase. If a log contains fewer than the requested
number of rows, display all available rows without padding or duplication.

## Execution

1. Locate the logs under `.sc/qa-log/`: the per-round event log
   (`phase-<p>.jsonl`, "Log B") and its cumulative stats sibling
   (`phase-<p>-stats.jsonl`, "Log A"), for the current phase. If neither
   exists, say so and stop.
2. Sort Log A by `snapshot_at` descending and Log B by `completed_at`
   descending, then apply the requested row count (default 10; unlimited
   with `--all`). Render the selected rows in two tables with these columns:
   - **Log A — cumulative phase-stats** (`phase-<p>-stats.jsonl`): columns
     `snapshot_local`, `tot`, `open`, `blk`, `imp`, `min`, `trigger_task`.
   - **Log B — per-round events** (`phase-<p>.jsonl`): columns
     `completed_local`, `task`, `sprint`, `tested`, `iteration`, `verdict`,
     `blk`, `imp`, `min`, `fnd`, `duration`, `pr_number`.
   The JSONL `snapshot_at` and `completed_at` timestamps are UTC. Convert
   those timestamps to the user's local timezone for the `snapshot_local`
   and `completed_local` display columns (`HH:MM`); do not rewrite the logs.

3. For display only, shorten `task` and `trigger_task` to the substring
   starting at the final `qa-pr<number>` or `sanity-pr<number>` component,
   preserving any following suffix. For example,
   `pfx-d-23-entity-id-compatibility-qa-pr651-f2-qa-pr686-f1-qa-pr742`
   displays as `qa-pr742`. Leave IDs without either component unchanged.
   Preserve all other column names, values and formatting.

## Output

Both tables, labeled `QA Run Log` and `QA Statistics`, for the current phase. Nothing else. Write the
tables as markdown directly in your reply text; a command's own stdout is
not reliably shown to the user, so never rely on tool output alone.
