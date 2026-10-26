# ADR-065 — Time Handling: UTC Under The Hood, 24-Hour Local Time On Screen

| Field | Value |
| --- | --- |
| ID | ADR-065 |
| Status | Accepted |
| Date | 2026-10-18 |
| Scope | Every ATM component that stores, transmits, logs or displays a time: the daemon and CLI, `atm-bd` (console, cron, reports), the orchestration scripts and their fixtures |
| Relates to | ADR-064 |

## Context

Times reach people through several surfaces: the `atm-bd` console header
and Dispatch rows, cron digests, reports, CLI output, diagnostics files,
task events and bead notes. Some of these print a bare wall-clock in UTC,
others print local time, and a reader cannot tell which is which. On
2026-10-18 the console's Log view printed UTC clocks next to a header in
local time; the two disagreed by the UTC offset with no marker on either.

Rand's ruling (2026-10-18): "all time 'under the hood' should be utc,
displayed times are 24h local format."

## Decision

### D1. Stored and transmitted times are UTC

Every time that is persisted, sent between processes or hosts, written to
a file, or placed in a bead, task event, snapshot or report data file is
UTC, serialised as ISO-8601 with an explicit `Z` suffix (for example
`2026-10-18T06:14:02Z`). A naive datetime never crosses a boundary.

### D2. Displayed times are 24-hour local time

Every time rendered for a person on a terminal, TUI, web page or chat
digest is converted to the viewer's local zone at render time and printed
in 24-hour form (`23:14`, `23:14:02`, `2026-10-17 23:14`). No AM/PM. When
the same screen shows a date, it is the local date of that instant.

### D3. One formatter per program

Each program has one display formatter that performs the conversion in D2;
no call site formats a UTC datetime for display directly. Log and
diagnostics files are D1 surfaces (machine-readable, UTC with `Z`), even
when a person reads them.

### D4. The zone is the environment's

Local time means the zone the process runs in (`TZ`, then the host zone).
A displayed time never carries a hard-coded offset. Tests that assert
displayed times set `TZ` explicitly to a non-UTC zone.

## Consequences

- A reader can trust that any time with a `Z` is UTC and any time without
  one on a screen is their own local time.
- `atm-bd` 0.16.1's Log view violates D2 and D3 (bead atm-phase-bi.9);
  the fix routes the diagnostics entries through the console's existing
  local-time formatter and gives the diagnostics file a `Z` timestamp.
- New display code is reviewed against D3: a `strftime` on a UTC datetime
  in a render path is a finding.
