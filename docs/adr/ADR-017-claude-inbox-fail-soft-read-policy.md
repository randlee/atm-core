# ADR-017 — Claude Inbox Fail-Soft Read Policy

| Field | Value |
| --- | --- |
| ID | ADR-017 |
| Status | accepted |
| Date | 2026-06-06 |
| Deciders | arch-ctm, team-lead |
| Relates-to | ADR-010, ADR-012 |
| Supersedes | — |

## Context

ATM now treats the current Claude `.json` inbox array as the primary shared
inbox path. That makes malformed current-Claude inbox content an operational
boundary problem, not a legacy-format edge case.

One malformed fragment inside a current Claude inbox array could previously
hide unrelated valid messages behind a whole-file parser error. Missing a
recoverable sprint message is worse than surfacing a degraded warning alongside
the valid messages that still parse.

At the same time, normal send/ack rewrite paths must not silently rewrite a
malformed current-Claude inbox array and discard evidence of corruption.

## Decision

ATM adopts a fail-soft read policy for current Claude inbox reads.

Rules:
- current Claude inbox reads must salvage segmentable valid message objects
  from malformed `.json` arrays whenever possible
- malformed localized fragments must surface explicit degraded warnings/items
  rather than hiding unrelated valid messages
- malformed tolerated additive data such as unknown metadata or historical
  derivative fields must not cause a whole-inbox read failure
- only root corruption with no segmentable valid message units remains a
  terminal read failure
- normal send/ack compatibility rewrite paths remain fail-closed on malformed
  current-Claude inbox arrays
- explicit repair/rebuild remains the only approved rewrite seam for malformed
  current-Claude inbox content

## Consequences

Required implementation consequences:
- mailbox-read helpers expose a degraded-item surface internally so callers can
  distinguish valid messages from recovered malformed fragments
- current-Claude array parsing first attempts normal parse, then falls back to
  top-level object salvage when the array syntax is malformed
- localized corruption produces typed degraded warnings instead of a generic
  opaque parser error
- append/rebuild paths continue using strict parsing so malformed inboxes are
  not silently rewritten during normal command execution
