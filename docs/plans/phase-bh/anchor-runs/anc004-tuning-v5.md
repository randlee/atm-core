# ANC-004 v5 planning-question diagnostic

## Authority and record

- Rand ruling `01M508P4HBYKVEKKFZ1EMFGBJ9`: merge/fetch state is `ok`, `conflict`, or `unavailable`; unavailable means branch safety is unproven and holds sanity/QA dispatch. A fetch failure must propagate without a stale-ref fallback. PR and CI state do not hold dispatch.
- Native oversight ruling `01M509HD0YR2ZFQRNAVSXBSB1W` authorizes the canonical `anchors.json`/rendered `anchors.md` planning update through the normal CLI, without another product PR.
- The planner applied `jev_judge.py update` to `docs/plans/phase-bh/anchors.json`, producing locked ANC-004 v5. The original verbatim quote remains `we are working off a set of rules already in production.` The original PR/CI pass/fail fixtures remain, with one pass and one fail fetch fixture appended. `jev_judge.py validate`, `lint --id ANC-004`, and `render` succeeded.
- This is planning-only tuning. Development fixtures are not a release gate for this ruling.

## Frozen planning oracle

- `anc004-oracle-v5.json` was frozen before the first live Jev call; SHA-256 `34272d164c0008a22b75d3aefabf15fe6b9a1fd1d3d18ded02599a0a0f775d14`.
- Ten cases include both original approved fixtures, both ruling-specific fixtures, compliant dispatch, topical mention, silence, irrelevance, verified conflict, and an unavailable fetch misreported as conflict. Each has expected `relevant`, `addressed`, and `contradicted` labels and a threshold-free semantic verdict.
- Live requests used `PLAN_ANCHOR_ALLOW_REMOTE_JUDGE=1` only for the individual commands; `TYPESAFE_API_KEY` was neither logged nor copied. Jev answered as `jev-1.13.0`.

## Fixture check

`PLAN_ANCHOR_ALLOW_REMOTE_JUDGE=1 python3 .claude/skills/plan-anchoring/scripts/jev_judge.py check-fixtures --anchors docs/plans/phase-bh/anchors.json --gate plan --id ANC-004 --run-dir /tmp/anc004-tuning.FTG5s6/fixture-runs` passed all four planning fixtures: both positive cases `satisfied`, both negative cases `contradicted`. The generated run JSON files are under `/tmp/anc004-tuning.FTG5s6/fixture-runs/fixtures/`.

## Candidate comparison

Each candidate was asked twice on the ten unchanged oracle cases, using `jev_judge.py judge --gate plan --anchor ANC-004 --threshold 0 --no-confirm`. The threshold-zero projection is a tournament diagnostic only; normal production review retains its configured confidence threshold.

| Question set | File SHA-256 | Exact role cells | Exact semantic verdicts | Stable cases | Cells with confidence >= 0.8 | Accepted |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| Default `assets/questions/plan.json` | `39008e915a5af91ce6d9c9b74b5b6901b7940aff7572dd32638358b5bb26bee2` | 52/60 | 14/20 | 10/10 | 42/60 | No |
| `anc004-candidate-state-v5.json` | `c5d6b6031df7822aa2898035c23163c2f0ec0c3fb608a5d3397b810ec2a29760` | 54/60 | 14/20 | 10/10 | 51/60 | No |
| `anc004-candidate-decision-v5.json` | `6c6b6c005433b7d23afcef2dd3fd203dd7e0645f2ee4cfcbf0e4cb97f08f54e5` | 50/60 | 12/20 | 10/10 | 49/60 | No |

The focused state candidate still classified the topical-only and silent-dispatch cases as irrelevant and failed to treat a failed fetch reported as a confirmed conflict as contradiction. The decision candidate also called the clean-evidence case contradictory. The default missed those cases plus the clean-evidence `addressed` label. No candidate matched every frozen role and verdict; no question set is sealed or installed. The 60 generated Jev run files remain in `/tmp/anc004-tuning.FTG5s6/` and are not a canonical `plan-anchor/tuning-history/2` seal.

The topical-only and silent-dispatch relevance labels may be broader than ANC-004's scope, which says it applies to beads defining when dispatch is sent or held. They remain frozen in this diagnostic; the labels were not changed after seeing Jev's answers. Clarifying that boundary with the authority owner is necessary before using this oracle to seal an override.

`jev_judge.py update` supports statement, scope, summary, deterministic check, and fixtures, but exposes no CLI operation for `question_overrides`. ANC-004 has no question beads; no record override was hand-edited and no bead was created. A future override requires a supported, authorized record mechanism and a fully accepted tournament.

## Separate development diagnostic

`check-fixtures --gate dev --id ANC-004` returned `asserted_only` for both positive sentences because neither cites a checkable artifact; both negative cases returned `contradicted`. This is an evidence-fixture limitation, not a reason to weaken `evidenced`, and does not change the planning-only outcome above.
