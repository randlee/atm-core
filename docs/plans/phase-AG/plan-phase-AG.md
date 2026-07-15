---
title: Phase AG Plan
status: planned
branch: feature/cross-host-communication
worktree: ../atm-core-worktrees/feature/cross-host-communication
---

# Phase AG Plan

## Goal

Prove that ATM's existing Windows/macOS cross-host interfaces are working and
release-usable on `1.3.1` real binaries, with no code changes unless the
validation matrix exposes a real product defect.

Phase `AG` is not a transport redesign phase. It is a validation and
release-readiness phase that should, in the ideal case, close with:

- no product code changes
- one working macOS daemon
- one working Windows daemon
- a live daemon-to-daemon channel between hosts
- passing cross-host send/read/ack coverage
- retained evidence that release binaries behave as claimed

Because at least one oversight or bug is expected, the phase is structured to
turn failures into named findings with exact reproduction and narrowly scoped
fix follow-ups rather than speculative up-front implementation.

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
- phase goal is broader than smoke-only proof; it is interface validation and
  release readiness
- closure expectation is "no code unless testing proves a bug"
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

## Branch Policy Exception

Phase `AG` is a single-branch docs/evidence-only validation phase. It does not
own a separate implementation integration line because:

- the current phase goal is to validate existing released interfaces first,
  not stage speculative code work
- the current branch content is planning, runbook, checklist, findings, and
  readiness material only
- any future product-code fix opened by a concrete AG finding can still route
  through its own normal implementation branch/integration sequence

Accordingly, this planning package is intentionally authored on
`feature/cross-host-communication` and PR #542 targets `develop` directly. If
Phase AG later needs product-code changes, that follow-up work must declare its
own branch/integration path explicitly rather than silently inheriting this
docs/evidence-only exception.

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

- cross-host interfaces are present in the product and behave correctly on
  release binaries across Windows and macOS

Release claim this phase must not make without evidence:

- that the product is cross-host ready just because same-host smoke passed
- that cross-host transport is secure or TLS-backed on `1.3.1`

## Scope

Phase `AG` may:

- add or refine planning docs, operator runbooks, checklists, and findings
  records
- execute release-binary validation on macOS and Windows
- use disposable clean-room state first
- use copied-state validation only after the clean-room lane is green
- record defects and open narrowly scoped follow-up fix work if validation
  fails
- make minimal planning-document updates during execution when evidence shows a
  setup or evidence contract was underspecified

Phase `AG` must not:

- redesign the peer transport contract
- begin with speculative code changes before a real failing row exists
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

- in the ideal case, existing code is sufficient and no product changes are
  needed
- in the likely case, at least one oversight/bug will be exposed by validation
- setup ambiguity is itself a real phase finding if it blocks reproducible
  execution
- the highest-value early objective is to get the daemon-to-daemon channel live
  between one Windows host and one macOS host; once that works, the rest of the
  matrix should move quickly

## Validation Lanes

### Lane A — Disposable Clean-Room Cross-Host Validation

Purpose:

- prove the full Windows/macOS cross-host interface set on synthetic state only
- keep failures attributable to setup/bootstrap/transport/runtime boundaries

Required shape:

- one disposable `ATM_HOME` per host
- one disposable `ATM_CONFIG_HOME` per host
- one disposable `ATM_LOG_DIR` per host
- explicit `ATM_DAEMON_PEER_ADDR` configuration per host using a literal
  `IP:port`
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
- exact peer address value each host should point at
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

- invalid/missing peer address guidance in the runbook is a `SETUP-GAP`
- peer address supplied as a hostname rather than a literal `IP:port` is a
  `SETUP-GAP` if the docs allowed it, otherwise an `ENV-MISTAKE`
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
- exact env/daemon/peer-address contract for both hosts
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

- validate the main cross-host interface set on clean-room state

Outputs:

- formal ownership of checklist rows `AG-VAL-003` through `AG-VAL-007`
- send/read coverage in both directions
- `--requires-ack` ack round-trip coverage
- precise findings for any interface failures

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

### AG.3 Degraded Path And Retry-Visible Recovery

Primary objective:

- prove non-happy-path behavior remains visible and correctly classified

Outputs:

- checklist rows `AG-VAL-008` and `AG-VAL-009`
- degraded-notification proof after durable send
- interruption/restart/recovery proof
- evidence that failures are not misclassified as delivery failure when the
  durable write already succeeded

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

### AG.4 Copied-State Revalidation

Primary objective:

- rerun the approved subset on disposable copied state once Lane A is already
  green

Outputs:

- copied-state revalidation evidence
- exact operator repair/setup notes for realistic-state execution

Execution owner:

- `arch-ctm` with Windows/macOS operators capturing host-side evidence
- `windows-operator` and `macos-operator` execute the concrete host-local
  commands and produce the retained artifacts consumed by AG.4

Verification owner:

- `quality-mgr`

### AG.5 Findings Closeout And Release Verdict

Primary objective:

- close remaining planning-time findings and record the release verdict

Outputs:

- final findings ledger
- readiness record
- explicit statement of whether the `1.3.1` cross-host claim is authorized,
  blocked, or partially blocked

Execution owner:

- `arch-ctm`

Verification owner:

- `quality-mgr`

## Exit Criteria

Phase `AG` is complete only when all of the following are true:

- the clean-room cross-host lane is fully executed with evidence
- the copied-state lane is either green or explicitly blocked by a named
  product defect outside operator/setup ambiguity
- every failed row has a named finding and required next action
- the readiness record states whether `1.3.1` cross-host communication is
  release-usable
- if code changes were needed, they came from concrete findings rather than
  speculative pre-work
