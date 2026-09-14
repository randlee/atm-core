---
procedure: colima-prompt-handoffs
family: integration
runner: scripts/integration/run_colima.py
evidence: site/reports/integration/colima/<run>/steps/NN-prompt-handoffs/step.json
revisions:
  - rev: 10f80d6ec985eff0c1594e53c11b056d54cab35a
    date: 2026-09-13
    note: "refactor(bb8): drive colima checks through fixture prompts"
  - rev: 1f3334062b30f1b9dc1fd2ca5aa00344dac04551
    date: 2026-09-13
    note: "feat(bb8): run prompt handoff checks in the shared integration fixture"
  - rev: a2491a6c69e99e93bc971be69a51f5c7dd24f96a
    date: 2026-09-12
    note: "test(smoke): exercise observed interval guard"
  - rev: 2cc19c18da82188ce964889a3b4c1ef32c1c9fca
    date: 2026-09-12
    note: "test(smoke): cover observed reminder deadline"
  - rev: 6d18058c0e21501a6f8329b980a9756b02ea9e11
    date: 2026-09-12
    note: "fix(smoke): bound disabled reminder wait by observed interval (BB6-026)"
  - rev: 054fd8b268e275498e9a0315eb24c4564a19a4cd
    date: 2026-09-12
    note: "test(bb6): match handoffs by durable identity"
  - rev: 98e410369120438bb91089ec14ba38fcbdedf93d
    date: 2026-09-12
    note: "test(bb6): bound bare mailbox evidence reads"
  - rev: f5cc8d384247bd28577a73e4c002d83442d0d6d2
    date: 2026-09-12
    note: "test(bb6): reconcile all prompt recipient surfaces"
  - rev: 9429a9971243182203b5e6fb050a5c2ca2f6e84b
    date: 2026-09-12
    note: "test(bb6): add prompt handoff colima runner"
---

## What this test proves
Inside the shared colima testbed, a real agent executes prompt AT11 and observes only public ATM CLI JSON. Task events expose queued, ready, reminder, and started for the prompted task but queued only for later tasks; disabling reminders removes the reminder event and produces the documented doctor finding.

## Flow
```mermaid
flowchart LR
  setup[Reuse the live testbed fixture]
  prompt[Run AT11 prompt-handoffs agent prompt]
  case1[1: prompted task events only]
  case2[2: disabled reminder doctor finding]
  verdict[Aggregate PASS/FAIL]
  evidence[Write step.json]
  setup --> prompt
  prompt --> case1
  case1 --> case2
  case2 --> verdict
  verdict --> evidence
  note[runner revision 10f80d6e]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Tell the fixture agent to assign three tasks, wait for a reminder, and start the prompted task | `atm task events --json` shows queued, ready, reminder, started for task 1 and queued only for tasks 2–3 | `prompt-AT11.json` and `step.json` cases |
| 2 | Tell the agent to disable task reminders and assign another task | task events omit reminder and `atm doctor --json` reports `disabled_task_nudge_template_override` | `prompt-AT11.json` and `step.json` cases |

## Evidence layout
This procedure is one step of a colima integration run. The agent's unedited `prompt-report-1` JSON is retained beside `step.json`; `scripts/integration/render_colima.py` renders the panel and aggregate. Historical payloads remain byte-identical and render through the same path.

## Changes
The revision sections below list every runner revision that produced committed evidence, newest first. A run whose source revision is not an ancestor of any listed revision links to the revision in effect on its run date and is marked inferred.

## Revision 10f80d6e (2026-09-13)
The host-side handoff simulator and its storage inspection were deleted. A real fixture agent executes AT11 and reports only public `atm task events` and `atm doctor` observations.

```mermaid
flowchart LR
  setup[Reuse the live testbed fixture]
  prompt[Run AT11 prompt-handoffs agent prompt]
  case1[1: prompted task events only]
  case2[2: disabled reminder doctor finding]
  verdict[Aggregate PASS/FAIL]
  evidence[Write prompt report and step.json]
  setup --> prompt
  prompt --> case1
  case1 --> case2
  case2 --> verdict
  verdict --> evidence
  note[runner revision 10f80d6e]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | Tell the fixture agent to assign three tasks, wait for a reminder, and start the prompted task | `atm task events --json` shows queued, ready, reminder, started for task 1 and queued only for tasks 2–3 | `prompt-AT11.json` and `step.json` cases |
| 2 | Tell the agent to disable task reminders and assign another task | task events omit reminder and `atm doctor --json` reports `disabled_task_nudge_template_override` | `prompt-AT11.json` and `step.json` cases |

## Revision 1f333406 (2026-09-13)
The shared-fixture revision records the existing `prompt_handoffs` row count before this step, then reconciles only the rows added by the step. This preserves the exact terminal-line identity assertion without requiring a fresh container after earlier integration steps.

```mermaid
flowchart LR
  setup[Read shared fixture handoff baseline]
  case1[1: task_events_shows_ready_reminders_and_st]
  case2[2: disabled_task_reminder_override_yields_n]
  verdict[Aggregate PASS/FAIL]
  evidence[Write step.json]
  setup --> case1
  case1 --> case2
  case2 --> verdict
  verdict --> evidence
  note[runner revision 1f333406]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | `task_events_shows_ready_reminders_and_started_for_prompted_task_only`: record the shared-table baseline, assign three tasks, wait for reminders, start and close the prompted one | task events and prompt-handoff rows added after the baseline agree row for row, for the prompted task only; recorded PASS or FAIL at revision `1f333406` | `step.json` cases |
| 2 | `disabled_task_reminder_override_yields_no_handoff_and_doctor_finding`: disable the task reminder and assign | no handoff row is added and `atm doctor` reports the finding; recorded PASS or FAIL at revision `1f333406` | `step.json` cases |

## Revision a2491a6c (2026-09-12)
The runner change `test(smoke): exercise observed interval guard` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Start testbed container for colima-prompt-handoffs]
  case1[1: task_events_shows_ready_reminders_and_st]
  case2[2: disabled_task_reminder_override_yields_n]
  verdict[Aggregate PASS/FAIL]
  evidence[Write step.json]
  setup --> case1
  case1 --> case2
  case2 --> verdict
  verdict --> evidence
  note[runner revision a2491a6c]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | `task_events_shows_ready_reminders_and_started_for_prompted_task_only`: assign three tasks, wait for reminders, start and close the prompted one | task events and the prompt_handoffs table agree row for row, for the prompted task only; recorded PASS or FAIL at revision `a2491a6c` | `step.json` cases |
| 2 | `disabled_task_reminder_override_yields_no_handoff_and_doctor_finding`: disable the task reminder and assign | no handoff row is written and `atm doctor` reports the finding; recorded PASS or FAIL at revision `a2491a6c` | `step.json` cases |

## Revision 2cc19c18 (2026-09-12)
The runner change `test(smoke): cover observed reminder deadline` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Start testbed container for colima-prompt-handoffs]
  case1[1: task_events_shows_ready_reminders_and_st]
  case2[2: disabled_task_reminder_override_yields_n]
  verdict[Aggregate PASS/FAIL]
  evidence[Write step.json]
  setup --> case1
  case1 --> case2
  case2 --> verdict
  verdict --> evidence
  note[runner revision 2cc19c18]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | `task_events_shows_ready_reminders_and_started_for_prompted_task_only`: assign three tasks, wait for reminders, start and close the prompted one | task events and the prompt_handoffs table agree row for row, for the prompted task only; recorded PASS or FAIL at revision `2cc19c18` | `step.json` cases |
| 2 | `disabled_task_reminder_override_yields_no_handoff_and_doctor_finding`: disable the task reminder and assign | no handoff row is written and `atm doctor` reports the finding; recorded PASS or FAIL at revision `2cc19c18` | `step.json` cases |

## Revision 6d18058c (2026-09-12)
The runner change `fix(smoke): bound disabled reminder wait by observed interval (BB6-026)` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Start testbed container for colima-prompt-handoffs]
  case1[1: task_events_shows_ready_reminders_and_st]
  case2[2: disabled_task_reminder_override_yields_n]
  verdict[Aggregate PASS/FAIL]
  evidence[Write step.json]
  setup --> case1
  case1 --> case2
  case2 --> verdict
  verdict --> evidence
  note[runner revision 6d18058c]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | `task_events_shows_ready_reminders_and_started_for_prompted_task_only`: assign three tasks, wait for reminders, start and close the prompted one | task events and the prompt_handoffs table agree row for row, for the prompted task only; recorded PASS or FAIL at revision `6d18058c` | `step.json` cases |
| 2 | `disabled_task_reminder_override_yields_no_handoff_and_doctor_finding`: disable the task reminder and assign | no handoff row is written and `atm doctor` reports the finding; recorded PASS or FAIL at revision `6d18058c` | `step.json` cases |

## Revision 054fd8b2 (2026-09-12)
The runner change `test(bb6): match handoffs by durable identity` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Start testbed container for colima-prompt-handoffs]
  case1[1: task_events_shows_ready_reminders_and_st]
  case2[2: disabled_task_reminder_override_yields_n]
  verdict[Aggregate PASS/FAIL]
  evidence[Write step.json]
  setup --> case1
  case1 --> case2
  case2 --> verdict
  verdict --> evidence
  note[runner revision 054fd8b2]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | `task_events_shows_ready_reminders_and_started_for_prompted_task_only`: assign three tasks, wait for reminders, start and close the prompted one | task events and the prompt_handoffs table agree row for row, for the prompted task only; recorded PASS or FAIL at revision `054fd8b2` | `step.json` cases |
| 2 | `disabled_task_reminder_override_yields_no_handoff_and_doctor_finding`: disable the task reminder and assign | no handoff row is written and `atm doctor` reports the finding; recorded PASS or FAIL at revision `054fd8b2` | `step.json` cases |

## Revision 98e41036 (2026-09-12)
The runner change `test(bb6): bound bare mailbox evidence reads` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Start testbed container for colima-prompt-handoffs]
  case1[1: task_events_shows_ready_reminders_and_st]
  case2[2: disabled_task_reminder_override_yields_n]
  verdict[Aggregate PASS/FAIL]
  evidence[Write step.json]
  setup --> case1
  case1 --> case2
  case2 --> verdict
  verdict --> evidence
  note[runner revision 98e41036]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | `task_events_shows_ready_reminders_and_started_for_prompted_task_only`: assign three tasks, wait for reminders, start and close the prompted one | task events and the prompt_handoffs table agree row for row, for the prompted task only; recorded PASS or FAIL at revision `98e41036` | `step.json` cases |
| 2 | `disabled_task_reminder_override_yields_no_handoff_and_doctor_finding`: disable the task reminder and assign | no handoff row is written and `atm doctor` reports the finding; recorded PASS or FAIL at revision `98e41036` | `step.json` cases |

## Revision f5cc8d38 (2026-09-12)
The runner change `test(bb6): reconcile all prompt recipient surfaces` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Start testbed container for colima-prompt-handoffs]
  case1[1: task_events_shows_ready_reminders_and_st]
  case2[2: disabled_task_reminder_override_yields_n]
  verdict[Aggregate PASS/FAIL]
  evidence[Write step.json]
  setup --> case1
  case1 --> case2
  case2 --> verdict
  verdict --> evidence
  note[runner revision f5cc8d38]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | `task_events_shows_ready_reminders_and_started_for_prompted_task_only`: assign three tasks, wait for reminders, start and close the prompted one | task events and the prompt_handoffs table agree row for row, for the prompted task only; recorded PASS or FAIL at revision `f5cc8d38` | `step.json` cases |
| 2 | `disabled_task_reminder_override_yields_no_handoff_and_doctor_finding`: disable the task reminder and assign | no handoff row is written and `atm doctor` reports the finding; recorded PASS or FAIL at revision `f5cc8d38` | `step.json` cases |

## Revision 9429a997 (2026-09-12)
The runner change `test(bb6): add prompt handoff colima runner` is the source for this revision's procedure order.

```mermaid
flowchart LR
  setup[Start testbed container for colima-prompt-handoffs]
  case1[1: task_events_shows_ready_reminders_and_st]
  case2[2: disabled_task_reminder_override_yields_n]
  verdict[Aggregate PASS/FAIL]
  evidence[Write step.json]
  setup --> case1
  case1 --> case2
  case2 --> verdict
  verdict --> evidence
  note[runner revision 9429a997]:::revision
```

## Steps
| step | action | observable | evidence |
| --- | --- | --- | --- |
| 1 | `task_events_shows_ready_reminders_and_started_for_prompted_task_only`: assign three tasks, wait for reminders, start and close the prompted one | task events and the prompt_handoffs table agree row for row, for the prompted task only; recorded PASS or FAIL at revision `9429a997` | `step.json` cases |
| 2 | `disabled_task_reminder_override_yields_no_handoff_and_doctor_finding`: disable the task reminder and assign | no handoff row is written and `atm doctor` reports the finding; recorded PASS or FAIL at revision `9429a997` | `step.json` cases |
