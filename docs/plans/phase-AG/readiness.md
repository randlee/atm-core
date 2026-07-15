# Phase AG Readiness Record

## Purpose

Final readiness gate for Windows/macOS cross-host communication on the `1.3.1`
release line.

`Phase AG` is not ready until:

- the accepted AG.1/AG.2/AG.3 historical work is reconciled into the current
  phase sequence
- the missing interface/allowlist control-plane sprints are landed
- the clean-room lane then proves the cross-host interface matrix on release binaries
- any discovered defects are either fixed and revalidated or explicitly block
  release
- the copied-state lane is executed only after clean-room success
- transport-security disposition is recorded against the documented TCP/TLS
  requirement, and any plain-TCP mismatch remains a named finding instead of an
  implicit waiver

## Per-Sprint Closure Results

| Sprint | Closure Result | Candidate Commit | Notes |
| --- | --- | --- | --- |
| `AG.1` | `PASS` | `multiple` | setup contract and first live channel attempts were completed; the sprint correctly exposed the need for later product-control work |
| `AG.2` | `RECLASSIFIED` | `multiple` | initial live validation attempts ran, but the sprint could not close because the required cross-host control plane did not exist yet; the first real reverse-direction send also exposed open product bug `AG-FIND-005` on the live send path |
| `AG.3` | `PASS` | `85a7d4df` | loopback self-test surface landed and is retained as a supported local diagnostic mode; it is not remote host-pair proof |
| `AG.4` | `PENDING` | `TBD` | durable interface-selection/bind control plane not yet implemented |
| `AG.5` | `PENDING` | `TBD` | durable inbound host-allowlist control plane not yet implemented |
| `AG.6` | `PENDING` | `TBD` | `atm doctor` visibility for the cross-host control plane not yet executed |
| `AG.7` | `PENDING` | `TBD` | renewed live host-pair validation on the real product surface not yet executed |
| `AG.8` | `PENDING` | `TBD` | transport-security / encryption hardening not yet executed |
| `AG.9` | `PENDING` | `TBD` | copied-state revalidation and final release verdict not yet executed |
| `AG.10` | `PENDING` | `TBD` | secured cross-host transport implementation not yet executed |

Allowed closure-result values:

- `PENDING`
- `PASS`
- `FAIL`
- `BLOCKED`
- `RECLASSIFIED`

## Required Gate Criteria

`Phase AG` must remain not-ready until all of the following are true:

- the clean-room lane is executed first and passes or fails with named findings
- the missing control-plane product surface from `AG-FIND-004` is implemented
- `AG.6` does not begin until `AG.4` and `AG.5` land
- `AG.7` does not begin until `AG.6` lands
- `AG.9` does not begin until the required clean-room rows are green enough to
  justify copied-state revalidation
- `AG.9` may not claim transport-security closure until `AG.10` is `PASS`; if
  `AG.10` is deferred, the verdict must explicitly state cross-host is
  functionally usable but not transport-secure
- every failed row is linked to a finding in
  `cross-host-findings-ledger.md`
- the final verdict states whether `1.3.1` cross-host communication is
  release-usable
- any final release-usable verdict must explicitly state that it does not cover
  TLS / transport-security guarantees while the transport requirement remains an
  open `PRODUCT-BUG` or unresolved requirement drift

## Initial Verdict

- readiness status: `NOT READY`
- final accepted candidate line: `TBD`
- gate status: `BLOCKED`
- notes: `Early AG work proved the original validation-only framing was wrong.
  Cross-host closure is blocked on AG-FIND-004: durable interface selection,
  durable allowlist enforcement, CLI management for both, and doctor support.
  Live execution also surfaced AG-FIND-003 on the host-singleton clean-room
  contract and AG-FIND-005 on the send path, where cross-host sends still
  short-circuit into the local daemon sink instead of invoking peer
  transport.`
