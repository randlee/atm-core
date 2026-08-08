# AL.15 — Direct Cross-Host Evidence Closeout

**branch:** `feature/al-15-smoke`
**worktree:** `../atm-core-worktrees/feature/al-15-smoke`
**recommended_agent:** coordinator with review access to the AL.13 M5 and
AL.14 cwin PRs.
**must_follow:** accepted AL.13 and AL.14 evidence PRs, both pinned to the
same runtime commit/version.
**unblocks:** a truthful statement of current direct M5↔cwin smoke status.
It does not unblock replay, recovery, or Phase AM deletion by itself.
**parallel_safe:** none; this is the evidence-consumption gate.

## Purpose

Review the two host-owned evidence bundles as one direct cross-host result.
AL.15 prevents a local-only success, one-way send, generic CLI success line,
or manually assembled HTML page from being misrepresented as physical
cross-host proof. It makes the retained `just smoke` artifacts and the public
CLI observations the sole evidence source.

## Deliverables

1. Verify both PRs identify the same tested commit, ATM version, and direct
   endpoint selection. Verify that the testing ref includes `faf0c24b` (or a
   reviewed equivalent merge-forward) so the retained smoke artifacts use the
   master-report registration contract. A mixed build is a failed/blocked
   proof, not an acceptable compatibility result.
2. For each host, inspect its generated master reports index and navigate to
   all local and cross-host run pages. Confirm the expected self-contained
   layout and metadata:

   ```text
   site/reports/index.html
     -> smoke/<platform>/<host>/<run>/index.html
       -> per-host XHTML evidence panels
   ```

3. Confirm the exact ordered evidence matrix is complete:

   | Row | M5-originating proof | cwin-originating proof |
   |---|---|---|
   | matched daemon/CLI doctor | required | required |
   | localhost | required | required |
   | local-IP | required | required |
   | peer preflight | required | required |
   | direct send/read exact ID and body, both directions | required | required |
   | requires-ack/ack reply linkage, both directions | required | required |

4. Confirm each direct row used the normal public CLI path and the configured
   peer endpoint. Reject evidence that substitutes curl, raw sockets, a
   changed message body, direct database inspection, a second daemon, TLS
   setup, or a background recovery mechanism.
5. Record one of two precise outcomes in the AL.15 PR:

   - **PASS:** every matrix row passes from both origins and retained reports
  are browsable from the master index.
   - **BLOCKED/FAIL:** identify the first failing command, the host of origin,
     exact runner output, affected matrix rows, report link, and the smallest
     next owner. Do not claim partial cross-host closure.

6. If a defect is found, leave the evidence untouched, open a focused fix on
   the affected host's home sprint branch (or the smallest appropriate branch),
   and rerun only from the earliest invalidated stage after review. Escalate to
   Rand if the proposed fix changes endpoint selection, canonical request
   types, storage semantics, notification semantics, or introduces retry/
   replay.

## Acceptance criteria

- The two host PRs provide current, same-version, self-contained evidence for
  every matrix row.
- Evidence proves normal direct send/receive and acknowledgement semantics
  without a peer-specific application protocol or altered payload.
- Each result is browsable through `site/reports/index.html`; platform and
  host identity are visible in the retained path and report metadata.
- The closeout report is a binary PASS or a specifically bounded BLOCKED/FAIL;
  no local-only, one-way, or TLS/curl diagnostic result is accepted as direct
  cross-host success.
- No replay/recovery work, legacy daemon work, TLS work, or transport-schema
  changes enter AL.15.

## Required validation

- `just test` and CI-green status for every code-bearing fix cited by the two
  evidence PRs.
- Manual inspection of the report-index links and XHTML panels for each run.
- Independent review by an operator who did not author the evidence under
  review.

## Non-closure

AL.15 does not add a timer, heartbeat, resend cursor, `message[]` delivery,
outage replay, TLS certification work, or legacy daemon compatibility work.
Those are separate future decisions after direct cross-host behavior is proven.
