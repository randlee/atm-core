---
id: AG.14
title: Automated Integration Coverage For The Corrective Path
status: in_progress
branch: feature/pAG-s14-integration-coverage
worktree: ../atm-core-worktrees/feature/pAG-s14-integration-coverage
target: integrate/phase-AG
---

# Sprint AG.14 — Automated Integration Coverage For The Corrective Path

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.14
worktree: ../atm-core-worktrees/feature/pAG-s14-integration-coverage
branch: feature/pAG-s14-integration-coverage
status: in_progress
estimated_scope: medium
```

## Goal

Lock the AG.11-AG.13 corrective behavior into automated integration coverage
so the release does not depend only on manual smoke.

## Deliverables

- parser/normalization integration coverage for both supported remote-target
  syntaxes
- dispatch integration coverage proving remote-target sends never fall through
  to the local mailbox path
- localhost full-function integration coverage mirroring AG.12
- self-IP full-function integration coverage mirroring AG.13
- one narrow ADR-003 Tier-3 real-socket or real-daemon-spawn test covering the
  production `CrossHostDelivery` dispatch path end to end
- automated coverage for:
  - unauthorized rejection
  - authorized send/read/ack
  - nudge/notification classification
  - retry-visible recovery

## Deliverable-to-Test Matrix

| deliverable | fidelity | concrete coverage |
|---|---|---|
| parser/normalization coverage for `<agent>@<team>.<host>` and `--host <host>` | ADR-003 Tier-1 | `atm-core::send::remote_target_parse_tests::remote_target_syntaxes_normalize_to_the_same_contract`; `atm::commands::send::tests::build_request_normalizes_inline_and_explicit_remote_target_forms_equally` |
| localhost remote-target authorized send/read | ADR-003 Tier-2 | `atm-daemon::tests::runtime_root::loopback::dispatcher_loopback_send_round_trips_through_peer_listener_into_self_inbox` |
| localhost remote-target unauthorized rejection | ADR-003 Tier-2 | `atm-daemon::tests::runtime_root::loopback::dispatcher_loopback_send_rejects_unauthorized_host_before_mailbox_mutation` |
| localhost remote-target fail-closed when no listener is available | ADR-003 Tier-2 | `atm-daemon::tests::runtime_root::loopback::dispatcher_loopback_without_listener_fails_closed_without_mailbox_mutation` |
| localhost secure requires-ack round trip | ADR-003 Tier-2 | `atm-daemon::tests::runtime_root::loopback::dispatcher_secure_loopback_requires_ack_round_trips_and_updates_reply_state` |
| localhost secure authorized send/read | ADR-003 Tier-2 | `atm-daemon::tests::runtime_root::loopback::dispatcher_secure_loopback_send_round_trips_through_peer_listener_into_self_inbox` |
| self-IP authorized send/read | ADR-003 Tier-2 | `atm-daemon::tests::runtime_root::self_ip::dispatcher_self_ip_send_round_trips_through_peer_listener_into_self_inbox` |
| self-IP fail-closed when no listener is available | ADR-003 Tier-2 | `atm-daemon::tests::runtime_root::self_ip::dispatcher_self_ip_without_listener_fails_closed_without_mailbox_mutation` |
| self-IP unauthorized rejection | ADR-003 Tier-2 | `atm-daemon::tests::runtime_root::self_ip::dispatcher_self_ip_send_rejects_disabled_host_before_mailbox_mutation` |
| self-IP secure requires-ack round trip | ADR-003 Tier-2 | `atm-daemon::tests::runtime_root::self_ip::dispatcher_secure_self_ip_requires_ack_round_trips_and_updates_reply_state` |
| production `CrossHostDelivery` end-to-end over the real daemon local-IPC surface | ADR-003 Tier-3 | `atm-daemon::tests::runtime_root::local_ipc::local_ipc_client_preflight_round_trips_ack_required_send_after_add_member_roster_state` |
| notification classification for remote-target degradation | ADR-003 Tier-2 | `atm-daemon::peer_transport::tests::harness::localhost_remote_target_notification_degradation_is_classified_without_failing_delivery` |
| retry-visible recovery for deferred remote-target sends | ADR-003 Tier-2 | `atm-daemon::peer_transport::tests::harness::localhost_remote_target_retry_visible_recovery_remains_bounded_and_observable` |

## Acceptance Criteria

- the corrective path is covered by automated integration tests, not only by
  manual smoke
- at least one AG.14 test is explicitly ADR-003 Tier-3 and exercises the live
  production `CrossHostDelivery` path end to end rather than an in-process fake
- localhost and self-IP same-host proof both have automated success and
  rejection coverage
- the integration suite fails if a remote-target send writes to the local
  mailbox path
- the integration suite is suitable for `just test` gating on the corrective
  branch

## Required Validation

- integration tests exist for all AG.11-AG.13 corrective behaviors
- the sprint doc identifies the fidelity tier for each required AG.14 test:
  - parser normalization and branch-selection coverage may be ADR-003 Tier-1
    or Tier-2
  - at least one end-to-end remote-dispatch regression must be ADR-003 Tier-3
- `just test` exercises the new integration coverage on the corrective branch

## Unit-Test Plan

- none beyond any targeted fixture helpers needed by the integration suite

## Integration-Test Plan

- ADR-003 Tier-1 or Tier-2:
  - exact CLI parsing coverage for both remote-target syntaxes
  - exact dispatch-branch coverage for local vs remote sends
- ADR-003 Tier-3:
  - one real-socket or real-daemon-spawn regression proving a remote-target
    send traverses the production `CrossHostDelivery` path and cannot fall back
    to the local mailbox path
  - localhost same-host matrix coverage
  - self-IP same-host matrix coverage
  - no-local-fallback regression coverage

## Smoke-Test Plan

- no new second-host smoke closes this sprint
- AG.15 and AG.16 consume the AG.14 integration suite as a prerequisite safety
  net

## Out Of Scope

- second-host smoke closure by manual evidence alone
- copied-state release verdict

## Entry Gate

- AG.11 dispatch routing is complete
- AG.12 and AG.13 have defined the exact same-host behavior to lock in

## Ownership

- execution owner: `arch-ctm`
- verification owner: `quality-mgr`
