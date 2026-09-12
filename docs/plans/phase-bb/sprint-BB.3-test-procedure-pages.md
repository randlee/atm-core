---
status: planned
branch: feature/bb3-test-procedure-pages
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/bb3-test-procedure-pages
---

# BB.3 — Test-procedure pages (PARALLEL with BB.1)

| Field | Value |
| --- | --- |
| Phase | BB ([`phase-bb-plan.md`](./phase-bb-plan.md) §3; design §10) |
| Schedule | **PARALLEL. Wave 1, starts with BB.1.** `parallel_safe` with BB.1 and BB.2 (plan §4); waited on by nothing. |
| Owner | cipher |
| Branch | `feature/bb3-test-procedure-pages` off `integrate/phase-bb`; PR targets `integrate/phase-bb` |
| File overlap with BB.1, BB.2 | none. BB.1 touches `crates/**`; BB.2 touches `.claude/skills/**`. This sprint touches `docs/procedures/**`, `scripts/procedures/**`, `templates/procedure-report/**`, `site/reports/procedures/**`, `.just/generate_report_index.py`, the three evidence writers named in D3, and their tests. |
| Rulings | Rand 2026-09-12: "When I read through the current reports (smoke, integration, fuzz, ...), there is no way for me to see what the test did. i.e. every test has a test procedure and that procedure should be linked from the test. Ideally the test procedure would contain diagrams and good explanation for the user reviewing and available in html. (I expect we would only need a single page for each procedure version which would change slowly over time)." / "I am not asking you to rewrite tests, I just want info to help me understand exactly what procedure is done." / "we may need to dig up some older procedures from git history" |
| Requirements / ADRs edited | none |
| Governed interfaces (ADR-061) | none. Evidence JSON, the report index and the procedure manifest are not one of the three governed interfaces; their contract is pinned in §2 D3 and D5 of this doc. |
| Not in scope | changing what any runner does; a runner that executes its steps from the procedure file (considered, rejected by Rand as test rewriting); editing any existing file under `site/reports/**` other than adding new files; `atm-hermes-testbed` changes |

## 0. What the reader gets

Every report page on the public site (`site/reports/index.html`, published by
`.github/workflows/pages.yml`) gains one header line:

    Procedure: smoke-thorough @ 281e6f54 — what this run did

linking to a page that explains, with a diagram and an ordered step table,
exactly what the runner does at that revision. One page per procedure per
step-changing runner revision. Historical reports that carry no revision are
linked to the page in effect on their run date, with a visible "inferred from
run date" note.

The guarantee this sprint provides is provenance, not execution: the page
describes the runner revision that produced the report, and the report index
refuses to publish a report whose revision maps to no page. It does not make
the runner read the page (Rand: no test rewrites).

## 1. Families and their runners (verified at `281e6f546`)

| Procedure id | Runner | Evidence JSON | Envelope writer | Revision in evidence today |
| --- | --- | --- | --- | --- |
| `smoke-fast`, `smoke-normal`, `smoke-thorough` | `scripts/smoke/run_feature_smoke.py` (`FIXTURE_FEATURES`, L49; `add_case` L527; `write_report` L806) | `<run>/<feature>.json` (`feature, platform, host, run_id, status, cases[]`) | `write_report` L868–880 → `smoke.envelope.json` | **none** |
| `colima-hermes-skills` | `scripts/smoke/colima_skill_report.py` (`render` L71) rendering `atm-hermes-testbed ./test.sh` output | `<run>/colima-hermes-skills.json` (same shape) | L99–103 | **none** |
| `read-query-benchmark` | `scripts/smoke/read_benchmark.py` (`_report_variables` L709; campaign payload L775–793) | `<campaign>.json` | `scripts/smoke/benchmark_report.py::render_envelope` L392 | `source_revision` L793 |
| `send-message-benchmark` | `scripts/smoke/benchmark_report.py` (30 revisions) | `<campaign>.json` | same | `source_revision` where the producer wrote it (index comment L32–36) |
| `fuzz-<campaign-target>` | `.just/run_fuzz.py` (`validate_campaign` L316; `CAMPAIGN_OPTIONAL_FIELDS` L160) + `scripts/fuzz/render_report.py` | `site/reports/fuzz/<campaign>.json` | `scripts/fuzz/render_report.py` | optional `source_revision` (L337–339) |

Runner history for backfill sizing (`git log --follow`): `run_feature_smoke.py`
20 commits since 2026-07-26; `run_fuzz.py` 14 since 2026-07-31;
`benchmark_report.py` 30 since 2026-07-31; `read_benchmark.py` 9 since
2026-08-30; `colima_skill_report.py` 1 since 2026-09-08.

## 2. Deliverables

### D1 Procedure documents — `docs/procedures/<procedure-id>.md`

One markdown file per procedure id in §1 (seven files). Markdown is the
source; nobody edits html. Each file has exactly these parts, in order:

1. YAML front matter:

   ```yaml
   procedure: smoke-thorough
   family: smoke
   runner: scripts/smoke/run_feature_smoke.py
   evidence: site/reports/smoke/<platform>/<host>/<run>-thorough/thorough.json
   revisions:                 # step-changing runner revisions, newest first
     - rev: 281e6f546          # full sha
       date: 2026-09-12
       note: "current"
     - rev: <sha>
       date: 2026-08-2x
       note: "added mTLS rejection cases"
   ```

   `revisions` is the backfill (§2 D4). The first entry is the revision the
   document describes; older entries carry a one-line note of what the steps
   were before that change, read from `git show <rev>:<runner>`.

2. `## What this test proves` — three to eight sentences for a reviewer.
3. `## Flow` — one ` ```mermaid ` flowchart of the run: setup, each case
   group, teardown, where evidence is written.
4. `## Steps` — a table with columns `step | action | observable | evidence`.
   Rows are in runner order. For `smoke-*` a row per `add_case` name (the
   `name` argument, e.g. `"rand-m5.local curl mTLS evidence"` at L486) so the
   `cases[].name` values in the JSON read straight across to the table. For
   the benchmarks a row per family/lane. For fuzz a row per worker (§1 of the
   provenance md, e.g. `shape-probe`, `template-probe`, `boundary-probe`,
   `differential-probe`).
5. `## Evidence layout` — what files a run leaves and how to read each.
6. `## Changes` — the `revisions` list rendered as prose, newest first.

Wording lint: `just lint spell` runs on `docs/**`; add new proper nouns to the
spell allowlist the same way BA.6 did.

### D2 Page rendering — `scripts/procedures/render_procedure_pages.py`

New script, modelled on `docs/reports/generate_diagram_pages.py`:

- reads every `docs/procedures/*.md`, splits front matter, renders the
  mermaid fence to inline SVG the way `render_svg` does (L272–290, `npx -y
  @mermaid-js/mermaid-cli`), renders the markdown body to html fragments;
- renders `templates/procedure-report/procedure.html.j2` (new; copy the
  shell of `.just/templates/view-report.html.j2`) with `sc-compose render
  --root . --file … --var-file … --output …` exactly as `render_file` does
  (L106–127). Fragment variables (`body_html`, `flow_svg`, `steps_html`) are
  emitted with `| safe`; scalars (`title`, `procedure`, `rev`) stay escaped.
  See `docs/plans/phase-ba/…` SMK-004's sibling finding and PR #1430: sc-compose
  ≥1.6 HTML-autoescapes every variable on `.html` outputs;
- writes `site/reports/procedures/<procedure-id>/<rev8>.html` for the head
  revision, and `site/reports/procedures/<procedure-id>/index.html` listing
  every revision in `revisions` with its note (older revisions link to the
  head page's `#changes` anchor; no page is fabricated for a revision whose
  runner was not documented at the time);
- writes `site/reports/procedures/manifest.json`:

  ```json
  {"schema_version": 1,
   "procedures": [{"procedure": "smoke-thorough", "family": "smoke",
                   "runner": "scripts/smoke/run_feature_smoke.py",
                   "revisions": [{"rev": "281e6f546…", "date": "2026-09-12",
                                  "html": "procedures/smoke-thorough/281e6f54.html"}]}]}
  ```

- `--check` mode: exit 1 when any output differs from what would be written
  (same contract as `.just/generate_report_index.py --check`, L355–370).

`just procedures [--check]` recipe in `justfile` next to `reports-index`
(L181). `.github/workflows/pages.yml` L34 gains `just procedures --check`
before `just reports-index --check`.

### D3 Provenance field in evidence

Add `"source_revision": <git rev-parse HEAD of the runner's checkout>` to:

- `scripts/smoke/run_feature_smoke.py::write_report` — in the `<feature>.json`
  payload (L813–822, beside `status`) and in `smoke.envelope.json`
  (L868–880). Resolve it once via `subprocess.run(["git","rev-parse","HEAD"])`
  next to `branch_version()` (L95); on failure write `null`, never guess.
- `scripts/smoke/colima_skill_report.py::render` — payload (L85) and envelope
  (L99–103). The runner is the report renderer here; the value is the
  atm-core checkout that rendered, and the testbed's own ref goes in the
  existing `header_cases` (L57) as a case named `testbed ref`.
- `.just/generate_report_index.py`: `OPTIONAL_FIELDS` (L34) gains
  `source_revision` and `procedure`; `Envelope` (L44) gains
  `source_revision: str | None` and `procedure: str | None`;
  `parse_smoke_result` (L201) reads `source_revision` when present.

Benchmarks and fuzz already carry `source_revision`; no change to their
writers. Existing evidence files are not edited (they stay byte-for-byte
runner output); they take the historical path in D5.

### D4 Backfill from git history

For each runner in §1, `git log --follow --format='%H %ad %s' --date=short --
<runner>` and, for each commit, `git show <rev>:<runner>` to decide whether
the set or order of `add_case` names / families / workers changed. Every
step-changing revision becomes a `revisions` entry (D1) with a one-line
note. Expected: five to six entries for `smoke-*`, two to three for
`run_fuzz.py`, three to four for `benchmark_report.py`, one for the rest.
Record the command output in the PR body so QA can re-run it.

### D5 Index linking and the gate — `.just/generate_report_index.py`

- `discover_envelopes` (L227) also loads `site/reports/procedures/manifest.json`
  (absent manifest ⇒ `ReportIndexError`, the site is not publishable without
  procedures once this lands).
- For every `Envelope`, resolve its procedure page:
  1. `procedure` field present → must name a manifest procedure; the entry
     whose `rev` equals `source_revision` (if present) else the newest entry
     dated ≤ `generated_at`; no entry ⇒ `ReportIndexError`.
  2. no `procedure` field → derive it: `report_type == "smoke"` ⇒
     `smoke-<feature>` for `run_feature_smoke` runs and `colima-hermes-skills`
     for that feature name (feature is in the result JSON, L201–226);
     `benchmark` ⇒ by evidence directory name; `fuzz` ⇒ `fuzz-<campaign
     target>` from the campaign JSON. Then the same revision/date rule, and
     the entry is flagged `inferred: true`.
- Refusal contract: a report that resolves to no page raises
  `ReportIndexError(f"{envelope_path}: procedure {procedure} has no page for revision {rev} (run date {generated_at})")`;
  `generate_report_index.py` exits 1 exactly as it does for a malformed
  envelope today (L355–370), so `just reports-index --check` and
  `pages.yml` fail closed. No other caller reads the index script.
- `_entry_html` (L296) appends `<a class="procedure" href="…">procedure
  smoke-thorough @ 281e6f54</a>` plus the text ` (inferred from run date)`
  when flagged.
- Report headers: pass `procedure_label` and `procedure_html` into
  `templates/smoke-report/inbound-peer-frame.html.j2` (header at L24) and
  `inbound-peer-review.html.j2` from `write_report` (L832–860) and
  `colima_skill_report.py::render` (L90–97); into
  `.just/templates/view-report.html.j2` (header L56) from
  `read_benchmark.py::_report_variables` (L709) and `benchmark_report.py`;
  into the fuzz page from `scripts/fuzz/render_report.py`. Historical pages
  are not re-rendered (evidence rule); the index link covers them.

### D6 Tests (pytest, discovered by `.just/run_pytests.py`)

`scripts/tests/test_render_procedure_pages.py` (real `sc-compose`, real
`npx` mermaid; skip with an explicit reason only when `npx` is absent, and
CI must have it — `pages.yml` already runs the diagram pipeline's
dependencies or gains them in this sprint):

- `test_renders_every_procedure_to_head_revision_page`
- `test_flow_svg_and_steps_table_are_markup_not_text` (the `| safe`
  regression class from PR #1430: parse the output with `xml.dom.minidom`
  and assert an `<svg>` and a `<table>` element)
- `test_scalar_title_stays_escaped`
- `test_manifest_lists_every_revision_in_front_matter_order`
- `test_check_mode_detects_stale_page`

`.just/tests/test_generate_report_index.py` additions (same fixtures style as
L62–330):

- `test_links_report_to_procedure_by_source_revision`
- `test_links_historical_smoke_result_by_run_date_and_marks_inferred`
- `test_rejects_report_whose_procedure_has_no_page`
- `test_rejects_site_without_procedure_manifest`
- `test_accepts_source_revision_and_procedure_envelope_fields`

`scripts/smoke/test_run_feature_smoke.py` (L230 pattern):

- `test_report_and_envelope_carry_source_revision`
- `test_missing_git_writes_null_source_revision`

`scripts/smoke/test_colima_skill_report.py`:

- `test_payload_and_envelope_carry_source_revision_and_testbed_ref_case`

## 3. Acceptance criteria

1. `just procedures --check` and `just reports-index --check` both exit 0 on
   the PR head, and `pages.yml` runs both.
2. `site/reports/index.html` shows a procedure link on every entry; a
   reviewer can open any smoke, colima, benchmark or fuzz report and reach a
   page with a diagram and a step table whose step names match the
   `cases[].name` (smoke/colima), family (benchmark) or worker (fuzz) values in
   that report's JSON.
3. Every historical entry links the page in effect on its run date and says
   "inferred from run date"; no historical evidence file has changed
   (`git diff --stat develop -- site/reports` shows additions only).
4. A fresh `just smoke fast` run on the PR head writes `source_revision` into
   both its JSON and envelope and its index entry links without the inferred
   note.
5. All tests in §2 D6 exist, pass, and are named exactly as listed.
6. `just lint spell`, `just lint lines`, `just lint pytests` pass. No
   `cfg(test)` shims, no unused code, no edits under `crates/`.

## 4. QA notes

- Diff the D1 step tables against `git show 281e6f546:<runner>` line by line;
  a missing or reordered case is a finding.
- Re-run the D4 git commands from the PR body; a step-changing revision not
  in `revisions` is a finding.
- Render one page with `sc-compose render` by hand and confirm the mermaid
  SVG is inline (no external script, the site is static).
- Confirm no runner behavior changed: `scripts/smoke/test_run_feature_smoke.py`
  and `.just/tests/test_run_fuzz.py` pass unchanged apart from the two new
  tests.

## 5. References

- Design §10: `docs/plans/nudge-transition-templates/design.md`; plan: `docs/plans/phase-bb/phase-bb-plan.md`
- sc-compose autoescape and the `| safe` rule: PR #1430, `templates/smoke-report/inbound-peer-pane.xhtml.j2` L17, sc-compose issue #607
- Diagram pipeline: `docs/reports/generate_diagram_pages.py`, `docs/templates/diagram-report/`, `docs/sc-compose-feature-request-diagram-panels.md`
- Public site contract: `docs/github-pages.md`, `.github/workflows/pages.yml`
- Evidence rule: committed evidence is runner-written, never authored or edited
