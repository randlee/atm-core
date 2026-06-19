# Phase AB Smoke Staging Pack

## Purpose

This staging pack contains pre-plan investigation documents for Phase AB cross-host smoke
testing. It is the output of a repository investigation performed on branch
`feature/phase-AB-smoke-staging`, cut from `origin/develop` at commit `c451afe4`
(post-Phase-AC integration). No execution, no builds, and no system mutations were
performed to produce these documents.

The pack surfaces transport implementation gaps, staleness risks introduced by Phase AC,
and concrete prerequisites for both hosts. Its intended audience is the architect and
team-lead preparing the next execution-planning session.

## Base Context

- Branch: `feature/phase-AB-smoke-staging`
- Base commit: `c451afe4` (post-Phase-AC, includes all AC crate additions and fixes)
- Investigation scope: pre-plan only — no code changes, no CI runs, no deployments

## Document Index (recommended read order)

1. **`executability-gap.md`** — Lead document. States the headline finding: AB.2–AB.4
   cannot execute on current develop due to a missing receiver-side TCP listener.
   Read this first.

2. **`transport-findings.md`** — Technical deep-dive into the cross-host transport
   implementation state. Cites exact file paths and line numbers in `peer_transport.rs`
   and `composition.rs`, and quotes the architecture specification sections that define
   the contract ahead of implementation.

3. **`develop-deltas-since-AB-plan.md`** — Records what changed on develop between the
   AB plan PR (#389, merged 2026-06-04) and the Phase AC integration PR (#420, merged
   2026-06-09). Identifies crates added, crates removed, CLI file changes, and the two
   Windows SQLite fix commits.

4. **`ac-freshness-flags.md`** — Row-by-row impact table for the AB smoke checklist.
   Flags which rows carry elevated re-verification risk because of Phase AC changes.

5. **`windows-host-prereqs.md`** — Prerequisites and disposable environment setup for
   the Windows host (`2023-001` @ `192.168.1.146`) covering AB.1 same-host clean-room
   execution and forward-looking notes for cross-host lanes.

6. **`mac-host-prereqs.md`** — Symmetric prerequisites for the Mac host
   (`Erik_RVS_MacBookPro` @ `192.168.1.178`).

7. **`ab1-execution-readiness.md`** — Concrete preparation checklist and exact command
   sequences for the next session's AB.1 execution on both hosts, including evidence
   capture expectations.

8. **`ac-freshness-flags.md`** — (See item 4 above; listed separately for completeness.)

## Status

Status: pre-plan; ready for execution-planning session on Mac primary.
