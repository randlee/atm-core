---
phase: AX
sprint: AX.7
title: Live Herdr dogfood evidence on the integrate head
branch: none (proof sprint on integrate/phase-ax)
integration_branch: integrate/phase-ax
status: draft
recommended_agent: none
recommended_model: n/a
operator: fenix (rand-m5 owner; a proof sprint, not a coding sprint)
execution_track: D
parallel_with: []
dependency_relations:
  - prerequisite: AX.6
    dependent: AX.7
    relation: must_follow
    rationale: proves AX.1 through AX.6 together on the merged integrate head.
---

# AX.7 — Live Herdr dogfood evidence

Repeat the 2026-09-04 dogfood matrix against a daemon built from the
`integrate/phase-ax` head, on the real `atm-dev` Herdr team, and record
the observed prompt text per case.

## Procedure

1. Build `atm` + `atm-daemon` from the integrate head into
   `~/.atm-builds/phase-ax-<sha>/` (outside `~/Documents`), sign via
   `.just/sign_daemon_dev.py`, switch with `daemon-switch.py` (service
   `com.atm.daemon.crosshost-smoke`). Confirm `atm doctor` healthy and
   `graft_receivers` non-empty.
2. Roster: all six `atm-dev` members `--backend herdr` with live named
   Herdr agents (`herdr agent list` shows `name` for each); exactly one
   member with `agent_type == lead`.
3. Run the cases in the table, each recorded with sender, recipient,
   message id, the exact prompt text observed in the recipient pane, and
   the recipient's reply. No message bodies beyond the test strings, no
   tokens or config contents.

| Case | Command | Expected |
| --- | --- | --- |
| C1 | `atm send team-lead --requires-ack` | DeliveryAck body, targeted read action |
| C2 | `atm send cipher` (codex) | Delivery body |
| C3 | `atm queue cipher` while cipher idle | Queue body, no `<when>` |
| C4 | `atm queue cipher` while cipher working; observe after idle | Queue body delivered on idle |
| C5 | recipient `atm ack` back to fenix | Acknowledge body, unchanged shape |
| C6 | `atm send` then `atm queue` to cipher 30 ms apart | DeliveryAck then Queue; both read and acked |
| C7 | `atm teams set-nudge-template atm-dev queue --file q.xml` then C3 | override body; then `clear-nudge-template` |
| C8 | two `--task-id` sends to idle cipher; cipher acks the first | `"task_tracked": true` in the send JSON, no steer; Task reminder for the first within one tick; none for the second; first ack succeeds (Active); cipher's `atm ack` of the second exits 3 naming the first |
| C9 | cipher `atm send fenix --task-complete <first> --stdin` | second task reminded on the next tick |
| C10 | cipher `--task-complete` with an unassigned id | exit 3, no message written |
| C11 | cipher acks the second task then stays idle 3 min without completing | Task reminders ~60 s apart, none closer; stop after `--task-complete` |
| C12 | `atm teams update-member atm-dev team-lead --agent-type general-purpose`; doctor; restore `lead` | `ATM_ROSTER_NO_LEAD` warning appears, then clears |
| C13 | `atm list --tasks` and `atm list --task-events <first>` after C11 | rows and events consistent with C8–C11 |
| C14 | a task to cipher left idle and unacked for ≥ 10 min | ten `Reminded` rows; one queued message from `atm-daemon` in team-lead's inbox; doctor warns `ATM_TASK_STALLED`; clears after `--task-complete` |
| C15 | `atm send cipher --task-id t-adm --stdin` from a peer host (cross-host send via `--peer`) | delivered; `atm list --tasks` shows no row for `t-adm` |
| C15b | cipher `atm ack` of the C15 message | ack succeeds (exit 0); still no row and no event for `t-adm` |
| C16 | `atm send tmux-member --task-id t-tm --stdin` (one member temporarily on `--backend tmux`) | no steer; queue marker set; Task body delivered on idle |
| C17 | `atm send loki@hermes --task-id t-gr --stdin` (graft member, if the hermes testbed is up) | one graft dispatch with `kind: queue`; hermes-agent surfaces the Task body when idle; else recorded as not run |

4. C6 is the regression case for the stranded-7a defect. With the targeted
   read action the recipient must read both messages. If stranding
   recurs because the two prompts coalesced inside Herdr's 300 ms Enter
   window, record it as evidence for the coalescing follow-up on #1173,
   not as an AX failure.
5. C15/C15b run only if a peer host is available in the session;
   otherwise record them as "not run" with the reason.

## Deliverables

This is the authoritative deliverable checklist.

- [ ] D1 — `docs/plans/phase-ax/ax7-live-proof.md`: the case table
  filled in with message ids and pane transcripts (prompt text only),
  daemon build sha, herdr version, doctor output before and after
  showing `herdr_breaker` closed and `herdr_queue_pump.last_tick_at`
  advancing, one `herdr_queue_poll_tick` log line with non-zero
  `task_reminders`, and the `atm list --task-events` output for C11 and
  C14 (`reminded` rows). Doctor does not report reminder counts.

### Paths to delete

None.

## Acceptance criteria

1. C1–C5, C7–C14, and C16 PASS with transcripts.
4. C17 PASS or recorded as not run.
2. C6 PASS, or FAIL attributed to coalescing with the evidence attached
   and the follow-up issue referenced.
3. C15 and C15b PASS or recorded as not run.

## Required validation

quality-mgr reviews `ax7-live-proof.md` against this table before the
phase PR to `develop`; the phase-ending critical review runs on the
integrate head afterwards.

## Out of scope

Windows or Linux Herdr runs; hermes graft members; tmux members.
