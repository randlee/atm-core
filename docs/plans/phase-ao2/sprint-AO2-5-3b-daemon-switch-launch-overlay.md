---
phase: AO2
sprint: AO2.5.3b
title: Daemon-switch typed temporary launch overlay
branch: pending-worktree-provisioning
integration_branch: integrate/phase-ao2
status: draft_for_review
depends_on:
  - ADR-047-layered-peer-wire-security
  - ADR-052-benchmark-account-isolation-and-snapshot-policy
  - PR-976-Apple-signing-migration
blocks:
  - AO2.5.4-harness-integration-and-evidence-schema
  - AO2.5.5-physical-proof-and-rollback-drill
---

# AO2.5.3b — Daemon-switch typed temporary launch overlay

## Decision summary

`daemon-switch` needs one narrowly typed, crash-recoverable operational
transaction for launching the already selected ATM CLI/daemon pair temporarily
in `mutual-tls` or `plaintext-test` peer-wire mode.  It is a control-plane
feature: it changes the managed service launch specification before a restart,
then restores the exact prior specification.  It does not add a benchmark
daemon, a child-process fallback, an environment selector, an alternate
`HostRuntimeScope`, a second HTTP route, or any work to the admission hot path.

AO2.5.4 must consume this capability rather than reimplementing per-platform
service control in `run_admission_capacity.py`.  The harness will receive only
typed evidence from `daemon-switch`; it may neither write a service file nor
pass arbitrary daemon arguments.

## Problem statement and root cause

ADR-047 deliberately makes peer-wire security a process-local launch decision:
normal startup defaults to mTLS and a bounded diagnostic/benchmark invocation
may use only `--peer-wire-security plaintext-test`.  AO2.5 also requires a
physical benchmark to use a managed, selected release pair and a dedicated
benchmark OS account.  The current `daemon-switch` can switch/restart the pair,
but it cannot temporarily alter the managed service's launch arguments.

The retired harness attempted to bridge that gap with a direct daemon child and
`ATM_HOME`-based setup.  That violates ADR-026's one canonical runtime scope,
creates two lifecycle owners, and previously exposed the interactive database
to replacement.  A macOS-only plist helper is not an adequate replacement:
Windows service `binPath`, macOS LaunchAgent plists, and Linux systemd-user
units have different configuration and rollback mechanics.

The required capability is therefore an explicit cross-platform service
configuration transaction.  If any platform's current service specification
is unsupported or ambiguous, the tool must refuse before stopping the daemon;
there is no direct-process or environment-based fallback.

## Goals, scope, and non-goals

### Goals

1. Keep CLI and daemon selection paired and preserve the existing macOS signing
   gate before every lifecycle-changing operation.
2. Select exactly one typed wire mode: `mutual-tls` or `plaintext-test`.
3. Capture enough durable state to recover the original service configuration
   after interruption, process crash, reboot, or a failed restart.
4. Give macOS, Windows, and Linux the same operator contract and structured
   evidence even though their service mechanisms differ.
5. Make a normal daemon restart mTLS again after successful restoration.

### In scope

- a `daemon-switch` temporary-launch session API and persisted recovery journal;
- platform adapter implementations for a current-user macOS LaunchAgent, the
  configured Windows SCM service, and systemd `--user` service;
- atomic capture/restore, fail-closed validation, bounded stop/start/doctor
  proofs, and a deliberate recovery command;
- unit, adapter-contract, failure-injection, and platform smoke evidence;
- requirements/ADR records and an architecture guard that keeps this feature
  outside the ATM runtime/harness hot path.

### Out of scope

- changing `atm-http-runtime`, its router, `WriteRequest`, storage, TLS stream
  adapter, post-write scheduling, or benchmark workload;
- modifying the frozen synchronous daemon; all managed service launches target
  the Tokio/Axum `atm-http-runtime` daemon binary;
- arbitrary daemon arguments, arbitrary service edits, an operator-selected
  endpoint/database root, an environment peer-wire selector, or a generic
  service-management API;
- automatic repair that guesses a missing original configuration;
- execution of a physical benchmark.  AO2.5.4/5 own harness integration and
  evidence after this capability is reviewed and implemented.

## Governing contracts

| Contract | Constraint carried into this sprint |
| --- | --- |
| ADR-026 | One OS user has one non-configurable durable root, lock ownership, endpoint, and daemon. A launch overlay cannot select an alternate one. |
| ADR-047 / `REQ-CORE-TRANSPORT-002B1` | mTLS is default. `plaintext-test` is process-local, typed, non-durable, never environment-driven, and uses the identical HTTP/router/persistence path. |
| ADR-052 / `REQ-P-BENCHMARK-001` | Only the dedicated benchmark account can run a physical benchmark; setup and lifecycle remain outside the timed interval. |
| `REQ-P-PLATFORM-001/002` | macOS, Linux, and Windows have product-level parity; unsupported local service shapes fail clearly rather than silently omitting a platform. |
| PR-976 signing migration | `daemon-switch` continues to require the available Apple Development signature for both selected binaries before macOS lifecycle mutation. |

Before implementation, create and review **ADR-053 — Typed Temporary
Daemon-Service Launch Overlay** and **`REQ-P-DAEMON-SWITCH-001`**.  They must
state the transaction/recovery guarantees below and cross-link ADR-047,
ADR-052, and `REQ-P-BENCHMARK-001`.  The requirement must not imply that
plaintext is normal production configuration or that a benchmark has authority
to modify an interactive account.

## Operator contract

The implementation exposes a purpose-built session rather than a generic
argument-passthrough flag.  Exact subcommand spelling remains subject to
ADR-053 review, but the command model is fixed:

```text
daemon-switch temporary-launch begin \
  --peer-wire-security {mutual-tls,plaintext-test} --yes <normal selectors>
daemon-switch temporary-launch quiesce --session <id> --yes <normal selectors>
daemon-switch temporary-launch restart --session <id> --yes <normal selectors>
daemon-switch temporary-launch restore --session <id> --yes <normal selectors>
daemon-switch temporary-launch recover --session <id> --yes <normal selectors>
```

`<normal selectors>` means the existing explicit selected CLI link, selected
daemon link, service name, and (on macOS) LaunchAgent plist selector.  The
session API cannot infer a different endpoint, service, account, or binary.

`begin` validates the selected matching pair, captures the original service
configuration, durably records the session, stops the singleton, applies the
typed overlay, starts the same selected pair, and waits for `atm doctor --json`
to prove pair/version and selected wire mode.  `quiesce` stops only that
session's daemon.  `restart` requires the matching active session and starts
the overlay service after the normal stop proof.  `restore` stops the overlay,
restores the exact captured original service configuration, removes only the
owned overlay after durable verification, starts the selected pair normally,
and proves doctor reports mTLS/default state.

`recover` is intentionally not implicit.  A later invocation that sees an
unfinished session refuses normal `switch`, `restart`, or a second `begin` and
prints the one exact recovery command.  This prevents a later agent from
overwriting the only restoration evidence.  It also makes an interrupted
session visible before a benchmark can run.

The session returns one JSON evidence object: session ID, selected pair
versions/hashes, platform adapter, service identifier, original/overlay
configuration digests (never secrets), requested/doctor-observed wire mode,
each lifecycle phase's monotonic duration, and recovery disposition.

## Transaction and recovery state machine

The recovery journal is an owned file below the existing per-user
`daemon-switch` state directory, separate from the saved default binary pair.
It is written through a private atomic-write helper: private parent directory,
temporary same-directory file, file fsync, atomic replace, then parent-directory
fsync where supported.  Journal files are owner-only.  They retain only the
minimum original service material required for exact restoration (a source
path/digest on macOS/Linux and the exact `binPath` scalar on Windows); exported
evidence contains digests and redacted metadata only.  They never record
private keys, certificate contents, or raw trust records.

| Phase | Durable journal before mutation | Required proof to advance | Failure disposition |
| --- | --- | --- | --- |
| `captured` | session ID, account/service identity, selected pair digest, typed mode, original configuration bytes/path or raw `binPath`, original digest | source configuration is owned, unambiguous, and matches selected service | no daemon stop; leave journal and report refusal/recovery |
| `stopped` | `captured` plus stop intent | service unloaded/stopped and singleton owner absent | leave service stopped; `recover` uses original capture |
| `overlay_applied` | overlay location/raw config and digest, while original remains intact | overlay digest and service-manager reload/check succeed | do not start; `recover` restores original |
| `overlay_started` | start attempt and selected pair identity | doctor proves matched pair and requested mode | bounded stop then `recover`; never repoint to a different pair |
| `quiesced` | session remains active, service state stopped | singleton absent | session can `restart` or `restore` |
| `restoring` | original digest and overlay removal intent | original config restored exactly and manager reload/check succeeds | retain journal and diagnostic artifacts; service remains stopped |
| `completed` | completion evidence only after normal start/doctor proof | matched pair and default mTLS observed | atomically delete active-session journal; retain non-secret evidence |

The original specification is immutable recovery material.  Every operation
checks the expected digest before replacing or deleting an owned overlay.
Digest mismatch, missing journal, conflicting active session, a different
service identifier, a changed selected pair, a running unexpected daemon, or
ambiguous command shape is a typed fail-closed error.  It must not "best
effort" append an argument, start a child process, or clean up unknown files.

## Platform adapter design

All adapters implement one private control-plane boundary, conceptually
`CapturedLaunchSpec`, `apply_overlay(mode)`, `restore_exact()`, and
`inspect()`.  The boundary accepts the typed `PeerWireSecurity` value, never
raw argument text.  It owns external command/file I/O and produces structured
redacted evidence; `daemon-switch` owns state-machine ordering and pair/doctor
validation.

### macOS — controlled LaunchAgent overlay

1. Require the existing `--launch-agent-plist` and verify it is a regular,
   readable, current-user-owned file with a recognized launchd label and an
   unambiguous `ProgramArguments` array for the selected Tokio/Axum daemon.
2. Read and hash the original bytes without modifying them.  Reject duplicate,
   malformed, or pre-existing `--peer-wire-security` values rather than trying
   to normalize an unknown plist.
3. Render an owned overlay plist in the daemon-switch state directory by
   copying the accepted plist structure and adding precisely one rendered
   typed argument.  Preserve every unrelated key/value exactly at the plist
   data-model level; no shell command string is involved.
4. Persist `captured`, `bootout` the original LaunchAgent, prove it unloaded,
   then `bootstrap` the overlay plist and prove launchd loaded that exact
   overlay path.  The normal source plist is never rewritten.
5. Restore by booting out the overlay, verifying the original byte hash has
   not changed, bootstrapping the original plist path, doctor-proving normal
   mTLS, and only then deleting the owned overlay/journal.

If an operator changed the original plist during the session, restore stops
and retains the overlay/journal for inspection; it must never overwrite that
operator change.

### Windows — SCM `binPath` transaction

1. Query the named configured ATM service with `sc.exe qc`; capture the raw
   `BINARY_PATH_NAME` result and a digest before stopping it.  Prove that the
   service exists and is an unambiguous invocation of the selected daemon.
2. Parse/render the command line with a dedicated Windows argv codec whose
   round-trip behavior is tested against Windows quoting cases.  It rejects
   duplicate/pre-existing peer-wire arguments and unsupported service wrappers
   rather than using a shell append.
3. Build the overlay `binPath` from the validated argv plus exactly one typed
   peer-wire option.  Store both raw original and generated raw overlay values
   in the journal; run `sc.exe config` only after `stopped` is durable.
4. Start through SCM and prove doctor observes the selected pair/mode.  On
   restore, stop first, confirm the currently installed raw `binPath` equals
   the owned overlay digest, then set the exact original raw string and start
   normally.

No service description, account, start type, failure action, or unrelated SCM
field is changed.  If service administration privileges are unavailable or
the current installed value does not match expected state, the command fails
without a direct-process alternative.

### Linux — systemd user-unit drop-in

1. Require a named `systemd --user` service and inspect its resolved unit plus
   current `ExecStart`.  Support only one explicit ATM daemon command shape
   that the adapter can parse and render losslessly; reject multiple commands,
   shell indirection, environment-file selection, template ambiguity, or a
   pre-existing peer-wire argument.
2. Preserve the source unit identity and `ExecStart` digest.  Generate only an
   owned drop-in beneath the user's systemd configuration directory, with an
   `ExecStart=` reset followed by the validated base invocation and exactly one
   typed security option.  Record the drop-in path/digest before
   `daemon-reload`.
3. Stop, reload, start, and doctor-prove the selected pair/mode.  Restore
   stops the service, verifies the owned drop-in digest, removes only that
   drop-in, reloads, starts normal service state, and doctor-proves mTLS.

The adapter cannot rewrite the base unit or consume an arbitrary systemd
override graph.  Ambiguity is a supported refusal with remediation guidance,
not a reason to weaken the control-plane boundary.

## Work breakdown and dependency DAG

```text
AO2.5.3b.1 requirements + ADR-053
       │
       ├── AO2.5.3b.2 shared journal/state machine + typed CLI
       │       ├── AO2.5.3b.3 macOS adapter
       │       ├── AO2.5.3b.4 Windows adapter
       │       └── AO2.5.3b.5 Linux adapter
       │                    │
       └────────── AO2.5.3b.6 cross-platform contract/failure tests
                              │
                       AO2.5.3b.7 physical adapter proofs + review
                              │
                      unblocks AO2.5.4 harness integration
```

1. **Requirements and ADR decision.** Add ADR-053, `REQ-P-DAEMON-SWITCH-001`,
   traceability links, error/recovery vocabulary, and this plan's reviewed
   command/session contract.  No implementation before this closes.
2. **Shared control-plane session.** Refactor only
   `.claude/skills/daemon-switch/scripts/daemon-switch.py` and its direct
   tests.  Add typed parsing, strict selector/pair/signature preflight,
   durable state journaling, evidence schema, and recovery gate.  Do not alter
   the daemon binary or benchmark runner.
3. **macOS adapter.** Implement plist capture/owned overlay/restore and unit
   tests using temporary plists plus a fake launchctl boundary.  Then perform a
   fresh signed release-pair loopback proof with no primary-database access.
4. **Windows adapter.** Implement SCM capture/quoted argv/restore using a fake
   `sc.exe` boundary and Windows-native tests.  A real fastpc4 proof is
   required before it is called supported.
5. **Linux adapter.** Implement narrow systemd-user unit/drop-in support using
   a fake `systemctl` boundary and Linux-native tests.  A real user-service
   proof is required before it is called supported.
6. **Failure/recovery matrix.** For every transition point, inject failure and
   verify either no mutation occurred or the exact original configuration is
   recoverable from retained journal material.  Verify a second session and
   normal switch/restart are blocked while an active journal exists.
7. **Review and handoff.** Run `just lint`, `just test`, all adapter contract
   tests, platform-specific real service proof, and a final architecture sweep.
   QA must inspect the raw session evidence and source/destination configuration
   digests.  Only then may AO2.5.4 receive a worktree/task assignment.

## Acceptance criteria and test matrix

| Case | macOS | Windows | Linux | Required result |
| --- | --- | --- | --- | --- |
| Normal mTLS begin/restore | LaunchAgent overlay | SCM binPath | systemd-user drop-in | paired doctor proves mTLS; exact source configuration restored |
| Plaintext-test begin/restore | same | same | same | doctor/log/evidence prove plaintext only during active session |
| Invalid mode / raw option | parser | parser | parser | rejected before service/configuration mutation |
| Existing/duplicate mode argument | plist validation | argv validation | ExecStart validation | rejected before stop |
| Capture failure | source read | `sc qc` | unit inspection | no stop and no overlay |
| Overlay write/config failure | staged plist | `sc config` | drop-in/reload | journal preserves original and daemon remains/recoverably stops |
| Start/doctor failure | bootstrap | SCM start | systemd start | bounded stop; journal supports exact recovery |
| Crash after each journal phase | fake boundary | fake boundary | fake boundary | next command refuses normal lifecycle and `recover` restores exactly |
| Operator source change | source hash change | raw binPath mismatch | drop-in/base mismatch | no overwrite; retained actionable recovery evidence |
| Pair/signature mismatch | selected-pair gate | selected-pair gate | selected-pair gate | no service mutation |
| Runtime/performance guard | static boundary test | static boundary test | static boundary test | no `atm-http-runtime` admission/persistence/router change; no timing work in profile |

The shared test suite treats `ResourceWarning` as an error, tests atomic journal
failure paths, and validates that the evidence is redacted.  CI runs the
platform-neutral suite on every OS; macOS/Windows/Linux use their native
adapter tests.  Physical proofs use only the disposable benchmark OS account
and must prove a curl receiver check, CLI loopback send/read, and same-host
send/read after both mTLS and plaintext-test sessions.  Cross-host proofs are
AO2.5.5 work, not a prerequisite for this control-plane implementation.

## Performance-regression prevention

This feature must be mechanically absent from the admission pipeline.  It runs
only before/after a physical profile and only in `daemon-switch`, so it must
not add a per-request branch, logging call, TLS construction, filesystem read,
or environment lookup to plaintext admission.  A static architecture test
shall reject imports/calls from `atm-http-runtime`, direct-peer connector,
router, persistence, and benchmark `run_profile` into the temporary-launch
module.

AO2.5.4 records lifecycle setup/teardown with monotonic timestamps outside its
timed samples.  Its result is invalid if a session start, stop, snapshot,
restore, doctor, or evidence write overlaps `run_profile`.  AO2.5.3b itself
does not claim any throughput result.

## Rollback and operational recovery

Code rollback reverts the `daemon-switch` feature; it does not revert
ADR-047's mTLS default or weaken PR-977's primary-database refusal.  An active
session is operationally recovered with the persisted `recover` command:

1. validate journal version, account/service identity, selected pair, and
   original/overlay digests;
2. stop the known managed service and prove singleton absence;
3. restore only the exact captured service configuration;
4. reload the OS service manager where required;
5. start the existing selected pair normally and doctor-prove mTLS; and
6. retain redacted evidence before deleting the active journal.

If any step cannot prove its precondition, it leaves the service stopped and
the journal/artifacts retained.  The tool reports the failed phase and precise
manual inspection path.  It never changes an unknown service configuration,
starts another daemon, or touches SQLite.

## Boundary review and rejection criteria

- **Daemon runtime boundary:** only the Tokio/Axum `atm-http-runtime` daemon is
  selected/launched.  Any proposal to revive or patch the synchronous legacy
  daemon is rejected and routed to the AL.5–AL.7 cutover work.
- **CLI/daemon pair boundary:** `daemon-switch` retains exclusive lifecycle
  ownership; the harness calls it but never owns child processes or platform
  service files.
- **Security boundary:** only typed command parsing constructs `PeerWireMode`.
  Environment variables, durable config, certificate availability, and TLS
  errors cannot choose plaintext or fall back to it.
- **State boundary:** the feature journals service configuration only.  It has
  no SQLite, roster, message, trust-store, or credential mutation authority.
- **Platform boundary:** every OS adapter is narrow and lossless for its
  accepted managed-service shape.  The tool rejects unknown shapes rather than
  exposing a generic edit facility.

Any implementation that weakens one of these boundaries, embeds a hostname or
account name, changes a public ATM HTTP request/response path, or introduces a
benchmark-only daemon is out of scope and requires a new ADR/plan review.

## Review checkpoints

Before implementation, reviewers must confirm that the plan is self-contained,
that its DAG has no cycle, that each platform adapter restores a captured
original rather than a synthesized default, and that no requirement permits an
interactive-account service mutation.  After the requirements/ADR pass,
perform architecture, platform, security, and failure-recovery reviews before
the first code task.  AO2.5.4 remains blocked until all of those review gates
pass.
