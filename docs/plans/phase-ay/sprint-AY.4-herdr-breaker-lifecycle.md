---
phase: AY
sprint: AY.4
title: Herdr breaker escalation and failure lifecycle
branch: feature/ay4-herdr-breaker-lifecycle
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay4-herdr-breaker-lifecycle
integration_branch: integrate/phase-ay
status: draft
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: core
parallel_with: [AY.8]
stack_parent: feature/ay3-herdr-endpoint-doctor-config
pr_target: feature/ay3-herdr-endpoint-doctor-config
dependency_relations:
  - prerequisite: AY.3
    dependent: AY.4
    relation: must_follow
    rationale: lifecycle assertions consume AY.3's fully composed client configuration, typed endpoint doctor, presence correlation, and deterministic doctor projection; AY.4 is stacked directly on AY.3.
  - prerequisite: AY.4
    dependent: AY.5
    relation: must_follow
    rationale: operator entry ownership starts only after the optional-startup and Herdr-failure model is proven end to end, preventing the installer control plane from becoming an implicit runtime repair mechanism.
  - prerequisite: AY.4
    dependent: AY.8
    relation: parallel_safe
    rationale: AY.4 edits only Tokio/Axum breaker-escalation and real-composition lifecycle-test modules after AY.3's contracts merge; AY.8 owns atm-herdr socket transport, fixtures, boundary revisions, and its architecture exemption.
---

# AY.4 — Herdr breaker escalation and failure lifecycle

Close the runtime failure model on the Tokio/Axum daemon: one durable lead
escalation per breaker-open cycle, bounded recovery after Herdr returns, no
duplicate prompt after an unknown submission, and queued-mail survival across
Herdr and ATM restarts. This is production behavior and real-composition proof,
not a test-only sprint.

## Delivery topology and `/gh-stack`

AY.4 is stacked directly above AY.3 in the linear implementation chain:

```text
integrate/phase-ay <- AY.2 <- AY.3 <- AY.4 <- AY.5 <- AY.6 <- AY.7
```

Use the `/gh-stack` skill noninteractively:

```bash
git config rerere.enabled true
git config remote.pushDefault origin
gh stack link --base integrate/phase-ay \
  feature/ay2-herdr-transport-seam \
  feature/ay3-herdr-endpoint-doctor-config \
  feature/ay4-herdr-breaker-lifecycle
gh pr view feature/ay4-herdr-breaker-lifecycle \
  --json headRefName,baseRefName,state
```

`gh stack link` is the `/gh-stack` operation for this external-worktree flow and
creates no local tracking; verify the PR base with `gh pr view --json`. Phase
AY forbids `gh stack rebase`, `gh stack sync`, and `gh stack merge`. Use merge
commits and no force-push. AY.3 development pushed, not QA, triggers
merge-forward from AY.3 before every AY.4 development/fix round. Parent PRs
merge first.

AY.8 is an independent branch from `integrate/phase-ay` after AY.1–AY.3 merge.
It may run in parallel with AY.4; neither branch merges an unmerged sibling.

## Preconditions

- P-A and P-B from the Phase AY plan are satisfied.
- AY.3 development is pushed, and AY.4 is created from
  `feature/ay3-herdr-endpoint-doctor-config`.
- AY.3's endpoint/config/doctor acceptance suite is green on the parent branch.
- The Phase AX.6 `herdr_escalation::escalate` helper and queue-reminder behavior
  are present in the Phase AY baseline.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial, shape-only,
or test-only completion fails the sprint.

- [ ] D1 — Add a small breaker-cycle escalation component in
  `crates/atm-http-runtime/src/herdr_breaker_escalation.rs` and a thin call from
  the existing Tokio/Axum Herdr queue-wake breaker observation. It invokes the
  Phase AX.6 escalation helper at most once per breaker-open timestamp. Queued
  ATM mail to the lead and configured recipients is durable; Herdr desktop
  notification is an independent best-effort attempt and never contains the
  mail body.
- [ ] D2 — Wire D1 through `atm-http-runtime` composition with no readiness
  dependency on Herdr. A closed/half-open/new-open transition follows C2;
  escalation failure cannot stop the daemon, discard mail, or extend the
  breaker's normal backoff.
- [ ] D3 — Add the complete real-composition failure/recovery suite in C3,
  backed by AY.2's portable fake Herdr and AY.3's doctor projection. Tests drive
  the actual Tokio/Axum composition and queue-wake path, not isolated mocks of
  the behavior being claimed.
- [ ] D4 — Extend the HR-SAFE-003 architecture guard and focused observability
  assertions for D1: notification text contains state/remedy but never mail
  body; each attempt records the breaker cycle, mail outcomes, notification
  outcome, and recovery transition without secrets.

### Paths to delete

None.

## Required work and exact targets

| Ownership | Exact targets |
| --- | --- |
| Production state/dedup | new `crates/atm-http-runtime/src/herdr_breaker_escalation.rs` |
| Runtime hook | one thin integration in `crates/atm-http-runtime/src/herdr_queue_wake.rs` plus module export/composition |
| Composition fixtures | `crates/atm-daemon-bootstrap/src/herdr_lifecycle_tests.rs` and its `#[cfg(test)]` module declaration |
| Architecture/observability proof | the existing HR-SAFE-003 guard and focused runtime tests; no new legacy-daemon allowlist |

New production logic stays outside `herdr_queue_wake.rs`; that existing file
receives only the call needed to pass an observed breaker transition into the
new component. No work may patch or harden the frozen synchronous daemon. No
component starts, supervises, updates, or owns Herdr.

## Code contracts

### C1 — Existing escalation helper

Use the Phase AX.6 helper without changing its durable/best-effort split:

```rust
pub(crate) fn escalate(
    runtime: &LocalServiceRuntime,
    herdr_process: &dyn HerdrProcessAdapter,
    task_store: &dyn TaskStore,
    daemon_home: &Path,
    team: &TeamName,
    mail_body: &str,
    notification: EscalationNotification,
    kind: EscalationKind,
) -> EscalationOutcome;
```

`EscalationOutcome` exposes lead write, configured-recipient writes, and
`notify_ok` independently. A notification failure never rolls back or suppresses
mail. The notification carries endpoint state/remedy only, never `mail_body`.

### C2 — Breaker-cycle deduplication

```rust
pub(crate) struct HerdrBreakerEscalationGate {
    last_escalated_opened_at: Option<IsoTimestamp>,
    last_escalated_at: Option<IsoTimestamp>,
    min_interval: Duration, // default 30 min; config key `herdr.escalation_min_interval_secs`
}

impl HerdrBreakerEscalationGate {
    /// True at most once per distinct Open.opened_at value, and never
    /// sooner than `min_interval` after the previous escalation (uses the
    /// injected clock). A suppressed claim is logged with the next
    /// eligible time.
    pub(crate) fn claim(&mut self, opened_at: IsoTimestamp, now: IsoTimestamp) -> bool;
}
```

The state machine is explicit:

```text
closed/half-open -- new Open(opened_at) --> claim true, remember opened_at and now
same Open(opened_at) --------------------> claim false
later Open(new_opened_at), now - last_escalated_at <  min_interval --> claim false (suppressed, logged)
later Open(new_opened_at), now - last_escalated_at >= min_interval --> claim true, replace both keys
```

The gate does not count polling ticks. The interval bound is what stops a
flapping endpoint from producing an unbounded mail storm: at most one
escalation per `min_interval` per endpoint regardless of how many open
cycles occur. Restarting the ATM daemon forgets the in-memory keys, so one
repeat escalation for an outage that spans a restart is the accepted,
documented behaviour (no durable idempotency store is added; low-code); the
lifecycle fixtures pin it: L10 (daemon restart during one open outage
yields exactly one additional escalation) and L11 (open-close-open flapping
within `min_interval` yields exactly one escalation). No prompt is automatically retried
when its submission outcome is unknown because prompt operations are not
idempotent.

### C3 — Real-composition scenario matrix

| ID | Stimulus | Required result |
| --- | --- | --- |
| L1 | No Herdr binary and no Herdr backend | Tokio/Axum daemon reaches ready; tmux and Hermes graft nudges succeed |
| L2 | Fake Herdr always returns `server_not_running` | Breaker opens, typed doctor state/remedy appears, daemon stays ready, exactly one cycle escalation fires |
| L3 | Fake Herdr changes to `protocol_mismatch` mid-run | Breaker opens, doctor shows `client_server_mismatch`, one escalation fires, first successful half-open call closes breaker |
| L4 | Connection resets during `wait` | No automatic prompt retry or duplicate prompt; pending mail remains and queue wake re-nudges it |
| L5 | ATM daemon restarts with queued mail | SQLite queue survives and backlog drains after readiness |
| L6 | Server is below `HERDR_MINIMUM_VERSION` | Doctor reports `below_minimum`; ordinary calls fail/continue on Herdr's own protocol terms without daemon shutdown |
| L7 | Desktop `notification show` fails | Lead and recipient mail writes remain, `notify_ok == false`, and no second escalation occurs for the same open timestamp |
| L8 | Breaker is already open during doctor | `BreakerPolicy::Bypass` reports current endpoint state while separate breaker projection remains open |
| L9 | Herdr starts after ATM has been running | First successful call after one backoff window closes breaker and queued work resumes without daemon restart |
| L10 | ATM daemon restarts during one open outage | Exactly one additional escalation after restart; no further repeat while the same outage persists |
| L11 | Endpoint flaps open-close-open five times inside `min_interval` (injected clock) | Exactly one escalation; suppressed claims logged with next eligible time |

Every wait uses injected time/deadlines or existing bounded backoff controls;
fixed sleeps and flaky retries are prohibited.

## Failure inventory

| Failure | Stable observation | Recovery |
| --- | --- | --- |
| Durable lead write fails | structured escalation outcome/log names lead write failure | preserve queued work; correct ATM storage and allow the next distinct open cycle to notify |
| Configured-recipient write fails | per-recipient structured outcome | preserve other writes; correct the named recipient/team record |
| Desktop notify fails | `notify_ok == false`; mail outcomes unchanged | inspect Herdr availability; durable ATM mail remains authoritative |
| Repeated open poll | dedup suppresses second attempt for same timestamp | none; wait for half-open/recovery or a new cycle |
| Prompt submission becomes unknown | infrastructure error and pending mail retained | never retry prompt automatically; queue reminder supplies idempotent follow-up |
| Half-open attempt fails | breaker returns to open under existing ADR-058 policy | wait for the next bounded backoff; daemon remains ready |

## Acceptance criteria

- [ ] A1 — Gate table tests prove exactly one `claim` per open timestamp and a
  new claim after a distinct later cycle.
- [ ] A2 — D1 tests prove lead and configured-recipient mail survive desktop
  notification failure, `notify_ok == false`, and HR-SAFE-003 rejects any mail
  body reaching notification arguments.
- [ ] A3 — Every C3 scenario passes through real Tokio/Axum composition; no
  scenario substitutes the frozen synchronous daemon.
- [ ] A4 — L3 and L9 prove recovery within one existing backoff window without
  ATM restart; L4 proves no duplicate prompt on unknown submission.
- [ ] A5 — Open-breaker doctor uses bypass and retains the independent endpoint
  and breaker projections from AY.3.
- [ ] A6 — Source/architecture review finds no `herdr server` launch, Herdr
  supervisor, readiness probe, blocking sleep, private Tokio runtime, or legacy
  daemon change.
- [ ] A7 — Merge gate is 0 blocking, 0 important, and 0 minor in scope;
  quality-mgr posts PASS and CI is green at merge time.

## Required validation

This is the authoritative validation list.

- [ ] V1 — `cargo test -p atm-http-runtime -p atm-daemon-bootstrap` exits zero
  for gate, escalation, and real-composition lifecycle tests.
- [ ] V2 — `just validate` exits zero on all CI lanes.
- [ ] V3 — `python3 .just/check_line_counts.py` exits zero.
- [ ] V4 — Run the HR-SAFE-003 and long-lived-Herdr-child architecture guards;
  both exit zero.
- [ ] V5 — `gh pr view feature/ay4-herdr-breaker-lifecycle --json
  headRefName,baseRefName,state` reports base
  `feature/ay3-herdr-endpoint-doctor-config`; AY.8 is not in this stack.

## Non-closure and out of scope

- Endpoint/config/doctor contracts are closed by AY.3 and are not redesigned.
- Herdr entry install/remove/status/repair and restart coordination are AY.5
  and AY.6.
- Windows process fixes are AY.7; socket transport/cutover are AY.8–AY.9.
- No legacy synchronous-daemon runtime or dispatch work is permitted.
