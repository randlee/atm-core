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
| 1 | `atm-setup-environment` | the ATM test agent | daemon answers, every expected team member is in the roster (adds missing ones), `atm doctor` has 0 errors, self round trip |
| 2 | `atm-smoke` | the ATM test agent (CLI), then each Hermes agent (native) | send, list, read by id (count=1), read marks read, peek does not mutate, 5 s idle stale-connection probe, ack-required send to a partner and its ack |
| 3 | `atm-hermes-ready` | the ATM test agent | Hermes agents in roster, graft receivers registered and fresh, each answers a ping with an ack |
| 4 | `atm-nudge-roundtrip` | tester = ATM test agent, responder = Hermes agent | the hermes-atm path: ack-required send → nudge → native read by exact id → native ack → tester sees the ack and the pending-ack state clears |
| — | `atm-troubleshoot` | whoever hit a FAIL | root cause of one FAIL line from the mail DB row, `atm log`, `atm doctor`, a probe message, the gateway log |

Order 1 → 2 → 3 → 4 is the whole run-book: set up, local smoke, confirm the Hermes agents are
running, then the hermes-atm tests.

## Where the skills live

- Canonical: this repo, `.claude/skills/atm-*/SKILL.md` (+ `atm-smoke/REPORT.md`).
- Codex agents: `.codex/skills/<name>` symlinks to the same directories, and `AGENTS.md` names
  every skill, so a Codex agent runs the identical file.
- Hermes agents: a copy at the Hermes home, `~/.hermes/skills/<name>/SKILL.md` on rand-m5, and
  `/root/.hermes/skills/<name>/SKILL.md` baked into the atm-hermes-testbed image. Copies are
  byte-identical to the repo files; nobody edits a copy.

## Not a black box

The fixture (local host or colima container) is observable and addressable:

- The ATM database is queried directly (read-only) for message and state rows:
  `~/.atm/db/mail.db`, tables `mail_messages` and `mail_message_states`. The exact query is in
  `atm-troubleshoot`.
- `atm send` reaches every agent in the fixture; `atm list`/`atm read` show what came back.
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

## Roles

| role | agent | does |
| --- | --- | --- |
| orchestrator | fenix@atm-dev | sends the one-sentence run-book, collects reports, dispatches fixes to arch-ctm, merges |
| ATM test agent | cipher@atm-dev (CLI only) | runs skills 1–4 as tester on rand-m5; the same role inside the testbed is the container's tester agent |
| Hermes agent under test | skillrx@hermes on rand-m5; the container's Hermes agents in the testbed | runs `atm-smoke` natively, responds in `atm-nudge-roundtrip` |
| testbed maintainer | loki@hermes | bakes the byte-identical skill files into the image; changes no test content |
| QA | quality-mgr | reads the reports against the skill text; may reject a run for interference |
| decisions | Rand | approves this plan, authorizes any build/rollout and any publish |

## Steps

1. **Skills land** (this PR, #1304): five skills, report template, `.codex/skills` links,
   `AGENTS.md` section, this plan. Docs-only; no code.
2. **Verify outside the fixture, current install (1.5.6).** Copies at `~/.hermes/skills`. cipher
   runs 1 → 2 → 3 → 4 as tester against skillrx; skillrx runs 1 and 2 natively and responds in 4.
   Eight reports to fenix. Expected: the known 1.5.6 defects (#1297 stale connection after >3 s
   idle, #1298 native read by id count=0) appear as FAIL lines with cause lines. That proves the
   skills catch them. This step is "does the run-book work", not "is 1.5.6 good".
3. **Fix what step 2 finds in the skills** (wording, wrong flag, unreachable observable): same PR.
   Product findings go to the open issues, not to this sprint.
4. **Repeat step 2 on 1.5.7** after PR #1299 merges and Rand authorizes the local rollout. Expected:
   all PASS. Tag per the patch-bump-per-test rule.
5. **Deploy to the testbed.** loki bakes the same files into the image (`/root/.hermes/skills`),
   README states the four sentences. The run inside colima is the same run-book from step 2 with
   `ATM_TEST_FIXTURE=colima-<image digest>`; fenix (or any coordinator) sends the sentences to the
   container's agents with `atm send`, reads the reports, and can query the container's `mail.db`
   and gateway logs when a line says FAIL.
6. **Verdict.** All reports PASS on both fixtures for the same ATM version = the integration test
   passes for that version. Publication remains Rand's decision.

## Exact targets

- `.claude/skills/atm-setup-environment/SKILL.md`, `atm-smoke/SKILL.md`, `atm-smoke/REPORT.md`,
  `atm-hermes-ready/SKILL.md`, `atm-nudge-roundtrip/SKILL.md`, `atm-troubleshoot/SKILL.md`
- `.codex/skills/atm-*` symlinks; `AGENTS.md` "ATM Integration Test Skills" section
- this document; `docs/project-plan.md` entry
- No source, harness, runtime, requirement, ADR or testbed file changes in this PR.

## Acceptance

- One sentence per skill is enough; no agent asked a follow-up question to run one.
- Each skill completes in under 5 minutes; the whole run-book in under 30.
- Every FAIL line carries cause / fix / retest.
- The same five files, unchanged, run on rand-m5 and inside the testbed image.
- Nothing was added that is not used by a step in a skill.
