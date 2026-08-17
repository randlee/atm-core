---
title: Phase AG Plan
status: planned
branch: develop
worktree: ../atm-core
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
- historical loopback diagnostic work (`AG.3`)
- prerequisite control-plane product work (`AG.4` / `AG.5`)
- doctor-surface closure on the new product controls (`AG.6`)
- renewed live cross-host validation once the product is actually operable
  (`AG.7`)
- late transport-security planning/reconciliation (`AG.8`)
- secured transport implementation (`AG.10`)
- corrective remote-target routing and revalidation ladder after the reviewed
  AG.6-AG.10 line (`AG.11` through `AG.17`)
- post-AG.15 ruthless-boundary cleanup and cross-host unification ladder
  (`AG.18` through `AG.25`)

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

Phase `AG` now has three distinct records:

- historical early execution on `feature/cross-host-communication`
- reviewed corrective replan history on
  `plan/phase-ag-multihost-advertise-allowlist`
- current hardened planning source merged into `develop`, with execution on
  separate sprint worktrees

The historical lines remain important evidence, but they no longer define the
forward phase sequence by themselves. The current branch is the planning source
of truth because it reconciles what was actually learned:

- the original validation-only framing was insufficient
- environment-variable-driven peer selection is not the desired product model
- interface selection, allowlist enforcement, and doctor visibility must become
  first-class product surfaces before real cross-host closure can be trusted

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
- the remaining blockers are not another proof-of-concept transport hack; they
  are completion of:
  - the durable control-plane surface
  - the still-missing remote-target send-dispatch contract captured in
    `AG-FIND-005`
- setup ambiguity is itself a real phase finding if it blocks reproducible
  execution
- the highest-value next objective is to land interface/allowlist/doctor
  surfaces so the daemon-to-daemon channel can be exercised through real
  product controls rather than env hacks
- after those surfaces exist, the lowest-risk proof order is:
  - localhost same-host remote-target proof
  - self-IP same-host remote-target proof
  - automated integration coverage
  - other-Mac smoke
  - Windows/macOS smoke

## Remote-Target Contract Rule

The corrective AG.11 line uses exactly two operator-facing remote-target forms:

- `atm send <agent>@<team>.<host> ...`
- `atm send <agent>@<team> --host <host> ...`

Parser rules for the inline form are part of the plan, not an implementation
choice left open for later review:

- agent/member names and team names must not contain `.`
- inline parsing splits on the final `.` after `@`
- the suffix after that final `.` is the remote host
- the prefix before that final `.` is the team name
- mixed inline-host plus `--host` input is rejected instead of silently
  preferring one source

Routing rules are equally narrow:

- empty normalized `remote_host` => local mailbox path
- non-empty normalized `remote_host` => cross-host delivery trait boundary
- sender-side daemons must not write a remote target directly into a local
  mailbox path
- `localhost` and the sender host's own advertised or bound IP address are
  ordinary non-empty remote-host values on that same remote-delivery path

Remote-delivery result rules:

- if the cross-host path is currently healthy, the CLI may wait up to `10s`
  for remote acceptance
- if the cross-host path is currently unhealthy, the CLI returns immediately
  with a deferred-delivery result
- "healthy" means the daemon has a currently usable enabled interface row, a
  resolvable outbound target, and no cached terminal-failure state for that
  host
- the daemon may continue bounded background retry for `60s..120s` only for
  transient runtime failure kinds:
  - connect timeout
  - connection refused
  - connection reset
  - host/network unreachable
- terminal runtime failure kinds never spend the retry budget:
  - allowlist rejection
  - authentication / certificate rejection
  - protocol rejection
  - malformed target
- deferred retry cadence is fixed:
  - initial retry interval `5s`
  - exponential backoff `2x`
  - per-attempt interval cap `30s`
  - bounded jitter `±20%`
  - hard attempt cap `6`
- concurrent deferred background deliveries are bounded to `256` per host
- the daemon must emit structured tracing events for:
  - immediate unhealthy-path classification
  - each deferred retry attempt
  - retry success after a deferred result
  - retry exhaustion
  - terminal rejection without retry
  - final sender-inbox receipt emission
- a daemon restart during the deferred window must resume the pending retry and
  receipt obligation from durable state until the bounded window expires
- the daemon concludes deferred work by appending a final delivery/failure
  receipt into the sender inbox

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
- localhost same-host remote-target unauthorized rejection before mailbox
  mutation
- localhost same-host remote-target full-function success with real ATM
  payloads
- self-IP same-host remote-target unauthorized rejection before mailbox
  mutation
- self-IP same-host remote-target full-function success with real ATM payloads
- automated integration coverage that proves:
  - both supported remote-target CLI forms normalize identically
  - remote-target sends do not fall back to the local mailbox path
- localhost and self-IP same-host proof both cover send/read/ack,
    nudge/notification classification, and retry-visible recovery
- other-Mac host-pair smoke covering unauthorized rejection plus the same
  full-function matrix used on localhost/self-IP same-host proof
- Windows/macOS host-pair smoke covering unauthorized rejection plus the same
  full-function matrix used on localhost/self-IP same-host proof
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

## Corrective Final-Verdict Rule

The reviewed AG.6-AG.10 sprint docs remain authoritative historical planning
artifacts and are not silently rewritten by the corrective line.

However, once the post-AG.15 ruthless-boundary findings exist, the
authoritative closeout for the original corrective verdict line remains
`AG.17`, while the authoritative unification-proof closeout for the remaining
CROSSHOST-UNIFY line moves to `AG.25`.

That means:

- `AG.9` remains the reviewed earlier final-verdict sprint for the pre-
  corrective line
- `AG.10` remains the security prerequisite sprint for any transport-security
  claim
- `AG.17` remains the historical corrective-line final-verdict sprint for
  AG.11 through AG.16
- `AG.25` is the final live-proof sprint for the AG.18 through AG.24
  unification line

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

This finding-classification enum is evidence/QA-only; it does not control
runtime retry behavior.

The authoritative runtime retry-decision enum for the corrective line is:

- `Transient`
  - connect timeout
  - connection refused
  - connection reset
  - host/network unreachable
- `Terminal`
  - allowlist rejection
  - authentication / certificate rejection
  - protocol rejection
  - malformed target

Only `Transient` runtime failures may spend the bounded deferred retry budget.
`Terminal` runtime failures must emit an immediate terminal result/receipt with
no retry spending.

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

### AG.8 Transport Security And Encryption Hardening

Primary objective:

- reconcile and plan the late transport-security gap after functional
  cross-host behavior is real

Outputs:

- requirements/architecture reconciliation for transport security
- implementation plan for encryption / peer-auth hardening
- explicit release-language boundaries while this remains open

Entry gate:

- functional host-pair validation is already credible on the AG.4 / AG.5
  surfaces

### AG.10 Secured Cross-Host Transport Implementation

Primary objective:

- implement the actual secured daemon-to-daemon transport defined by AG.8

Outputs:

- secured transport implementation
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

Execution owner:

- `arch-ctm`

Verification owner:

- `quality-mgr`

### AG.11 Corrective Remote-Target Contract And Dispatch Routing

Primary objective:

- close the missing send-routing gap by making remote-target syntax a
  first-class CLI and runtime contract instead of a local-mailbox fallthrough

Outputs:

- exact remote-target syntax:
  - `atm send <agent>@<team>.<host> ...`
  - `atm send <agent>@<team> --host <host> ...`
- exact parser contract:
  - agent/member names and team names must not contain `.`
  - split the inline form on the final `.` after `@`
  - mixed inline-host plus `--host` input is rejected
- one typed `remote_host` field in the send request model
- one dispatch rule:
  - empty `remote_host` => local mailbox path
  - non-empty `remote_host` => cross-host delivery trait boundary
- explicit rejection/error path when a remote-target send cannot use the
  cross-host delivery path
- one delivery-result policy:
  - healthy path => wait up to `10s` for remote acceptance
  - unhealthy path => return immediate deferred-delivery result
  - daemon continues bounded retry for `60s..120s`
  - final delivery/failure receipt lands in sender inbox
- socket transport reuses the same ATM wire message shapes already used on
  other transports; no transport-specific socket schema is introduced
- `localhost` and self-IP same-host rows remain ordinary non-empty remote-host
  sends on that same branch; no localhost-special code path is allowed
- one authoritative deletion/reduction ledger for retained AG.3-AG.10
  cross-host surfaces:
  - remove env-driven peer endpoint control (`ATM_DAEMON_PEER_ADDR`) from the
    intended steady-state operator path
  - remove CLI-only loopback transport compatibility paths that bypass the
    daemon runtime
  - reduce any cross-host parsing or socket-policy logic that leaks outside the
    sealed delivery/storage boundaries
- requirements / architecture / ADR updates aligned to that dispatch rule
- finding handoff to `AG-FIND-005`

Entry gate:

- the reviewed AG.6-AG.10 planning line remains intact
- corrective work is appended after AG.10 rather than silently rewriting
  reviewed sprint scope

Execution owner:

- `arch-ctm`

Verification owner:

- `quality-mgr`

### AG.12 Localhost Full-Function Same-Host Remote-Target Proof

Primary objective:

- prove 100% of the remote-target functionality on `localhost` before
  involving self-IP or a second host

Outputs:

- localhost same-host unauthorized rejection evidence (`AG-VAL-016`)
- localhost same-host full-function success evidence (`AG-VAL-017`)
- localhost transport-security disposition (`AG-VAL-018`)
- retained proof that real ATM payloads traverse the peer-transport path
  instead of the local mailbox path
- exact localhost runbook additions for the corrective path

Entry gate:

- AG.11 routing work is complete enough that localhost remote-target sends no
  longer fall back to the local mailbox path

Execution owner:

- `arch-ctm`

Verification owner:

- `quality-mgr`

### AG.13 Self-IP Full-Function Same-Host Proof

Primary objective:

- rerun the full remote-target functionality on one host through its own
  advertised or bound IP address

Outputs:

- self-IP same-host unauthorized rejection evidence (`AG-VAL-019`)
- self-IP same-host full-function success evidence (`AG-VAL-020`)
- retained proof that bind/advertise configuration and allowlist enforcement
  both survive ordinary same-host IP addressing
- exact self-IP same-host setup instructions for the corrective path

Entry gate:

- AG.12 localhost remote-target closure is complete

Execution owner:

- `arch-ctm`

Verification owner:

- `quality-mgr`

### AG.14 Automated Integration Coverage For The Corrective Path

Primary objective:

- turn the AG.11-AG.13 corrective behavior into durable automated integration
  coverage so the release does not depend only on manual smoke

Outputs:

- parser/normalization integration coverage for both supported remote-target
  syntaxes
- dispatch integration coverage proving remote-target sends no longer write to
  the local mailbox path
- localhost/self-IP same-host integration coverage for send/read/ack,
  unauthorized rejection, nudge/notification classification, and
  retry-visible recovery
- at least one ADR-003 Tier-3 real-socket or real-daemon-spawn integration
  test that exercises the production `CrossHostDelivery` path end to end

Entry gate:

- AG.11 dispatch routing is complete
- AG.12 and AG.13 have defined the exact same-host behavior the automated suite
  must lock in

Execution owner:

- `arch-ctm`

Verification owner:

- `quality-mgr`

### AG.15 Other-Mac Cross-Host Smoke

Primary objective:

- prove the corrective path survives a real second-host topology on another
  Mac before introducing Windows-specific variables

Outputs:

- other-Mac smoke evidence:
  - `AG-VAL-021A`
  - `AG-VAL-021B`
  - `AG-VAL-021C`
  - `AG-VAL-021D`
  - `AG-VAL-021E`
  - `AG-VAL-021F`
- retained evidence for authorized send/read/ack and unauthorized rejection on
  a Mac-to-Mac host pair
- first-line recovery notes for firewall, routing, or operator mistakes
  discovered on the second-host path

Entry gate:

- AG.13 self-IP same-host proof is complete
- AG.14 automated integration coverage is complete enough to make second-host
  failures actionable rather than ambiguous

Execution owner:

- `arch-ctm`
- `macos-operator`

Verification owner:

- `quality-mgr`

### AG.16 Windows/macOS Cross-Host Smoke

Primary objective:

- prove the corrective path survives the real heterogeneous-host topology

Outputs:

- Windows/macOS smoke evidence:
  - `AG-VAL-022A`
  - `AG-VAL-022B`
  - `AG-VAL-022C`
  - `AG-VAL-022D`
  - `AG-VAL-022E`
  - `AG-VAL-022F`
- retained evidence for authorized send/read/ack and unauthorized rejection on
  the Windows/macOS host pair
- recovery/runbook deltas for Windows-specific firewall, routing, or daemon
  bring-up behavior

Entry gate:

- AG.15 other-Mac smoke is complete

Execution owner:

- `arch-ctm`
- `windows-operator`
- `macos-operator`

Verification owner:

- `quality-mgr`

### AG.17 Corrective Copied-State Revalidation And Release Verdict

Primary objective:

- rerun the approved subset on disposable copied state after the AG.11-AG.16
  corrective line is green, then record the release verdict that accounts for
  both the original AG line and the corrective line

Outputs:

- copied-state rerun of the approved corrective subset
- final findings-ledger reconciliation
- final readiness verdict after AG.11-AG.16
- explicit statement of whether cross-host is:
  - functionally release-usable
  - blocked
  - functionally usable but not transport-secure

Entry gate:

- AG.16 Windows/macOS smoke is complete enough to justify copied-state rerun
- AG.10 security status is known before the final verdict is issued
- AG.9 is treated as historical reviewed verdict scope only; AG.17 is the
  authoritative final verdict for the corrective line

Execution owner:

- `arch-ctm`

Verification owner:

- `quality-mgr`

### AG.18 Collapse Compose And DirectDeliver Into One Envelope And Handler

Primary objective:

- delete the duplicate message-semantic envelope/handler split so send and ack
  use one canonical send request family

Outputs:

- one canonical outbound send envelope
- one canonical inbound send handler family
- deletion of the `Compose`/`DirectDeliver` semantic split

Entry gate:

- AG.15 ruthless-boundary review findings are accepted as follow-on scope

### AG.19 Delete Separate Remote-Ack Execution Path

Primary objective:

- make remote ack use the same outbound send path as ordinary send and restore
  confirmed-delivery-only source-state mutation

Outputs:

- no separate remote-ack execution function
- source ack state commits only after confirmed remote delivery

Entry gate:

- AG.18 is complete

### AG.20 Move Deferred Replay Policy Out Of Transport

Primary objective:

- reduce peer transport to transport only by removing deferred/replay policy
  and replay persistence from the transport implementation

Outputs:

- transport returns transport facts only
- retry/deferred policy is owned above transport

Entry gate:

- AG.19 is complete

### AG.21 Collapse Duplicate Dispatch Routing And Inbound Persistence Paths

Primary objective:

- reduce daemon dispatch to one send decision point and one inbound persistence
  path

Outputs:

- one daemon send dispatch path
- one inbound persistence path for send-shaped requests

Entry gate:

- AG.20 is complete

### AG.22 Relocate Host Matching And Endpoint Selection Out Of Transport

Primary objective:

- move host matching, interface selection, and ambiguity policy out of
  transport into a narrower resolution boundary

Outputs:

- explicit endpoint-resolution boundary
- transport consumes resolved endpoints only

Entry gate:

- AG.21 is complete

### AG.23 Remove Synthetic Deferred Receipt Construction From Daemon Dispatch

Primary objective:

- delete mailbox-visible deferred receipt synthesis from daemon dispatch

Outputs:

- no dispatch-local deferred receipt persistence helper
- one shared outcome policy layer outside dispatcher

Entry gate:

- AG.22 is complete

### AG.24 Stop Transport From Mutating Request Shape Before Send

Primary objective:

- preserve the canonical send request shape across transports

Outputs:

- no transport-layer `remote_host` clearing
- one explicit wire-adapter layer if serialization needs adaptation

Entry gate:

- AG.23 is complete

### AG.25 Live Two Daemon Pair Proof For Unified Cross Host Delivery

Primary objective:

- prove the unified post-AG.18-AG.24 design on real daemon pairs

Outputs:

- localhost, self-IP, and real cross-host proof rows
- retained evidence that ack follows the same outbound path as send

Entry gate:

- AG.18 through AG.24 are complete

## Exit Criteria

Phase `AG` is complete only when all of the following are true:

- the accepted historical AG.1 / AG.2 / AG.3 work is reconciled into the final
  phase sequence
- the AG.4 / AG.5 control-plane product work is complete
- the AG.6 doctor surface is complete
- the clean-room cross-host lane is fully executed with evidence on that real
  product surface
- the AG.11 corrective remote-target routing work is complete
- the AG.12 localhost same-host proof lane is complete
- the AG.13 self-IP same-host proof lane is complete
- the AG.14 automated integration suite locks in the corrective path
- the AG.15 other-Mac smoke lane is complete
- the AG.16 Windows/macOS smoke lane is complete
- the AG.17 copied-state and final corrective verdict are complete
- the AG.18 envelope/handler collapse is complete
- the AG.19 remote-ack path deletion is complete
- the AG.20 deferred/replay policy relocation is complete
- the AG.21 dispatch/inbound persistence collapse is complete
- the AG.22 host-resolution boundary move is complete
- the AG.23 deferred-receipt dispatch deletion is complete
- the AG.24 request-shape preservation work is complete
- the AG.25 live two-daemon proof lane is complete
- if secure transport is part of the release claim, AG.10 is complete
- the copied-state lane is either green or explicitly blocked by a named
  product defect outside operator/setup ambiguity
- every failed row has a named finding and required next action
- the readiness record states whether `1.3.1` cross-host communication is
  release-usable
- any functional release-usable statement is explicit about whether it excludes
  transport security
