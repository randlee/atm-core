---
status: brief
branch: plan/colima-agentic-harness
pr_target: develop
owner: fenix
requested_by: Rand, 2026-09-23
---

# Colima integration harness redesign: agents talk, humans watch

## Ruling (verbatim, Rand 2026-09-23 23:05Z)

> don't let the colima integration tests spiral into a endless vortex of not
> knowing what is going on.
>
> this entire process is far too opaic and needs a redesign.
>
> you should be able to talk to the agents inside the harness, with adequate
> context, they could easily trougleshoot any issues.
>
> using 5 minute timers and scripts is rediculiously primitive.

## What happened on 2026-09-23 (the case for the redesign)

Release validation of `prerelease/v1.6.1` took three colima attempts and
three hours without a verdict. Every ATM check passed every time (daemon,
doctor, roster, self and cross-host round trips, task-start and assignment
prompt steps). The time went to the harness:

1. **Silent dependency.** Hermes v2026.9.21-atm became multiplex-only and
   stopped reading `ANTHROPIC_API_KEY` from the process environment. Bringup
   printed `gateway up` and `doctor healthy`; the gateway logged `Model
   resolution failed` on every turn; the harness reported the result as four
   silent 30 s phases and `skills reported 1/7`. Root cause took reading the
   gateway log by hand. Fixed in atm-hermes-testbed #16 (key in the profile
   env, one bringup probe that prints the gateway's own line and stops).
2. **Timers instead of conversation.** `test.sh` sends a sentence, polls an
   inbox every 10 s with shell parsing, and gives up at a fixed deadline. A
   30 s deadline (set by the running agent) failed every phase by
   construction; the default 600 s turns one missing report into a ten-minute
   wait per phase. Nobody asked the tester or hermes why they were silent,
   although both answer `atm send` inside the container.
3. **Nothing observable while it runs.** `run_colima.py` captures the whole
   harness stdout in memory and writes it when the step ends. During the run
   the only signals were `docker exec` inspections of inboxes and logs.
4. **Runs collide.** Every run uses the container name `hermes-testbed`. A
   second `run.sh` replaced the fixture under a running harness; two
   harnesses then polled one oversight inbox, and the saved reports mixed two
   fixtures. Neither run could be trusted.

## Principles

- **Observable first.** One run writes one transcript file, appended live and
  readable with `tail -f`: every sentence sent, every START/WAIT/REPORT
  received (verbatim), every probe result, every gateway or agent log line
  quoted on a failure. The same file becomes the evidence, byte for byte.
- **Conversation, not timers.** A conductor agent runs the sequence. It sends
  the sentence, watches for the START line, and when an agent is quiet it
  asks the agent what it is doing and records the answer. A budget still
  exists (the run must end) but it is a last resort, not the mechanism.
- **Fail fast at bringup.** Every dependency gets a probe that exercises the
  real path and prints the product's own error verbatim: daemon, herdr,
  Hermes provider (one inbound turn), Claude Code tester (one nudge), peer
  link. A failed probe ends the run before any phase starts.
- **One fixture per run, no sharing.** Container name carries the run stamp
  (`hermes-testbed-<stamp>`); the testbed refuses to start while another
  fixture of the same family is running; the harness only ever talks to the
  container it started.
- **Visibility, not code.** No validators or classifiers of agent behaviour.
  The verdict is computed once, from the structured `ATM TEST REPORT`
  messages the skills already emit, by one parser with tests.

## Architecture

```
run_colima.py (driver, unchanged contract: one envelope, one verdict, panels)
  └── testbed run.sh --stamp <stamp>          fixture bringup + probes, own container name
  └── conductor (agent)                        the sequence as a conversation
        ├── sends sentences to tester / hermes over ATM (as stub-alpha)
        ├── reads oversight's inbox; every message → transcript, verbatim
        ├── on silence: asks the quiet agent "what are you doing, what blocks you"
        │   and records the reply; escalates once, then marks the slot FAILED
        ├── on FAILED: quotes the last 20 gateway/agent log lines into the transcript
        └── writes result.json (slots, verdicts, timings) + transcript.md
  └── prompt steps (AT9, AT10, AT11) stream their harness output live
```

The conductor is a skill (`colima-conduct`) executed by a background agent on
the host (haiku is enough; it reads and relays, it does not judge). Its
inputs are the container name, the expected roster and the seven slots; its
tools are `docker exec … atm send|read|list` and the transcript file. It
never edits evidence; it appends.

## Layers (append-only stack above develop, one PR each)

1. **Observe + isolate** (small, lands first):
   - `run_colima.py` streams every child's stdout/stderr to the step's log
     file as it happens (tee), so `tail -f` shows the run.
   - testbed: `run.sh --stamp` names the container per run and refuses to
     start when a `hermes-testbed*` container is running; `test.sh` and
     `bringup.sh` take the name from the environment.
   - testbed: bringup probes for the Claude Code tester and the peer link
     alongside the Hermes provider probe from #16 (each prints the product's
     own error and exits).
2. **Conductor** replaces the phase loop in `test.sh`:
   - the seven-sentence sequence, START/WAIT/REPORT handling and the verdict
     move into the `colima-conduct` skill; `test.sh` keeps resolve inputs,
     build, bringup, and verdict rendering from `result.json`.
   - silence handling is a question to the agent, recorded verbatim, then one
     re-nudge, then FAILED with log excerpts.
   - the transcript is the first artifact in the evidence dir and the first
     panel on the report page.

## Acceptance criteria

- A run can be followed live from one file, and that file is the evidence.
- A missing provider, a dead tester or a broken peer link stops the run at
  bringup with the product's own error quoted.
- Two runs started within a minute of each other never share a container or
  an inbox; the second refuses to start and says why.
- An agent that goes quiet is asked and its answer appears in the transcript
  before any FAILED verdict.
- No shell-side deadline knobs (`PHASE_DEADLINE` removed); one run budget in
  the driver, documented.
- The report page family, envelope and index row stay as BB.8 defined them.
