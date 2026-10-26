# ADR-067 — One Structured Log Per Program; Viewers Read It, Never A Second File

| Field | Value |
| --- | --- |
| ID | ADR-067 |
| Status | Accepted |
| Date | 2026-10-18 |
| Scope | Every ATM program with an operational log and a view onto it: `atm-bd` (console Diagnostics view, cron), the daemon's log readers, future tools |
| Relates to | ADR-065 (log files are UTC surfaces), ADR-064 |

## Context

`atm-bd` 0.16.1 writes its operational events through the sc-observability
logger (one JSONL file per repository) and, separately, a hand-built text
file `diagnostics.log` that the Diagnostics view reads. The hand-built file
dropped the command, exit code and stderr of every failure, printed a UTC
clock with no marker, and re-recorded a cached GitHub error on every tick,
while the structured log already held all of that data (bead
atm-phase-bi.13). The structured entries were written with level Info and
an empty message, so a viewer reading them would have had nothing to show.

Rand's rulings (2026-10-18):

- "the <fix> <error kind> <details> should just come from the log entries.
  i.e. I don't want the diagnostic viewer doing any special processing of
  structured log message besides time and error level extract."
- "remove the 'hand-built' test file completely."
- The entry form: "07;11:18 error: `check gh and retry` command_failed
  <more-details-in-log-entry>", refined to
  "00:10:18 error: `check gh and retry` gh api graphql [1] gh: API rate
  limit already exceeded for user ID 8083956."

## Decision

### D1. One log per program

A program writes its operational events to one structured log through the
sc-observability logger. It keeps no second diagnostics or error file.

### D2. The writer sets the level and composes the message

Every entry carries its real level (error, warning, info) and a one-line
human message composed once, at the writer. For a failed command the
message is `<fix in backticks> <command words> [<exit code>] <first stderr
line>`, where the command words are the executable and its subcommand, not
the full argument vector.

### D3. A viewer extracts time and level only

A view onto the log (a Diagnostics tab, a digest) reads the structured log,
renders the timestamp per ADR-065 D2 (24-hour local), prints the level, and
prints the entry's message unchanged. It parses nothing else out of the
entry.

### D4. One failure, one entry

A failure is logged once, when it happens. A cached failure that is still
being shown (for example the last GitHub error kept with cached PR data)
is a status on the sheet that shows the cache, not a new log entry per
tick.

## Consequences

- The Diagnostics view and the log file always agree, because they are
  the same data.
- Review check: a program-written text file that duplicates log content,
  a viewer that re-parses an entry's fields, or an error entry at level
  Info, is a finding.
- Any other hand-written trace file a program keeps (for example an input
  trace) is reviewed against D1 and either folded into the structured log
  or removed by a separate decision.
