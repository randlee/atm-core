---
title: Phase AG Plan
status: planned
branch: plan/phase-ag-multihost-advertise-allowlist
worktree: ../atm-core-worktrees/plan/phase-ag-multihost-advertise-allowlist
---

# Phase AG Plan

## Goal

Complete the missing Windows/macOS cross-host product surface and then validate
it on real binaries with retained evidence.

Early AG execution proved the original assumption was wrong: ATM did not yet
have a complete operator-manageable cross-host control plane. The missing
pieces are not optional hardening; they are prerequisite product work:

- CLI-managed interface/bind configuration for the daemon
- CLI-managed inbound allowed-host configuration
- SQLite tables storing both
- `atm doctor` support surfacing both
- loopback self-test retained as a supported local diagnostic mode

Phase `AG` therefore becomes:

- historical early validation sprints (`AG.1` / `AG.2`)
- accepted loopback diagnostic work (`AG.3`)
- prerequisite control-plane product work (`AG.4` / `AG.5`)
- doctor-surface closure on the new product controls (`AG.6`)
- renewed live cross-host validation once the product is actually operable
  (`AG.7`)
- late transport-security planning/reconciliation (`AG.8`)
- secured transport implementation (`AG.10`)
- copied-state revalidation and final release verdict (`AG.9`)

## Historical Input And Namespace Rule

This phase supersedes the older `Phase AB` cross-host smoke planning line as
the active namespace for current work.

`Phase AB` remains useful input because it already captured:

- the high-level Windows/macOS host-pair objective
- the disposable clean-room lane before copied-state revalidation
- the basic smoke matrix for send/read/ack, degraded notification, and retry

`Phase AB` is historical planning input only, not execution evidence. Its own
readiness record remains `NOT READY` with all sprint closure rows `PENDING`, so
AG may reuse AB's planning structure but must not rely on AB as proof that any
cross-host path was previously validated.

`Phase AG` changes the framing:

- active namespace is `AG`, not `AB`
- phase goal is broader than smoke-only proof; it is missing-surface
  completion plus interface validation and release readiness
- closure expectation is no longer "no code unless testing proves a bug";
  prerequisite product work is now part of the phase by design
- setup/runbook detail must be operational enough that macOS and Windows agents
  can execute without guessing how to bring the channel live

## Release Framing

Phase `AG` is the next release-directed phase after the accepted same-host
release-readiness line.

Current release baseline on entry:

- Phase AF's accepted same-host reliability-recovery line is merged to
  `develop` at `98a4e66c`
- the authoritative Phase AF dependency artifacts in this branch must also
  report that merged/closed state before AG may rely on them as already
  validated input

Entry-gate prerequisites:

- same-host daemon behavior is already validated on the exact merged baseline
  used for cross-host execution
- Windows same-host release-binary command health is already validated on that
  exact merged baseline before AG.1 starts

## Branch Framing

Phase `AG` now has two distinct lines:

- historical early execution on `feature/cross-host-communication`
- corrective replanning on `plan/phase-ag-multihost-advertise-allowlist`

The historical line remains important evidence, but it no longer defines the
forward phase sequence by itself. The current branch is the corrective source
of truth because it reconciles what was actually learned:

- the original validation-only framing was insufficient
- environment-variable-driven peer selection is not the desired product model
- interface selection, allowlist enforcement, and doctor visibility must become
  first-class product surfaces before real cross-host closure can be trusted

Bounded exception already exercised in this phase:

- `AG-FIND-004` was discovered during AG.1's live viability attempt as a real
  product defect in daemon-to-daemon bring-up, not a docs ambiguity
- per Phase AG's own "fix real bugs in code" policy, that finding was fixed
  in-sprint on `feature/pAG-s1-macos-execution` and carried by PR #551
  instead of being deferred to a separate branch after the defect was already
  isolated and understood
- this exception is named, one-off, and limited to the `AG-FIND-004`
  peer-listener viability fix; it does not authorize unrelated product-code
  work to accumulate silently on AG planning branches

Release claim this phase must validate:

- cross-host communication is functionally operable through real product
  surfaces on release binaries across Windows and macOS

Release claim this phase must not make without evidence:

- that the product is cross-host ready just because same-host smoke passed
- that ad hoc env-variable wiring is an acceptable final operator surface
- that cross-host transport is secure or TLS-backed on `1.3.1`

## Scope

Phase `AG` may:

- add or refine planning docs, operator runbooks, checklists, and findings
  records
- add prerequisite product-planning work for the missing cross-host control
  plane
- execute release-binary validation on macOS and Windows
- use disposable clean-room state first
- use copied-state validation only after the clean-room lane is green
- record defects and open narrowly scoped follow-up fix work if validation
  fails
- make minimal planning-document updates during execution when evidence shows a
  setup or evidence contract was underspecified

Phase `AG` must not:

- treat missing interface/allowlist product surfaces as mere test setup
- use live host state as the first validation lane
- conflate notification degradation with durable cross-host delivery failure
- hide setup ambiguity behind hand-wavy references to existing docs

## Ownership Model

Phase `AG` separates execution from verification explicitly:

- `team-lead`
  - owns dispatch, sequencing, branch routing, and final merge authorization
- `arch-ctm`
  - owns plan/package edits, execution-side document updates, and first-pass
    finding triage
- `quality-mgr`
  - owns independent review and PASS/FAIL/BLOCKED verdicts
- `windows-operator`
  - owns Windows-side command execution and evidence capture
- `macos-operator`
  - owns macOS-side command execution and evidence capture

The findings ledger `owner` field must use one of these exact values:

- `team-lead`
- `arch-ctm`
- `quality-mgr`
- `windows-operator`
- `macos-operator`
- `shared`

## Working Assumptions

The phase proceeds under these assumptions:

- AG.1 / AG.2 already proved the original product surface was insufficient
- the remaining blocker is not another proof-of-concept transport hack; it is
  completion of the durable control-plane surface
- setup ambiguity is itself a real phase finding if it blocks reproducible
  execution
- the highest-value next objective is to land interface/allowlist/doctor
  surfaces so the daemon-to-daemon channel can be exercised through real
  product controls rather than env hacks

## Validation Lanes

### Lane A — Disposable Clean-Room Cross-Host Validation

Purpose:

- prove the full Windows/macOS cross-host interface set on synthetic state only
- keep failures attributable to setup/bootstrap/transport/runtime boundaries

Required shape:

- one disposable `ATM_HOME` per host
- one disposable `ATM_CONFIG_HOME` per host
- one disposable `ATM_LOG_DIR` per host
- durable daemon-owned interface configuration instead of env-driven peer/port
  control as the target product surface
- one release `atm-daemon` process per host
- one release CLI surface per host
- no reads or writes against live `~/.atm` or `~/.claude`
- pre-send configuration validation so peer-transport misconfiguration fails
  fast rather than appearing as a later write-path mystery
- transport-security disposition is captured explicitly against the documented
  TCP/TLS requirement, but TLS implementation itself is not expanded inside AG
  scope; the phase records the gap as a named finding instead

### Lane B — Disposable Copied-State Revalidation

Purpose:

- prove cross-host interfaces still hold on a disposable copy of realistic ATM
  and Claude state

Entry condition:

- Lane A is already green end to end

Required shape:

- disposable copies of host state only
- no writes against live host-scoped state
- every repair/setup deviation from Lane A recorded explicitly

## Operator-Owned Setup Contract

The phase deliverable must be operational enough that one macOS operator and
one Windows operator can bring the channel live with no hidden local knowledge.

That contract includes:

- release binary path to use on each host
- disposable directory layout to create on each host
- exact environment variables to export/set on each host
- exact daemon start command on each host
- exact health commands on each host
- exact CLI-managed interface/allowlist state each host should use
- exact message addressing form to use in cross-host rows
- exact evidence to save for pass/fail classification
- exact first-line recovery steps when a row fails

These details are recorded in:

- `docs/plans/phase-AG/cross-host-setup-runbook.md`

## Required Interface Matrix

`Phase AG` must validate all of the following on release binaries:

- daemon bring-up on macOS clean-room state
- daemon bring-up on Windows clean-room state
- peer transport channel bring-up between hosts
- Windows -> macOS durable send
- macOS -> Windows durable send
- receiver-side read on both directions
- receiver-side ack for a `--requires-ack` message
- sender-side visibility of the ack/reply-state mutation
- degraded notification after durable cross-host delivery
- retry-visible interruption and recovery
- copied-state rerun of the approved subset only after clean-room success
- transport-security requirement disposition against
  `REQ-CORE-TRANSPORT-001/003/005`; if the implementation remains plain TCP,
  AG must carry a named `PRODUCT-BUG` or requirement-drift finding and any
  release-usable statement must explicitly exclude transport-security coverage

## Evidence Contract

Every validation row must capture:

- host pair and sender/receiver direction
- exact disposable env/config inputs
- exact daemon start command and resulting PID on each host
- exact CLI command transcript on both hosts
- sender JSON result
- receiver JSON result for read/ack rows
- `atm doctor --json` when relevant
- retained daemon log snapshot on both hosts when daemon-backed behavior is
  exercised
- finding ID linkage if the row fails
- whether the failure was:
  - setup contract gap
  - operator/environment mistake
  - product defect
  - blocked external dependency

## Failure Classification

The sole authoritative `classification` enum for Phase AG findings is:

- `SETUP-GAP`
- `ENV-MISTAKE`
- `PRODUCT-BUG`
- `EXTERNAL-BLOCKER`

All other Phase AG docs must reference this enum only and must not claim
separate or joint authority over it.

Rows may end in one of two useful states:

- `PASS`
  - the interface behaved as designed and the evidence is retained
- `FAIL`
  - the row produced a named finding with exact reproduction, artifacts,
    suspected surface, and required fix scope

The phase should avoid ambiguous "sort of worked" closure language.

Examples:

- invalid transitional env wiring in the legacy runbook is a `SETUP-GAP`
- daemon cannot establish peer transport with correct operator input is a
  `PRODUCT-BUG`
- host firewall prompt not handled/documented is an `EXTERNAL-BLOCKER` until
  shown otherwise

## Sprint Sequence

### AG.1 Cross-Host Setup Contract And Channel Bring-Up

Primary objective:

- get one macOS daemon and one Windows daemon into a state where a real
  cross-host channel can be attempted without guesswork

Outputs:

- frozen clean-room setup runbook
- exact setup contract for both hosts as early-phase historical evidence
- same-host release-binary health proof on both hosts via `AG-VAL-001` and
  `AG-VAL-002`
- transport-security requirement disposition via `AG-VAL-011`
- one first-live-channel viability attempt whose outcome can open a finding,
  but which does not formally close checklist rows owned by `AG.2`

Execution owner:

- `arch-ctm` with Windows/macOS operators capturing host-side evidence
- `windows-operator` and `macos-operator` execute the concrete host-local
  commands and produce the retained artifacts consumed by AG.1

Verification owner:

- `quality-mgr`

### AG.2 Core Cross-Host Interface Validation

Primary objective:

- preserve the initial live cross-host validation attempts and the evidence
  proving the original product surface was incomplete

Outputs:

- initial ownership/evidence for checklist rows `AG-VAL-003` through
  `AG-VAL-007`
- named finding handoff into `AG-FIND-004`

Entry gate:

- `AG.1` must already have:
  - recorded `AG-VAL-001` and `AG-VAL-002`
  - resolved the first live-channel viability attempt to either:
    - a working channel that allows AG.2 to proceed, or
    - a named blocking finding recorded in `cross-host-findings-ledger.md`

Execution owner:

- `arch-ctm` with Windows/macOS operators capturing host-side evidence
- `windows-operator` and `macos-operator` execute the concrete host-local
  commands and produce the retained artifacts consumed by AG.2

Verification owner:

- `quality-mgr`

### AG.3 Loopback Self-Test Surface

Primary objective:

- preserve and authorize the loopback self-test surface as a supported local
  diagnostic feature

Outputs:

- loopback addressing and self-dial behavior are explicitly planned and
  documented
- local diagnostic value is preserved without pretending it closes real
  host-pair validation

Entry gate:

- `AG.2` must already have:
  - resolved `AG-VAL-003` through `AG-VAL-007`
  - recorded each AG.2 core interface row as either:
    - a passing validation row that allows AG.3 to proceed, or
    - a named blocking finding recorded in
      `cross-host-findings-ledger.md`

Execution owner:

- `arch-ctm` with Windows/macOS operators capturing host-side evidence
- `windows-operator` and `macos-operator` execute the concrete host-local
  commands and produce the retained artifacts consumed by AG.3

Verification owner:

- `quality-mgr`

### AG.4 Durable Interface Configuration

Primary objective:

- add the durable SQLite-backed interface-selection/bind surface needed for
  real cross-host operation

Outputs:

- schema for daemon-managed cross-host interface rows
- CLI commands to create, inspect, enable/disable, and delete interface rows
- explicit staleness/refresh model for roaming hosts
- removal of env-variable-driven peer/port control as the target operator
  model

Execution owner:

- `arch-ctm`
- `windows-operator` and `macos-operator` support AG.4 only with
  config-visibility smoke on their hosts when needed:
  - confirm configured interface rows are visible
  - confirm bound/non-bound state is observable
  - they do not own real host-pair send/read/ack rows in this sprint

Verification owner:

- `quality-mgr`

### AG.5 Durable Host Allowlist Enforcement

Primary objective:

- add the durable SQLite-backed deny-by-default host authorization surface

Outputs:

- schema for exact-host allowlist rows
- CLI commands to add, disable, remove, and inspect allowed hosts
- daemon-side enforcement before mailbox mutation
- explicit reconciliation with `AG-FIND-004` and the loopback-bypass design
  finding

Execution owner:

- `arch-ctm`

Verification owner:

- `quality-mgr`

### AG.6 Doctor Visibility For The Cross-Host Control Plane

Primary objective:

- project the new control-plane state through `atm doctor`

Outputs:

- `atm doctor` interface/allowlist output contract
- requirements/architecture wording aligned to that doctor-visible state

Entry gate:

- `AG.4` and `AG.5` are complete enough that real host-pair execution is using
  the intended product surfaces rather than env hacks

Execution owner:

- `arch-ctm`

Verification owner:

- `quality-mgr`

### AG.7 Live Cross-Host Revalidation

Primary objective:

- rerun the real host-pair matrix on the now-complete product surface

Outputs:

- renewed ownership of real host-pair validation rows
- explicit ownership of unauthorized-host rejection row `AG-VAL-003A`
- explicit LAN-first execution preference where available (including Mac Studio)
- integration findings separated cleanly from control-plane product findings

Entry gate:

- AG.4 and AG.5 are complete
- AG.6 doctor visibility is complete

### AG.8 Transport Security And Release-Language Reconciliation

Primary objective:

- reconcile the documented transport-security contract with the actual current
  plain-TCP implementation line without claiming secure transport is already
  shipped

Outputs:

- requirements/architecture/readiness reconciliation for transport security
- explicit statement of what earlier AG closure does and does not authorize
- concrete AG.10 implementation scope for certificate, trust, handshake,
  doctor, and smoke/test work
- explicit record that AG.7 live host-pair validation is still pending

Entry gate:

- the AG.4/AG.5/AG.6/AG.7 functional code paths and local harness exist on the
  current line
- AG.7 live hardware rows may still be pending, but AG.8 must say so plainly

### AG.10 Secured Cross-Host Transport Implementation

Primary objective:

- implement the secured daemon-to-daemon transport defined concretely by AG.8

Outputs:

- TLS-backed daemon-to-daemon transport
- on-demand local self-signed daemon certificate generation and durable storage
- explicit peer trust approval / persistence path
- explicit insecure-mode support with doctor/runtime visibility
- secure loopback validation support
- secure LAN/routed validation support

Entry gate:

- AG.8 planning/reconciliation work is complete

### AG.9 Copied-State Revalidation And Release Verdict

Primary objective:

- rerun the approved subset on disposable copied state and then record the
  actual release verdict

Outputs:

- copied-state revalidation evidence
- final findings ledger
- readiness record
- explicit statement of whether cross-host is:
  - functionally release-usable
  - blocked
  - functionally usable but not transport-secure

Entry gate:

- AG.7 live host-pair validation is complete enough to justify copied-state
  rerun
- AG.8 planning/reconciliation work is complete
- AG.10 status is known before the final release verdict is issued:
  - if `AG.10` is `PASS`, the verdict may include transport-security closure
  - if `AG.10` is deferred, blocked, or out-of-scope, the verdict must state
    cross-host is functionally usable but not transport-secure
  - if `AG-FIND-005` remains open, the verdict must instead state cross-host is
    functionally blocked and copied-state revalidation must remain blocked

Execution owner:

- `arch-ctm`

Verification owner:

- `quality-mgr`

## Exit Criteria

Phase `AG` is complete only when all of the following are true:

- the accepted historical AG.1 / AG.2 / AG.3 work is reconciled into the final
  phase sequence
- the AG.4 / AG.5 control-plane product work is complete
- the AG.6 doctor surface is complete
- the clean-room cross-host lane is fully executed with evidence on that real
  product surface
- if secure transport is part of the release claim, AG.10 is complete
- the copied-state lane is either green or explicitly blocked by a named
  product defect outside operator/setup ambiguity
- every failed row has a named finding and required next action
- the readiness record states whether `1.3.1` cross-host communication is
  release-usable
- any functional release-usable statement is explicit about whether it excludes
  transport security
- if ordinary remote send is not routed into peer transport, the release verdict
  must classify cross-host as functionally blocked rather than merely insecure
