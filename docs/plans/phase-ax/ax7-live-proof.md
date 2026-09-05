---
phase: AX
sprint: AX.7
title: AX.7 live Herdr dogfood evidence
status: in-progress
operator: fenix
branch: feature/ax7-herdr-dogfood-evidence
stacked_on: feature/ax6-lead-notification-doctor (PR #1204)
daemon_build_sha: pending
herdr_version: pending
---

# AX.7 — Live Herdr dogfood evidence (proof record)

Evidence record for `docs/plans/phase-ax/sprint-AX.7-herdr-dogfood-evidence.md`.
Every row is filled in from a real run; rows not yet run say `pending`, rows
that could not run say `not run` with the reason. Prompt text only; no message
bodies beyond the test strings, no tokens, no config contents.

## Build and daemon state

| Item | Value |
| --- | --- |
| integrate/phase-ax head built | pending |
| build path | `~/.atm-builds/phase-ax-<sha>/` (pending) |
| signing | pending |
| daemon service switched | pending |
| `atm doctor` before (healthy, graft_receivers non-empty) | pending |
| `atm doctor` after (`herdr_breaker` closed, `herdr_queue_pump.last_tick_at` advancing) | pending |
| `herdr_queue_poll_tick` line with non-zero `task_reminders` | pending |

## Roster

| Member | backend | Herdr agent name | agent_type |
| --- | --- | --- | --- |
| team-lead | pending | pending | lead |
| arch-ctm | pending | pending | pending |
| quality-mgr | pending | pending | pending |
| cipher | pending | pending | pending |
| fenix | pending | pending | pending |
| publisher | pending | pending | pending |

## Cases

| Case | Sender | Recipient | Message id(s) | Prompt text observed | Recipient reply | Result |
| --- | --- | --- | --- | --- | --- | --- |
| C1 | | | | | | pending |
| C2 | | | | | | pending |
| C3 | | | | | | pending |
| C4 | | | | | | pending |
| C5 | | | | | | pending |
| C6 | | | | | | pending |
| C7 | | | | | | pending |
| C8 | | | | | | pending |
| C9 | | | | | | pending |
| C10 | | | | | | pending |
| C11 | | | | | | pending |
| C12 | | | | | | pending |
| C13 | | | | | | pending |
| C14 | | | | | | pending |
| C15 | | | | | | pending |
| C15b | | | | | | pending |
| C16 | | | | | | pending |
| C17 | | | | | | pending |
| C18 | | | | | | pending |

## Task events

### C11 `atm list --task-events`

pending

### C14 `atm list --task-events`

pending

## Acceptance criteria status

1. C1–C5, C7–C14, C16, C18 PASS with transcripts: pending
2. C6 PASS or FAIL attributed to coalescing (#1173): pending
3. C15 / C15b PASS or not run: pending
4. C17 PASS or not run: pending
