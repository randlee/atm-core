---
id: HERMES-SKILL-TESTS-R1
phase: AQ
sprint: HERMES-SKILL-TESTS-R1
title: Hermes/ATM integration tests as agent skills
status: proposed
branch: feat/atm-test-skills
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feat/atm-test-skills
target: develop
pr_target: develop
integration_branch: develop
execution_track: docs+skills
parallel_with: [COLIMA-SIMPLIFY-R1, HERMES-PATCH-MODEL-R1]
stack_parent: none
one_command: "run the <skill> skill … and send the report to <agent@team>"
target_minutes: 30
dependency_relations:
  - prerequisite: HERMES-SKILL-TESTS-R1
    dependent: COLIMA-SIMPLIFY-R1
    relation: must_follow
    rationale: The testbed runs these skills; it does not define its own tests.
---

# HERMES-SKILL-TESTS-R1 — Hermes/ATM integration tests as agent skills

Rand, 2026-09-07: "The tests should be very simple. You simply tell the hermes agent to use the
smoke-test skill and generate a report." "If we need to create 3-4 skills for 3-4 hermes involved
integration tests, that is all you need to do." "All of these tests should be EXACTLY the same
tests we run outside the fixture." "This is running a simple step-by-step run-book with no
complicated ceremony." Nothing in this plan may add ceremony back.

## What a test is

- A test is one skill: `.claude/skills/<skill-name>/SKILL.md`, a numbered checklist any agent
  (Claude, Codex, Hermes) executes after one sentence: "run the `<skill>` skill … and send the
  report to `<agent@team>`".
- Every skill ends in exactly one report message in the shape of
  `.claude/skills/atm-smoke/REPORT.md`. The report names the **fixture** (`$ATM_TEST_FIXTURE` or
  hostname); that line is the only thing that differs between the local host and the colima
  testbed. Same skill, same steps, same report shape, everywhere.
- Skills are tool-agnostic: a step says the native Hermes tool (`atm_send`, `atm_read`,
  `atm_list`, `atm_ack`) and the CLI form; the agent uses what it has and says which in the report.
- A step records PASS or FAIL with the observable that decided it (message id, count, exit or
  error code, seconds). No narrative, no bodies, no chat ids, no tokens, no capability values.

## The skills (this sprint's deliverable)

| order | skill | who runs it | proves |
| --- | --- | --- | --- |
| 1 | `atm-setup-environment` | the ATM test agent | daemon answers, every expected team member is in the roster (adds missing ones), `atm doctor` has 0 errors, self round trip, cross-host peer trust to the oversight host |
| 2 | `atm-smoke` | the ATM test agent (CLI), then each Hermes agent (native) | send, list, read by id (count=1), read marks read, peek does not mutate, 5 s idle stale-connection probe, ack-required send to a partner and its ack |
| 3 | `atm-hermes-ready` | the ATM test agent | Hermes agents in roster, graft receivers registered and fresh, each answers a ping with an ack |
| 4 | `atm-nudge-roundtrip` | tester = ATM test agent, responder = Hermes agent | the hermes-atm path: ack-required send → nudge → native read by exact id → native ack → tester sees the ack and the pending-ack state clears |
| — | `atm-troubleshoot` | whoever hit a FAIL | root cause of one FAIL line from the mail DB row, `atm log`, `atm doctor`, a probe message, the gateway log |

Order 1 → 2 → 3 → 4 is the whole run-book: set up, local smoke, confirm the Hermes agents are
running, then the hermes-atm tests.

## Where the skills live

- Canonical: the colima repo `randlee/atm-hermes-testbed`, `.claude/skills/atm-*/SKILL.md`
  (+ `atm-smoke/REPORT.md`). The tests belong to the testbed and run from it, on every host and
  inside the container. atm-core carries only this plan.
- Codex agents: `.codex/skills/<name>` symlinks to the same directories, and the testbed `AGENTS.md`
  names every skill with its one sentence, so a Codex agent runs the identical file.
- Hermes agents: the image `COPY`s the same directories into `/opt/hermes/skills/`, which the fork's
  boot hook syncs into `$HERMES_HOME/skills/`; on a developer host the same files are copied to
  `~/.hermes/skills/<name>/`. Copies are byte-identical; nobody edits a copy.

## Not a black box

The fixture (local host or colima container) is observable and addressable:

- The ATM database is queried directly (read-only) for message and state rows:
  `~/.atm/db/mail.db`, tables `mail_messages` and `mail_message_states`. The exact query is in
  `atm-troubleshoot`.
- `atm send` reaches every agent in the fixture; with the container run as a cross-host peer the
  oversight agent on the host addresses them as `<agent>@<team> --host <fixture-host>` and receives
  their reports in its own inbox.
- `atm log` and `atm doctor --json` are read inside the fixture the same way as outside.
- A Hermes agent's gateway log is read for its tool calls (ids redacted).

## FAIL discipline

- A FAIL in one step never stops the run. The agent finishes every step, then reports once.
- No agent reports a FAIL without: running `atm-troubleshoot` for that step, applying the fix if it
  is within the agent's reach on the fixture (roster entry, its own gateway or tool session, a
  stale receiver registration, an environment variable, a wrong command form), re-running the step
  once, and recording `cause / fix / retest` on the step line.
- Agents never change ATM code or binaries; a cause in the product goes to the requester as-is.
- quality-mgr may reject a run because an outside agent interfered, and says so. That is different
  from a product defect; the report's cause line makes the difference visible instead of
  "FAIL, stop everything" on the first hiccup.

## The run-book, verbatim

Three agents take part: the oversight agent, the ATM test agent (CLI only), one Hermes agent. The
oversight agent sends exactly these seven messages with `atm send --stdin`, in this order, and then
waits for seven reports. `<T>` is the test agent, `<H>` the Hermes agent, `<O>` the oversight agent,
`<F>` the fixture name.

```
to <T>: run the atm-setup-environment skill on fixture <F> and send the report to <O>
to <H>: run the atm-setup-environment skill on fixture <F> and send the report to <O>
to <T>: run the atm-smoke skill against <H> on fixture <F> and send the report to <O>
to <H>: run the atm-smoke skill against <T> on fixture <F> and send the report to <O>
to <T>: run the atm-hermes-ready skill for <H> on fixture <F> and send the report to <O>
to <T>: run the atm-nudge-roundtrip skill as tester against <H> on fixture <F> and send the report to <O>
to <H>: run the atm-nudge-roundtrip skill as responder on fixture <F> and send the report to <O>
```

The two `atm-smoke` sentences go out together; the two `atm-nudge-roundtrip` sentences go out
together. Everything else is sequential: the next sentence goes out when the previous report is in.
Seven reports in, the oversight agent writes the post-mortem. Expected wall clock after the build is
installed: about 10 minutes.

What a test run does **not** have, by rule: no task ids, no j2 templates, no acknowledgement
messages, no holds or go-gates, no phases, no finding ids or triage records, no QA rounds, no
reviewer in the loop, no scripts written, no code edited, no messages between agents other than
the seven sentences, the test traffic the skills themselves send, and the seven reports. The
2026-09-07 baseline this replaces spent four hours of coordination on a 25-minute run; every item
on this list was part of that four hours.

## Budget and no code churn

- A full run-book (skills 1–4, both agents, both roles) is a 30-minute job. Hours mean something is
  wrong with the fixture or the product, and the run stops at the budget with a post-mortem; it
  does not stretch.
- Skills are markdown checklists run by hand-tools (`atm` CLI, native ATM tools, `sqlite3`,
  `atm log`). There is no Python or shell harness in this sprint, so there is nothing for an agent to
  edit between runs. A skill wording fix is a one-line PR after the run, never during it.
- The oversight agent fixes environment state (roster, gateway, receiver, path, permission) and
  re-runs a step. It does not write scripts, patch tooling, or change ATM. If a step cannot be made
  to pass by fixing environment state, the cause goes into the post-mortem and the run moves on.

## No babysitting

- A run is fully specified by the skill text plus two inputs: the partner agent and the fixture
  name (`$ATM_TEST_FIXTURE`, else `hostname`). The agent makes no other decision.
- The expected roster is whatever `atm teams` already holds unless the request lists members;
  nothing in the environment is edited between runs, and no per-run configuration file exists.
- The same sentences work on every host that runs an ATM daemon and a Hermes install, and inside
  the testbed; a new host needs nothing but the repo checkout and the Hermes skill copies.
- Whoever sends the sentences waits for the reports. There is no polling of agents, no reminders,
  no decisions mid-run; a missing report after the 30-minute budget is itself the finding.

## Roles

| role | agent | does |
| --- | --- | --- |
| orchestrator | fenix@atm-dev | sends the one-sentence run-book, collects reports, dispatches fixes to arch-ctm, merges |
| ATM test agent (`<T>`) | a CLI-only agent of the local ATM team (today: cipher) | runs skills 1–4 as tester on the host; the same role inside the testbed is the container's tester agent |
| Hermes agent under test (`<H>`) | a Hermes agent of the host's hermes team (today: skillrx); the container's Hermes agents in the testbed | runs `atm-smoke` natively, responds in `atm-nudge-roundtrip` |
| QA | quality-mgr | reads the reports against the skill text; may reject a run for interference |
| oversight (`<O>`) | one agent per run (today: fenix) | watches the whole run-book with best effort to get the code verified: when a step fails for an environment reason (roster, gateway, receiver, path, permission) it fixes that and has the step re-run, so a run ends with the code's real result, not an environment hiccup; records every intervention for the post-mortem |
| decisions | Rand | approves this plan, authorizes any build/rollout and any publish |

## Steps

1. **Skills land**: five skills, report template, `.codex/skills` links, `AGENTS.md` section and
   the image `COPY` in the testbed repo (one PR there); this plan in atm-core (PR #1304). No code.
2. **Verify outside the fixture, current install (1.5.6).** Copies at `~/.hermes/skills`. The ATM
   test agent runs 1 → 2 → 3 → 4 as tester against the Hermes agent; the Hermes agent runs 1 and 2
   natively and responds in 4.
   Seven reports to the oversight agent. Expected: the known 1.5.6 defects (#1297 stale connection after >3 s
   idle, #1298 native read by id count=0) appear as FAIL lines with cause lines. That proves the
   skills catch them. This step is "does the run-book work", not "is 1.5.6 good".
   First result (2026-09-07, oversight agent running `atm-setup-environment` by hand): the CLI rejects
   self-addressed sends (`SelfAddressedSendInvalid`), so every self-send step was replaced by a
   partner step before any agent ran the skills. That is what step 2 is for.
3. **Fix what step 2 finds in the skills** (wording, wrong flag, unreachable observable): same PR.
   Product findings go to the open issues, not to this sprint.
4. **Repeat step 2 on 1.5.7** after PR #1299 merges and Rand authorizes the local rollout. Expected:
   all PASS. Tag per the patch-bump-per-test rule.
5. **Run in the testbed, as a cross-host peer.** Peer mode is the testbed default (`run.sh`; `--no-peer`
   only for a deliberately walled run), configured from the start: the container daemon and the host daemon trust each other as
   peers, so the oversight agent on the host sends the seven sentences with
   `atm send <agent>@<team> --host <fixture-host>` and the seven reports arrive in its own inbox over
   ATM. No docker exec, no log scraping, no second channel: the fixture is one more host. The
   container's agents run the byte-identical skills synced from `/opt/hermes/skills`; the fixture
   name is the container's peer host name. When a report line says FAIL, the oversight agent may
   still query the container's `mail.db` and gateway logs as the plan's non-black-box channels;
   normal runs never need them.
6. **Verdict.** All reports PASS on both fixtures for the same ATM version = the integration test
   passes for that version. Publication remains Rand's decision.
7. **Post-mortem report.** After every complete run (each fixture, each version) the oversight
   agent writes one report, `docs/plans/phase-aq/reports/hermes-skill-tests-<version>-<fixture>.md`:
   the seven report summaries (skill, agent, PASS/FAIL, elapsed), every FAIL with its cause line,
   every oversight intervention (what broke, why, what was changed, whether the step then passed),
   product defects found (issue numbers), and recommended changes in three lists: to the skills,
   to the fixture/testbed, to ATM. The report is what Rand reads; nothing else is.

## Exact targets

- atm-hermes-testbed: `.claude/skills/atm-setup-environment/SKILL.md`, `atm-smoke/SKILL.md`,
  `atm-smoke/REPORT.md`, `atm-hermes-ready/SKILL.md`, `atm-nudge-roundtrip/SKILL.md`,
  `atm-troubleshoot/SKILL.md`; `.codex/skills/atm-*` symlinks; `AGENTS.md`; one Dockerfile `COPY`
- this document; `docs/project-plan.md` entry
- after each run: `docs/plans/phase-aq/reports/hermes-skill-tests-<version>-<fixture>.md` (post-mortem)
- No source, harness, runtime, requirement, ADR or testbed file changes in this PR.

## Acceptance

- One sentence per skill is enough; no agent asked a follow-up question to run one.
- Each skill completes in under 5 minutes; the whole run-book in under 30.
- Every FAIL line carries cause / fix / retest.
- The same five files, unchanged, run on every developer host that has an ATM daemon and a
  Hermes install, and inside the testbed image. No host name appears in any skill or report
  template; the fixture name comes from `$ATM_TEST_FIXTURE` or `hostname` at run time.
- Nothing was added that is not used by a step in a skill.
- Every run ends with the code verified or with a post-mortem naming the product defect that
  prevented it; an environment hiccup never ends a run.
- Each run has its post-mortem with recommended changes before the next version is tested.
