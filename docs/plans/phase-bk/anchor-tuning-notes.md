# Phase bk plan gate: anchor runs and tuning notes

Date 2026-10-18 (runs cross midnight UTC into 2026-10-19). Worktree
`plan/phase-bk` on PR #1877 (base `develop`). Anchors
`docs/plans/phase-bk/anchors.json`, 31 records: ANC-001..022 from Rand's
verbatim rulings on atm-core #1873 and nine REQ-style anchors (REQ-BK-D1,
REQ-BK-D3, REQ-BK-D4A, REQ-BK-D4B, REQ-BK-ERRORS, REQ-BK-LOG,
REQ-BK-HOOK-CLI, REQ-BK-SCHEMA, REQ-BK-TESTS) from `design.md` decisions;
`validate` clean, `lint` 0 warnings. Jev model answered `jev-1.13.0`, plan
gate, threshold 0.6, `--allow-remote` per Rand's session opt-in
(`PLAN_ANCHOR_ALLOW_REMOTE_JUDGE=1`). Items are the four hardened sprint
documents discovered with `plan_source.py discover-markdown
docs/plans/phase-bk/phase-bk-plan.md` (descriptors under
`anchor-runs/items/`, each judged as its complete normalized text; no bead
import, no `compose-item`).

## Fixture check, all 31 anchors with the default questions

`check-fixtures --gate plan --id <id>` for every anchor (runs under
`anchor-runs/fixtures/plan-20261018T2339*`). 28 pass both fixtures. Three
mismatches:

| Anchor | Fixture | Verdict | relevant | addressed | contradicted | Run |
| --- | --- | --- | --- | --- | --- | --- |
| ANC-001 | fail:1 | underspecified (expected not_addressed or contradicted) | yes 0.53 | no 0.92 | yes 0.46 | `anchor-runs/fixtures/plan-20261018T233919Z-fixture_ANC-001_fail_1.json` |
| ANC-003 | pass:1 | underspecified (expected satisfied) | yes 1.0 | yes 0.62 | no 0.61 | `anchor-runs/fixtures/plan-20261018T233920Z-fixture_ANC-003_pass_1.json` |
| ANC-005 | fail:1 | underspecified (expected not_addressed or contradicted) | yes 0.95 | no 0.41 | yes 0.59 | `anchor-runs/fixtures/plan-20261018T233921Z-fixture_ANC-005_fail_1.json` |

Pattern: the `contradicted` role answers below threshold on a one-sentence
fixture that states the opposite ordering (ANC-001: storage write first,
hook notified after; ANC-005: success reported without performing the
assignment), and `addressed` is unsure on a positive fixture that names the
request fields (ANC-003). This is the question-defect class
`question-tuning.md` names, so the three anchors were tuned.

## Question tuning (plan-anchoring step B2), 2026-10-18, fenix

One `anchor-tuning` (opus) run per anchor from the tune-request template,
INTENT and the ANC packet, never a desired verdict, with the agent registry
of PR #1876 (`anchor-tuning` resolves to `.claude/agents/anchor-tuning.md`
sha256 `c348a762…`, version 0.6.1). The tuner edited no workspace file;
every anchor write was made by the main session. Each result was checked
with `anchor_runner_contract.py validate-fence --kind result` (one fence)
and `anchor_tuning_contract.py result` (valid), attested with
`build_attestation` (`anchor-runs/tuning/<ANC>/attestation.json`), and the
sealed `questions` object written as `question_overrides.plan` on the
anchor record, version unchanged. Evidence per anchor under
`anchor-runs/tuning/<ANC>/`: `result.json` (byte copy of the agent's
result, `cmp` equal), `result.md` (the same result inside the fence the
contract validator takes), `tuning-history.json` (canonical JSON whose
sha256 equals the result's `tuning_history_after_sha256`).

Testability rubric: no rubric seal record exists in this repository; the
rubric shipped with the skill was used unchanged, as phases bi and bj did.

| Anchor | Selected set | Sealed digest | Tournament score (frozen oracle, two byte-identical repeats) | History sha256 | Tuner wall time |
| --- | --- | --- | --- | --- | --- |
| ANC-001 | bundle `B4` = relevant R1, addressed A3, contradicted C3 (110 Jev requests, no retries; default set mislabelled both exception probes and false-passed `P-EXCEPTION-NOHOOK`) | `5f2f63745b085c17` | 33/33 role cells, 11/11 verdicts, 11/11 stable, min confidence 0.37 (one relevance cell), 32/33 at or above 0.8 | `e63d41cc0c01f5f2` | 545 s |
| ANC-003 | `relevant.c2` + `addressed.c3` + `contradicted.c1` (12 candidates over 11 frozen cases; default `addressed` answered unclear on a topical-only item and yes on an item giving only the caller's identity) | `4bd5eeb0ed3b102a` | 66/66 role cells, 22/22 verdicts, 11/11 stable, min confidence 0.89 | `8e29d5b8b2aef40e` | 635 s |
| ANC-005 | bundle `M1` = relevant R1, addressed A1, contradicted K3 (4 bundles over 14 frozen cases; default `B0` answered unclear on the topical-only probe) | `3c990e538391b09c` | 84/84 role cells, 28/28 verdicts, 14/14 stable, min confidence 0.85 | `bede7ab540e2f007` | 619 s |

All three results: `accepted: true`, `production_semantics_unchanged:
true`, zero regressions. Re-check with the overrides in place
(`anchor-runs/fixtures/plan-20261018T2359*` and `plan-20261019T0000*`, two
rounds, both green): ANC-001 fail:1 contradicted (addressed no 1.0,
contradicted yes 1.0), pass:1 satisfied (addressed yes 0.88 to 0.92);
ANC-003 pass:1 satisfied (addressed yes 0.91 to 0.92, contradicted no 0.97
to 0.98); ANC-005 fail:1 contradicted (addressed no 0.97 to 0.98,
contradicted yes 0.97), pass:1 satisfied (addressed yes 0.97 to 1.0).

## Anchor statement corrections

None. No anchor statement, scope or fixture was changed during this run;
`intent.md` and `design.md` were amended for Rand's round-3 ruling (no sha
header; `atm doctor` calls `hook.doctor()`) before the anchors were locked.

## Anchors added and checks updated after Rand's round 4 (2026-10-19, fenix)

Source: atm-core #1873 comment 6257077671 (Rand, 2026-10-18), relayed on
PR #1877. Three anchors added with `jev_judge.py add --status locked`,
each quoting Rand verbatim, with pass/fail fixtures:

- ANC-023 "Mandatory stop condition on every cron schedule" — quote:
  "there should probably be a 'stop condition' which was mandetory when
  it is created which would generally be a time, but a function/hook
  would also be useful."
- ANC-024 "Stop hook ends the schedule" — quote: "a function/hook would
  also be useful. for atm-bd cron, phase bead closing would be condition
  for stopping."
- ANC-025 "Until time ends the schedule" — quote: "which would generally
  be a time".

Deterministic-check and fixture corrections (statements and quotes
unchanged):

- ANC-014: the check now names the router's in-memory snapshot (design
  D-8, ruthless-boundary-qa F006 option B) instead of "no store read" on
  the request path; the pass fixture says the same. The statement
  ("Missing hook = today's behavior, no error") stands.
- ANC-017: the check now pins the args to the stdin `request.args` and the
  module argv to `[module_path, "cron.tick"]` (ADR-069 D3, F011); the
  earlier check let the args travel on argv.

## Sprint cut changes before the second gate round

- The cron worker moved from `atm-daemon-bootstrap` into the new
  `atm-cron` crate behind the sealed `CronScheduler` trait (Rand, PR #1877:
  "I would expect atm cron to be in it's own crate w/ simple trait exposing
  it's features."; F001).
- The single contract sprint grew to 31.8 KB once the three sealed traits,
  the doubles, the stop condition and the protocol were written at code
  level; `plan_source.py discover-markdown` refused it (`PLAN.TOO_LARGE`,
  24 000-byte cap). It is cut by crate into bk-1 (`atm-storage`,
  `atm-storage-rusqlite`, 15.8 KB) and bk-2 (`atm-core`, `atm-error`,
  23.6 KB), sequential because the core traits take the storage row types.
  The later sprints are renumbered bk-3 (runner), bk-4 (`atm-cron`), bk-5
  (CLI, doctor, composition, docs). Gate runs dated before
  `plan-20261019T0049` use the old numbering (bk-1 = old contract sprint,
  bk-2 = runner, bk-3 = cron worker).
- bk-3's first judge request was 24 520 bytes with ANC-001 and its
  questions (item 23 986 bytes raw, 20 603 chars after code reduction).
  Cut: the anchor enumeration in the header, five `rg` command tails in
  acceptance criteria that restated the criterion, two parentheticals that
  restated deliverable text, one status-note clause. No deliverable,
  signature, test name or criterion was removed. Item after cuts: 23 325
  bytes.
- Exclusions were re-recorded for the new numbering (old bk-1 contract
  exclusions carried to bk-2, old bk-3 to bk-4) and recorded for bk-1
  (29), bk-3 (9 more) and bk-5 (24); every exclusion names the sprint
  that carries the anchor.

## Round 5: RBQA-F002 ruled, no NEEDS RULING item remains

Rand (#1873 comment 6257361348, 2026-10-18, verbatim): "yes, we need
storage for cron". Applied: ADR-069 D5 is the one place that amends
ADR-063 D3 from twelve to fourteen semantic storage capabilities
(`HookRegistrationStore`, `CronScheduleStore`); ADR-068 Consequences
points to it; the plan's NEEDS RULING section became "RBQA-F002 ruled";
bk-1 no longer waits on a ruling and owns the ADR-036 amendment paragraph
and the two `docs/atm-storage/boundaries.md` sections. ADR-063 itself is
not edited. No anchor was added: the capability count is governance, not
a behaviour a sprint can be judged on.

## Second gate round: bk-3 and bk-4 revisions

- bk-3 round 2 (`plan-20261019T005404Z-bk-3.json`) left eight cells
  underspecified. Fixes: an ANC-021 AC (the dispatcher impl and the
  task-hook path share `dispatch_hook_reply`), an ANC-007 sentence (an
  action's request goes through the HTTP handler's `ApiRequest::from`, no
  action parser), the REQ-BK-D3 CLI clause reworded (the CLI spawns no
  hook process; `send_to.rs` is the pre-existing Send-To transfer). Five
  exclusions recorded with reasons (ANC-015 doctor section is bk-5,
  ANC-018 table is bk-1, ANC-019 subcommands are bk-5, ANC-023 the stop
  condition is bk-1/bk-2/bk-4/bk-5 and bk-3 only forwards it,
  REQ-BK-HOOK-CLI subcommands are bk-5); the ACs those anchors labelled
  stay, re-labelled with their ADR decision. Ceremony trimmed to stay
  under the cap: the bk-5 composition note and the beads sentence.
- bk-4 round 1 (`plan-20261019T005409Z-bk-4.json`) left ANC-022,
  ANC-023, REQ-BK-D3 and REQ-BK-LOG underspecified. Fixes: `custom = {}`
  is now explained (a schedule carries no owner custom, atm never reads
  or alters it); the `AtmCronDeps.runner` comment and a new REQ-BK-D3 AC
  state the crate receives the router's one runner and never builds or
  spawns one; the ANC-023 AC says none-or-both is unrepresentable here
  and names bk-5/bk-1 for the CLI and CHECK refusals; the log contract
  gains a `hook` field, a `stop_check` entry per `Hook` stop check, and
  names the dispatcher (bk-3) as the writer of per-action entries. The
  header anchor enumeration and three file:line restatements were
  trimmed to stay under the 24 000-byte cap (23 937 bytes).

## Round 6: fixtures for the stop-condition anchors, final gate (2026-10-19)

- ANC-023, ANC-024 and ANC-025 were locked in round 4 without fixtures.
  One pass and one fail fixture each were drafted from the anchor
  statements (lock-time capture, not tuning); `check-fixtures` landed all
  six as expected (pass `satisfied`, fail `contradicted`). Every one of
  the 34 anchors now has a checked fixture pair; no question was tuned.
- Final gate, all anchors, every item: bk-1 `plan-20261019T011006Z`,
  bk-2 `plan-20261019T011200Z`, bk-3 `plan-20261019T011225Z`, bk-4
  `plan-20261019T011203Z`, bk-5 `plan-20261019T010934Z`; no failing cell;
  coverage 34/34. REQ-BK-ERRORS moved to version 3 after its pass fixture's
  spelling was corrected ("unparsable", spell lint) and re-checked; its
  bk-1 and bk-5 cells are exclusions, which never reach Jev.
- bk-1 ANC-020 came back `underspecified` at 0.59 (leaning satisfied) on
  text that had passed. The weak spot was real: the restart criterion
  proved reopen survival without naming the daemon-start load. One
  replacement in that criterion names bk-4's worker loading
  `active_all()` at daemon start (+86 characters, authority: locked
  ANC-020 and bk-4's existing loader text); re-run satisfied 0.98.
- bk-3 ANC-010 fell from 0.61 to 0.58 on the relevance question with no
  text change. The criterion did not say the hook spawn is the subprocess
  the anchor's scope names; one replacement states that the hook-module
  spawn is the only subprocess bk-3 adds and that no bk-3 file calls `bd`
  or touches `.beads` (+127 characters, authority: locked ANC-010);
  re-run satisfied 0.92.

## Round 7: ruled reply enum and filter (2026-10-19)

Lock-time capture of Rand's 2026-10-18 rulings on #1873; no question was
tuned and no anchor was added. Authority, verbatim: "we should add a 3-rd
return type. instead of success=true/false which signals pass-through, we
could return enum with { handled => run my actions, not-applicable,
pass-through }"; "ahh, I forgot rufused, that is a significant one. same
pattern works though"; "the return array of actions are then injectec into
the normal input stream bypassing the hook-plug-in implementation";
"adding ATM_HOOK=1 is a lot more logic branching"; "manformed hook MUST
return hook failed to caller (error)"; "phases are INDEPENDENT entities";
"the cron timeout would need to be much longer than the hook timeout. 2.5s
is fine for hook"; "doctor would need to know team, phase and it would
return a dict/array"; plus the ATM_HOOK_* code names of design §9.1.

- Statements reworded to the four reply results, the filter, the
  team-only lookup, the cron cap and the four-variable environment:
  ANC-001, ANC-004, ANC-005, ANC-006, ANC-012, ANC-013, ANC-014, ANC-021,
  REQ-BK-D1, REQ-BK-ERRORS (REQ quotes re-captured from requirements §23.3).
- Checks and fixtures only: ANC-003, ANC-007, ANC-008, ANC-010, ANC-011,
  ANC-015, REQ-BK-D3, REQ-BK-LOG, REQ-BK-SCHEMA (no schema version
  constant exists; the record is ADR-061 D6).
- ANC-005's sealed questions named `{"success": true}`; only those
  reply-shape names were replaced, criteria otherwise unchanged.
- Exclusion reasons corrected where they named the router flag, the
  envelope or ATM_HOOK_ACTIVE; ANC-022's bk-4 exclusion removed (schedules
  carry `--custom`).
- check-fixtures: 38 of 38 as expected after REQ-BK-SCHEMA's pass fixture
  was restated to name the existing migration path its statement requires.
- Gate: bk-1 `plan-20261019T022704Z`, bk-2 `plan-20261019T022704Z`, bk-3
  `plan-20261019T022720Z`, bk-4 `plan-20261019T022707Z`, bk-5
  `plan-20261019T022705Z`; coverage 34/34. Remaining low cells (bk-2
  REQ-BK-ERRORS; bk-3 ANC-021, REQ-BK-D3; bk-4 ANC-004, ANC-012, ANC-023,
  ANC-024, REQ-BK-ERRORS, REQ-BK-LOG) go to the round-2 fix layer as
  sprint-doc gaps; sprint docs were not edited in this round.
