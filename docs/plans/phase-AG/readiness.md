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
| `AG.4` | `PENDING` | `TBD` | interface-control implementation is on the current AG development line, but sprint closure is still tracked through the held back-to-back queue rather than a separate accepted readiness update |
| `AG.5` | `PENDING` | `TBD` | durable inbound allowlist implementation is on the current AG development line, but sprint closure is still tracked through the held back-to-back queue rather than a separate accepted readiness update |
| `AG.6` | `PASS` | `48c85b8d` | `atm doctor` now projects interface/bind state, allowlist state, staleness, bind failures, and legacy-fallback visibility for the shipped AG.4/AG.5 model |
| `AG.7` | `PENDING` | `34846433` | local daemon-to-daemon harness and functional code-path closure exist on the current line; real Windows/macOS or Windows/Mac-Studio host-pair execution remains open until the live hardware matrix is rerun |
| `AG.8` | `PENDING` | `TBD` | planning/reconciliation sprint: documents the current plain-TCP security posture, records that AG.7 live evidence is still pending, and defines AG.10's secure transport direction |
| `AG.9` | `PASS` | `working tree` | copied-state revalidation is explicitly blocked because Lane A is not functionally green; final release verdict is now recorded honestly as functionally blocked by `AG-FIND-005` rather than merely transport-insecure |
| `AG.10` | `PASS` | `16c5ba03` | TLS-backed secure mode, trusted-peer approval, doctor visibility, secure loopback proof, and local untrusted-peer rejection are implemented on the AG.10 line; secure LAN/VPN reruns remain open before transport-security closure can be claimed |

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
- `AG.7` live host-pair evidence does not begin until `AG.6` lands
- `AG.8` may run before AG.7 live rows are green, but it must state that the
  real host-pair evidence is still pending
- `AG.9` does not begin until the required clean-room rows are green enough to
  justify copied-state revalidation
- `AG.9` may not claim transport-security closure until `AG.10` is `PASS`
- if `AG-FIND-005` remains open, the verdict must explicitly state cross-host
  is functionally blocked, regardless of transport-security status
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
- notes: `Phase AG is blocked by AG-FIND-005. Ordinary cross-host send is not
  wired into peer transport at all, so a real host-to-host send can report
  success while never leaving the sender host. The AG.4-AG.10 control-plane and
  AG.10 secure-transport work are real and locally validated, but transport
  security is moot until the functional send-routing gap is fixed. Copied-state
  revalidation is therefore not approved to run, because Lane A is not
  functionally green.`

## AG.9 Copied-State Revalidation Result

- status: `BLOCKED`
- approved subset executed: `none`
- reason: `AG-FIND-005 leaves Lane A functionally red, so AG.9 cannot honestly
  promote any copied-state rerun as meaningful release evidence`
- consequence: `AG-VAL-010` remains blocked pending a dedicated fix sprint for
  remote-send routing followed by fresh clean-room host-pair reruns

## Final Release Verdict

- cross-host communication: `FUNCTIONALLY BLOCKED`
- transport security: `implemented for the daemon surfaces that do work
  (secure loopback, trusted-peer rejection, doctor visibility), but not
  release-meaningful until AG-FIND-005 is fixed`
- release usability for `1.3.1` cross-host communication: `NOT RELEASE USABLE`

Plain-language verdict:

> Normal cross-host `atm send` is currently broken. It can report success while
> keeping delivery on the sender host instead of routing through the remote
> daemon. Until that routing defect is fixed, cross-host communication is
> blocked, not merely insecure.

## Operator Repair / Setup Notes

- do not attempt copied-state reruns while `AG-FIND-005` is open; they do not
  add trustworthy release evidence
- do not interpret a successful `atm send --json` as proof of daemon-to-daemon
  delivery unless receiver-side inbox evidence and daemon logs confirm remote
  mutation

## Follow-On Work Required

- schedule a dedicated fix sprint for `AG-FIND-005`; no AG.4-AG.10 sprint owns
  that code change
- the fix must use one explicit routing branch only:
  - local recipient -> current local mailbox path
  - remote recipient -> remote-delivery trait path
- remote-delivery routing must be isolated behind a trait or narrow trait set;
  do not spread peer-routing decisions across general daemon/runtime code
- after the fix lands, rerun Lane A clean-room host-pair rows before any
  copied-state revalidation or release verdict is reconsidered
