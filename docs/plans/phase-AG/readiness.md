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
- the copied-state lane is executed only after clean-room success and, when
  `AG-FIND-005` remains in scope, after the corrective same-host/second-host
  ladder is green enough to justify it
- transport-security disposition is recorded against the documented TCP/TLS
  requirement, and any plain-TCP mismatch remains a named finding instead of an
  implicit waiver

## Per-Sprint Closure Results

| Sprint | Closure Result | Candidate Commit | Notes |
| --- | --- | --- | --- |
| `AG.1` | `PASS` | `multiple` | setup contract and first live channel attempts were completed; the sprint correctly exposed the need for later product-control work |
| `AG.2` | `RECLASSIFIED` | `multiple` | initial live validation attempts ran, but the sprint could not close because the required cross-host control plane did not exist yet |
| `AG.3` | `PASS` | `85a7d4df` | loopback self-test surface landed and is retained as a supported local diagnostic mode; it is not remote host-pair proof |
| `AG.4` | `PENDING` | `TBD` | durable interface-selection/bind control plane not yet implemented |
| `AG.5` | `PENDING` | `TBD` | durable inbound host-allowlist control plane not yet implemented |
| `AG.6` | `PENDING` | `TBD` | `atm doctor` visibility for the cross-host control plane not yet executed |
| `AG.7` | `PENDING` | `TBD` | renewed live host-pair validation on the real product surface not yet executed |
| `AG.8` | `PENDING` | `TBD` | transport-security / encryption hardening not yet executed |
| `AG.9` | `PENDING` | `TBD` | reviewed earlier final-verdict sprint for the pre-corrective line; not the authoritative final verdict while `AG-FIND-005` remains open |
| `AG.10` | `PENDING` | `TBD` | secured cross-host transport implementation not yet executed; any transport-security claim still depends on this sprint |
| `AG.11` | `PENDING` | `TBD` | corrective remote-target contract and dispatch routing not yet executed |
| `AG.12` | `PENDING` | `TBD` | localhost full-function same-host remote-target proof not yet executed |
| `AG.13` | `PENDING` | `TBD` | self-IP full-function same-host proof not yet executed |
| `AG.14` | `PENDING` | `TBD` | automated integration coverage for the corrective path not yet executed |
| `AG.15` | `PENDING` | `TBD` | other-Mac cross-host smoke not yet executed across the full corrective matrix |
| `AG.16` | `PENDING` | `TBD` | Windows/macOS cross-host smoke not yet executed across the full corrective matrix |
| `AG.17` | `PENDING` | `TBD` | authoritative copied-state revalidation and final verdict for the corrective line not yet executed |
| `AG.18` | `PENDING` | `TBD` | envelope/handler collapse cleanup ladder not yet executed on the corrective unification line |
| `AG.19` | `PENDING` | `TBD` | remote-ack path collapse not yet executed on the corrective unification line |
| `AG.20` | `PENDING` | `TBD` | transport-policy deletion not yet executed on the corrective unification line |
| `AG.21` | `PENDING` | `TBD` | dispatch/inbound single-route cleanup not yet executed on the corrective unification line |
| `AG.22` | `PENDING` | `TBD` | endpoint-resolution boundary relocation not yet executed on the corrective unification line |
| `AG.23` | `PENDING` | `TBD` | synthetic deferred-receipt deletion not yet executed on the corrective unification line |
| `AG.24` | `PENDING` | `TBD` | request-shape preservation cleanup not yet executed on the corrective unification line |
| `AG.25` | `PENDING` | `TBD` | final live-proof closeout for the unification line not yet executed |

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
- the corrective remote-target routing gap from `AG-FIND-005` is implemented
- `AG.6` does not begin until `AG.4` and `AG.5` land
- `AG.7` does not begin until `AG.6` lands
- `AG.9` does not begin until the required clean-room rows are green enough to
  justify copied-state revalidation
- `AG.12` does not begin until `AG.11` lands
- `AG.13` does not begin until `AG.12` lands
- `AG.14` does not begin until `AG.11` is functional and `AG.12` / `AG.13`
  define the exact same-host behavior to lock in
- `AG.15` does not begin until `AG.13` and `AG.14` land
- `AG.16` does not begin until `AG.15` lands
- `AG.17` does not begin until `AG.16` is green enough to justify copied-state
  revalidation
- `AG.18` begins the post-AG.15 ruthless-boundary unification ladder
- `AG.19` does not begin until `AG.18` lands
- `AG.20` does not begin until `AG.19` lands
- `AG.21` does not begin until `AG.20` lands
- `AG.22` does not begin until `AG.21` lands
- `AG.23` does not begin until `AG.22` lands
- `AG.24` does not begin until `AG.23` lands
- `AG.25` does not begin until `AG.18` through `AG.24` all land
- `AG.9` may not claim transport-security closure until `AG.10` is `PASS`; if
  `AG.10` is deferred, the verdict must explicitly state cross-host is
  functionally usable but not transport-secure
- `AG.17` inherits the same transport-security rule from `AG.10`
- `AG.17` remains the historical corrective-line final verdict for AG.11
  through AG.16; `AG.9` remains historical reviewed verdict scope only
- `AG.25` is the authoritative live-proof closeout for the AG.18 through
  AG.24 unification line
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
  Corrective closeout is also blocked on AG-FIND-005: ordinary atm send still
  needs a first-class remote-target contract and dispatch branch before the
  localhost/self-IP/second-host validation ladder can close.`
