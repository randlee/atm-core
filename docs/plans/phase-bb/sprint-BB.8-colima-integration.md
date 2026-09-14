---
status: in-progress
branch: feature/bb8-colima-integration
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/bb8-colima-integration
---

# BB.8 — Colima integration: one sequence, one verdict, sequential panels

| Field | Value |
| --- | --- |
| Ruling | Rand, 2026-09-13 (verbatim below) |
| Recommended | arch-ctm |
| Depends on | nothing open; cut from `develop` @ b951ba330 (after #1499) |
| Worktree | `feature/bb8-colima-integration` |
| Governed interfaces | none (scripts, procedures, evidence layout, report index) |
| Owner of nav / index | `.just/generate_report_index.py` (`just reports-index`); report content under `site/reports/**` is atm-core's |

## Ruling (verbatim)

- "i do not want the colima tests littered around the repo. when an
  integration test is run w/ colima, it is a sequence of runs/tests which all
  aggregate to produce a pass/fail verdict."
- "the colima integration test process needs to generate required artifacts
  and organize them such that reporting is not a puzzle to be solved and
  [is] obvious from simply looking at the reporting directory structure"
- "I would like the colima-integration to be made up of a series of xhtml
  panels that play out sequentially. there needs to be traceability to what
  test-process was run for each result. We need to do this for historical
  runs so I can determine if what we are collecting is adequate."

## What exists today (the anti-pattern)

| Producer | Writes to | Runs | Linked from the site |
| --- | --- | --- | --- |
| `scripts/smoke/run_bb4_task_start.py` | `site/reports/bb4-task-start/<ts>/` | 2 | no |
| `scripts/smoke/run_bb5_assignment.py` | `site/reports/bb5-assignment/<ts>/` | 3 | no |
| `scripts/smoke/run_bb6_prompt_handoffs.py` | `site/reports/bb6-prompt-handoffs/<ts>/` | 3 | no |
| `scripts/smoke/colima_skill_report.py` | `site/reports/smoke/linux/hermes-testbed/<ts>-colima-hermes-skills/` | 4 | yes, as a smoke row |

Each bb runner docker-execs against its own testbed container, writes one
`<feature>.json` (with `status`, `source_revision`, image digest, `cases[]`)
and one hand-styled `index.html` with the JSON dumped in a `<pre>`. No
envelope, no procedure page, no panel, no way to tell from the tree that
these are one integration test.

## Target layout (the whole contract)

```
site/reports/integration/colima/
  <run-ts>/                         one driver invocation = one run
    index.html                      verdict banner, then the step panels in order (iframes)
    integration.json                aggregate: status, source_revision, image, steps[]
    steps/
      01-hermes-skills/             one directory per step, numbered in run order
        step.json                   the step runner's payload, byte-for-byte
        panel.xhtml                 the step's panel (self-contained XHTML, see below)
      02-task-start/
      03-assignment/
      04-prompt-handoffs/
  <run-ts>.envelope.json            report_type "integration", procedure "colima-integration"
                                    (a one-step historical run names its step procedure)
```

Rules:

- The driver (`scripts/integration/run_colima.py`, `just integration colima`)
  starts the testbed once, runs the steps in the declared order in that one
  container, and writes every file above itself. Nothing is assembled after
  the fact. A failing step is recorded and the run continues so the verdict
  covers the whole sequence; the aggregate `status` is PASS only when every
  step is PASS.
- Step runners become library functions called by the driver with the step
  directory as their only output location. `scripts/smoke/run_bb{4,5,6}_*.py`
  and `colima_skill_report.py` lose their `site/reports/<feature>` defaults
  and their standalone `index.html` rendering; their JSON payload shape is
  unchanged (historical payloads must still validate).
- `panel.xhtml` renders from `step.json` through an sc-compose template
  `templates/integration-report/step-panel.xhtml.j2` (XHTML 1.0 Strict like
  `templates/smoke-report/inbound-peer-pane.xhtml.j2`; well-formed XML;
  served as `application/xhtml+xml`). A panel shows, in this order: step
  number and name; verdict; the procedure that ran it as a link
  `Test plan: <procedure> @ <rev8>` resolving to
  `../../../../../procedures/<procedure>/<rev8>.html` (directory-form relative
  links; every link must pass `just lint site-links`); the container image
  digest; then one row per case: verdict, name, detail, and the exact
  commands with exit code and stdout. Machine text stays in `step.json`;
  the panel is for a human.
- `index.html` renders from `integration.json` through
  `templates/integration-report/run.html.j2`: verdict banner, source
  revision, image digest, procedure link for the run
  (`colima-integration @ <rev8>`), then one `<section>` per step with the
  step's verdict and an `<iframe src="steps/NN-name/panel.xhtml">`, in
  order. The nav stamp (breadcrumb, Result, Test plan) is added by
  `just reports-index`; do not hand-write a breadcrumb.
- `integration.json` is the aggregate: `generated_at`, `source_revision`,
  `image` (tag, id, sha256, architecture), `container`, `atm_version`,
  `procedure`, `procedure_revision`, `status`, `steps[]` with
  `{order, name, procedure, procedure_revision, status, started_at,
  finished_at, payload: "steps/NN-name/step.json", panel: "steps/NN-name/panel.xhtml"}`.
- The envelope follows the existing root-envelope schema used by smoke
  (`report_type`, `generated_at`, `source_revision`, `status`, `procedure`,
  `report_html`, `payload`) with `report_type: "integration"`.

## Traceability

- `docs/procedures/colima-integration.md` (family `integration`, runner
  `scripts/integration/run_colima.py`) names the sequence: one step row per
  step, each pointing at the step's own procedure.
- New step procedures `docs/procedures/colima-task-start.md`,
  `docs/procedures/colima-assignment.md`,
  `docs/procedures/colima-prompt-handoffs.md`; `colima-hermes-skills.md`
  moves to family `integration`. Every procedure follows the
  `render_procedure_pages.py` contract (front-matter `revisions` with a
  full SHA, a `## Revision <rev8>` section per revision, `just procedures`
  regenerates the pages; `just procedures --check` is a lint gate).
- Every run page and every panel links to the procedure revision that ran
  it; the index row links the latest run and the family history.

## Report index

- `.just/generate_report_index.py`: `REPORT_TYPES` gains `"integration"`;
  a family `Integration (colima)` appears as one row on
  `site/reports/index.html` with the latest run, its verdict, the procedure
  link and `N runs` → `history/integration.html`. Discovery is by the
  envelope, the same as smoke; no feature-name list.
- The nav stamp for a run page inside `integration/colima/<ts>/` and for
  its panels resolves the correct `../` depth (existing `_up`).

## Historical runs (Rand: "do this for historical runs")

- Every historical invocation becomes one run in the target layout with the
  steps it actually ran. Each of the 8 bb runs and the 4 colima-hermes-skills
  runs ran alone in its own container, so each becomes a one-step run named
  by its own timestamp. Do not synthesize a multi-step session that did not
  happen.
- Payload JSON files move with `git mv` and stay byte-identical (the
  historical `<feature>.json` becomes `steps/01-<name>/step.json`;
  the colima-hermes-skills `report-N.txt`, `result.txt`,
  `herdr-doctor.json` move alongside it). `integration.json`, the envelope,
  `index.html` and `panel.xhtml` for a historical run are generated from
  the moved payload by the same code path the driver uses (a
  `--from-payload` mode of the driver or a small `backfill` subcommand);
  values the old payload lacks (for example `procedure_revision`) are
  resolved from the procedure manifest the way the index does today and
  marked `inferred: true` in `integration.json`, never guessed silently.
- The old `site/reports/bb4-task-start/`, `bb5-assignment/`,
  `bb6-prompt-handoffs/` directories and the four
  `smoke/linux/hermes-testbed/*-colima-hermes-skills/` directories are
  gone after the move. `scripts/tests/test_site_reports_diff_is_generated_only.py`
  is extended to accept byte-identical renames (`git diff --name-status
  -M100%` status `R100`) so the guard keeps rejecting edits and deletions
  of evidence.
- The smoke history loses the four colima rows; the integration history
  gains twelve rows.

## Layers (append-only stack, one PR each)

The first dispatch was refused as too large for one execution window, so the
sprint lands as two layers. Nothing in the contract above changes.

| Layer | Branch | Contents | Needs colima |
| --- | --- | --- | --- |
| BB.8.1 | `feature/bb8-colima-integration` (PR #1500) | templates (D3), the three step procedures + `colima-hermes-skills` family move (D4 part), index family (D5), the render path from a payload to `integration.json` + panel + run page + envelope, historical backfill of the 12 runs (D6), guard relaxation (D7), tests for those (D9 part), plan row (D10 part) | no |
| BB.8.2 | `feature/bb8-2-colima-driver`, cut from the top of BB.8.1 | the live driver (D1), step-runner refactor (D2), `docs/procedures/colima-integration.md` sequence procedure (D4 rest), one real run (D8), driver tests (D9 rest), frontmatter `status: complete` (D10 rest) | yes |

BB.8.1 delivers the render path as a module the driver reuses in BB.8.2;
its entry point is `python3 scripts/integration/render_colima.py
--payload <step.json> --step <name> --out <run-dir>` (the backfill loops
over the twelve historical payloads with it). Every BB.8.1 run is one step,
so its envelope and `integration.json` name that step's procedure; the
`colima-integration` sequence procedure exists only once a driver runs a
sequence, so it lands with the driver in BB.8.2, and the renderer refuses a
multi-step run whose steps name different procedures until then. Rand's stated purpose for the
backfill is to judge whether what is collected is adequate, so BB.8.1 is the
layer that must be visible first.

## Tasks

1. Driver + step-runner refactor + templates (D1–D3).
2. Procedures (D4) and `just procedures`.
3. Index family, envelope type, nav depth (D5).
4. Historical backfill (D6) and the guard relaxation (D7).
5. One real run in colima on this branch's head (D8).
6. Tests (D9).

## Deliverables

- [ ] D1 — `scripts/integration/run_colima.py` + `just integration colima [--out DIR] [--steps a,b]` (the driver).
- [ ] D2 — `scripts/smoke/run_bb{4,5,6}_*.py`, `colima_skill_report.py` callable as steps; standalone `site/reports/<feature>` defaults removed; no dead code left behind.
- [x] D3 — `templates/integration-report/run.html.j2`, `templates/integration-report/step-panel.xhtml.j2`.
- [ ] D4 — `docs/procedures/colima-integration.md`, `colima-task-start.md`, `colima-assignment.md`, `colima-prompt-handoffs.md`; `colima-hermes-skills.md` family → integration; `site/reports/procedures/**` regenerated by `just procedures`.
- [x] D5 — `.just/generate_report_index.py` integration family + history page; `just reports-index` output committed.
- [x] D6 — twelve historical runs under `site/reports/integration/colima/`, payloads byte-identical (prove with `git diff -M100% --name-status`).
- [x] D7 — evidence guard accepts `R100` renames; test for it.
- [ ] D8 — one new run from the driver on this branch's head, committed as evidence (runner-written, never hand-edited), PASS or an honest FAIL with the failing step visible on the page.
- [ ] D9 — unit tests: driver aggregation (all PASS → PASS, one FAIL → FAIL, step order preserved), panel well-formedness (`xml.etree` parse of every `panel.xhtml`), backfill from each historical payload shape, index family row and history, guard rename acceptance.
- [ ] D10 — `docs/project-plan.md` BB table gains the BB.8 row; this doc's frontmatter `status: complete` at close.

## Validation (quote verbatim in the push report and the close body)

- `just lint` (includes `site-links`, `procedures --check` via pytests, `reports-index --check`) and `just test` summary lines at the final SHA.
- `python3 .just/check_site_links.py` pages scanned / 0 broken.
- `git diff -M100% --name-status $(git merge-base origin/develop HEAD) -- site/reports | grep -c '^R100'` = number of moved evidence files, and `grep -c '^D'` = 0.
- Open `site/reports/index.html`, `site/reports/history/integration.html`, the newest run page and one historical run page from the worktree (`open <file>`), and screenshot each with headless Chrome (`"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --screenshot=<png> file://<path>`); attach the four PNG paths in the push report. Fenix looks at them before the PR is reviewed.
- The new run's `index.html` shows every panel in order; each panel's `Test plan:` link opens the procedure revision page.

## Acceptance criteria

- One row `Integration (colima)` on the reports index with a verdict and a history link; the history lists thirteen runs newest first (twelve historical + one new).
- Every run page plays out its steps as ordered XHTML panels; every panel and every run page links to the procedure revision that produced it.
- No `site/reports/<feature>/` directory for any colima test remains; the reports tree reads as `benchmark`, `fuzz`, `integration`, `smoke`, `procedures` (plus the legacy root fuzz/benchmark files already there).
- All twelve historical payloads are byte-identical to their previous paths.
- `just lint` and `just test` green; CI green; no new flaky test.
