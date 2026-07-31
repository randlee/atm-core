---
recommended_agent: Cipher-311d
recommended_model: fast
execution_mode: after_merge
execution_dependencies:
  - AI.40
dependencies_relation:
  - sprint: AI.40
    relation: must_follow
    rationale: Both plans own the benchmark JSON/report artifact boundary.
---

# Benchmark HTML reporting plan

## Recommended Agent / Model

`Cipher-311d` / fast: the report-contract and rendering integration is
well-scoped planning/tooling work with no daemon hot-path change. This is a
planning-time recommendation, not a binding assignment.

## Execution Dependencies

This work `must_follow`s AI.40. Merge-forward trigger: AI.40 development is
pushed, not QA; before every round merge AI.40 into this branch. PR-completion
trigger: AI.40's PR merges into its integration target first. Both plans own
the benchmark JSON/report artifact boundary.

## Dependency Relations

| Sprint | Relation | Rationale |
| --- | --- | --- |
| AI.40 | must_follow | Both plans own the benchmark JSON/report artifact boundary. |

Status: planning-only proposal. This document defines the reporting layer for
the separate benchmark-execution work; it does not add benchmark workloads,
change daemon runtime code, or enable the project website.

## Design principles and scope split

The report producer consumes one deterministic benchmark-run JSON document and
delegates rendering to the shared user-level `html-report-generator` agent and
`html-report` skill. Those assets require `sc-compose` templates, a complete
JSON sidecar, optional XHTML fragments, inline CSS, and only copy-button
clipboard JavaScript. No repo-local HTML generator, chart library, browser
runtime, or ad-hoc string-concatenated document is permitted.

This is too broad for one production sprint because it crosses an atm-core
schema/adapter, a shared reporting contract, and a cross-product convention.
Split implementation into these sprints:

1. **Contract and fixture:** agree the benchmark-run schema, report name
   derivation, sidecar/index policy, and a representative fixture. Add only
   `site/reports/` scaffolding and a validating adapter; no chart extension is
   merged yet.
2. **Shared chart capability:** coordinate with the owner of
   `~/.claude/agents/html-report-generator.md` and
   `~/.claude/skills/html-report/` to add an approved structured SVG chart
   field and renderer. This is a cross-cutting dependency and must land in the
   shared standard before atm-core relies on it.
3. **Benchmark integration:** connect arch-ctm's `just benchmark` output,
   render one report per campaign/run, add local validation/opening recipes,
   and document retention/CI handoff.

Benchmark execution, hotpath instrumentation, CI regression policy, and full
`site/` enablement remain out of scope for this reporting sprint. The input
contract is intentionally compatible with the three expected benchmark areas:
local UDS/TCP admission and dispatch, mailbox reads, and cross-host peer
delivery.

## Authoritative deliverables

The implementation is complete only when all of these deliverables land at a
production-ready level for the scope stated above:

1. A versioned benchmark-run JSON Schema and producer contract consumed by the
   reporting adapter, including host, workload, series, units, provenance, and
   validation/error semantics.
2. A deterministic report-preparation adapter that maps one valid benchmark
   run into the html-report-generator fenced-JSON input contract without
   inventing fields or embedding required meaning in scratchpads.
3. A shared-contract change, separately reviewed by the html-report owner,
   that renders the required multi-line X-Y chart as pure inline SVG with no
   chart JavaScript. Until that dependency is accepted, the chart deliverable
   is explicitly not closed.
4. The per-run artifact layout under
   `site/reports/<report-name>.html` and
   `site/reports/<report-name>/`, with a JSON sidecar and any XHTML fragments
   at the derived sibling paths.
5. Minimal `site/reports/` scaffolding and a documented `just` reporting
   recipe that accepts benchmark JSON; full website navigation is not part of
   this sprint.
6. Deterministic validation of the schema, rendered HTML (`html-validate`),
   and every emitted XHTML fragment (`xmllint --noout`), plus a fixture proof
   that the chart contains one line for each `(packets_per_connection, host)`
   pair and that report names do not collide.

## Benchmark-run input contract

Arch-ctm's benchmark execution recipe produces this document. The reporting
adapter must reject unknown schema versions, missing provenance, mixed units,
non-finite numbers, duplicate series keys, and points whose X values are not
strictly ordered. A producer may add extension fields under `extensions`, but
the required fields below stay stable.

The canonical schema should live at
`docs/benchmarks/benchmark-run.schema.json` (JSON Schema draft 2020-12). The
following is the normative shape; it is intentionally shown as a Rust-facing
contract as well as JSON so implementation choices are unambiguous:

```rust
struct BenchmarkRun {
    schema_version: String,              // "atm.benchmark-run/v1"
    run_id: String,                      // globally unique producer ID
    campaign_id: String,                 // joins host captures into one chart
    benchmark: BenchmarkIdentity,
    provenance: Provenance,
    started_at: String,                  // RFC3339 UTC
    duration_ms: u64,
    x_axis: AxisSpec,
    y_metric: MetricSpec,
    series: Vec<BenchmarkSeries>,
    extensions: serde_json::Map<String, serde_json::Value>,
}

struct BenchmarkIdentity { name: String, version: String, command: String }
struct Provenance {
    git_sha: String, branch: Option<String>, toolchain: String,
    os: String, host: String, cpu: Option<String>, runner: Option<String>,
}
struct AxisSpec { key: String, label: String, unit: String }
struct MetricSpec { key: String, label: String, unit: String, direction: String }
struct BenchmarkSeries {
    key: String,                     // unique stable key
    host: String,
    packets_per_connection: u64,
    workload: serde_json::Map<String, serde_json::Value>,
    points: Vec<BenchmarkPoint>,
}
struct BenchmarkPoint {
    x: f64, metric: f64, samples: u64,
    p50: Option<f64>, p99: Option<f64>,
}
```

Example input:

```json
{
  "schema_version": "atm.benchmark-run/v1",
  "run_id": "20260731T041500Z-campaign42-9f31a2c1-host-macos",
  "campaign_id": "campaign42",
  "benchmark": {"name": "atm-daemon-local-capacity", "version": "1", "command": "just benchmark"},
  "provenance": {
    "git_sha": "9f31a2c1e4...", "branch": "feature/benchmarks",
    "toolchain": "rustc 1.94.1", "os": "macos", "host": "macos-arm64",
    "cpu": "Apple Silicon", "runner": null
  },
  "started_at": "2026-07-31T04:15:00Z",
  "duration_ms": 28431,
  "x_axis": {"key": "connections", "label": "Connections", "unit": "connections"},
  "y_metric": {"key": "throughput", "label": "Throughput", "unit": "ops/s", "direction": "higher_is_better"},
  "series": [
    {
      "key": "packets=1|host=macos-arm64",
      "host": "macos-arm64", "packets_per_connection": 1,
      "workload": {"path": "local-uds"},
      "points": [
        {"x": 1, "metric": 913.2, "samples": 20, "p50": 901.0, "p99": 1032.4},
        {"x": 8, "metric": 842.7, "samples": 20, "p50": 830.1, "p99": 991.8}
      ]
    }
  ],
  "extensions": {}
}
```

Arch-ctm's producer should write one file per host capture, then a small
campaign merge step combines captures with the same `campaign_id` before
rendering. The merge must retain each source's provenance and reject differing
benchmark names, axis definitions, metric units, or workload parameters. A
missing host is a visible `INFO`/`DRIFT` condition, not silently dropped data.

## Mapping to the html-report-generator input

The adapter constructs exactly one fenced JSON block for the background
`html-report-generator` agent. The benchmark-run document remains the machine
source of truth in the report section's `json_payload`; `body_html` contains
only deterministic prose/tables produced by the approved template family.

```json
{
  "output_path": "site/reports/atm-daemon-benchmark-20260731T041500Z-campaign42-9f31a2c1.html",
  "json_output_path": "site/reports/atm-daemon-benchmark-20260731T041500Z-campaign42-9f31a2c1/atm-daemon-benchmark-20260731T041500Z-campaign42-9f31a2c1.json",
  "title": "ATM daemon benchmark — campaign42",
  "subtitle": "throughput by connections, packets per connection, and host",
  "status": "PASS",
  "summary_html": "<p>Eight host/packet series were rendered from one validated campaign.</p>",
  "sections": [
    {
      "id": "xy-performance",
      "title": "Capacity comparison",
      "status": "PASS",
      "body_html": "<p>See the inline SVG chart and the accessible data table.</p>",
      "context_text": "Campaign campaign42; metric throughput (ops/s); X axis connections.",
      "json_payload": {"schema_version": "atm.benchmark-run/v1", "campaign_id": "campaign42", "series": []},
      "xhtml_path": "site/reports/atm-daemon-benchmark-.../atm-daemon-benchmark-...-xy-performance.xhtml",
      "fragment_source": "auto-generated",
      "evidence_rows": []
    },
    {
      "id": "run-metadata",
      "title": "Run metadata",
      "status": "INFO",
      "body_html": "<table>...</table>",
      "context_text": "Git, toolchain, host, workload, and timing provenance.",
      "json_payload": {"provenance": {}, "workload": {}}
    }
  ],
  "source_label": "atm-daemon just benchmark",
  "generated_at": "2026-07-31T04:15:30Z",
  "metadata": {"report_kind": "benchmark-run", "campaign_id": "campaign42"},
  "copy_actions": true
}
```

Required evidence must appear in `json_payload` and the HTML; `scratchpad_html`
is never used for benchmark measurements or verdicts. The adapter supplies
`output_path` at the top level and lets the shared generator derive the sibling
sidecar/fragments directory. It must run the required `html-validate` and
`xmllint` checks before reporting success.

## X-Y chart under the no-JavaScript constraint

### Recommendation: shared structured inline-SVG extension

Use a new optional `chart` field in the shared html-report-generator section
contract. The generator, not atm-core, computes scales and emits sanitized
inline `<svg>` containing axes, tick labels, a legend, accessible text, and one
`<polyline>` per `(packets_per_connection, host)` pair. This is server/agent-side
rendering: no chart library, external asset, raster data URI, or runtime
JavaScript is needed. The report must include an adjacent plain HTML data table
so the chart is accessible and remains meaningful when SVG is not visually
rendered.

Proposed cross-cutting contract addition (to be coordinated separately):

```json
{
  "chart": {
    "type": "xy-multiline",
    "x": {"label": "Connections", "unit": "connections", "values": [1, 8]},
    "y": {"label": "Throughput", "unit": "ops/s", "direction": "higher_is_better"},
    "lines": [
      {
        "id": "packets=1|host=macos-arm64",
        "label": "1 packet · macos-arm64",
        "color": "#2563eb",
        "points": [[1, 913.2], [8, 842.7]]
      }
    ],
    "aria_label": "Throughput by connections for each packets-per-connection and host pair"
  }
}
```

The shared template contract change is explicitly required: the current
`html-report` fields do not define `chart`, and atm-core must not silently
reinterpret `body_html` as an SVG escape hatch. The cross-cutting change must
add schema validation, finite-number/range checks, deterministic color
assignment, an SVG renderer, and HTML/XHTML fixture tests to the shared skill
and agent. If that change is rejected, the implementation should stop at the
contract/fixture sprint rather than embedding a competing chart renderer in
atm-core. Minimal clipboard JavaScript remains limited to the existing copy
buttons.

## Naming and artifact layout

The report name is derived once from the merged campaign input:

`atm-daemon-benchmark-<started_at_utc_compact>-<campaign_id>-<git_sha8>`

Normalize all components to lower-case `[a-z0-9-]`, cap each component, and
reject path separators. Include the campaign ID because one campaign may merge
Windows, macOS, and Linux captures; include the head SHA because the same
campaign label can be rerun. If a campaign has no shared SHA, use
`multi-<sha8-of-sorted-source-shas>` rather than a host name alone.

The required layout is:

```text
site/
└── reports/
    ├── README.md
    ├── .gitkeep
    ├── atm-daemon-benchmark-<name>.html
    └── atm-daemon-benchmark-<name>/
        ├── atm-daemon-benchmark-<name>.json
        ├── atm-daemon-benchmark-<name>-xy-performance.xhtml
        └── atm-daemon-benchmark-<name>-run-metadata.xhtml
```

Create only `site/reports/` scaffolding in this sprint; do not create a site
homepage, navigation, hosting, or browser-open implementation. Generated
reports should be ignored by default for local/CI output, with a documented
option for a curated baseline to be committed later. The sidecar and fragments
must use relative links from the top-level HTML, exactly as the shared contract
requires.

## sc-compose reconciliation status

No reply from `team-lead@sc-compose` or a relayed sc-compose owner was present
in the atm-dev inbox when this plan was finalized (2026-07-31). The local
sc-compose checkout is clean apart from unrelated `.cass/` and `.repowise/`
content, and its follow-on planning document describes a broader catalog,
latest/archive policy, producer-owned recipes, and a future `just reports`
aggregator. It does not establish the exact benchmark per-run name/index
convention required here. Before implementation, reconcile:

* whether the campaign ID/SHA naming above matches sc-compose's canonical
  report ID and archive policy;
* whether atm-core should publish a catalog/index entry or only the standard
  report package; and
* whether the shared chart extension belongs in sc-compose's report contract
  rather than the user-level html-report skill.

This is an explicit implementation gate, not a reason to invent local naming
or silently diverge from the cross-product standard.

## Authoritative acceptance criteria

The sprint is accepted only when every item below is true:

1. `docs/benchmarks/benchmark-run.schema.json` validates the normative input,
   and an arch-ctm-produced fixture can be validated without repo-specific
   undocumented fields.
2. The adapter maps one merged campaign to the documented fenced JSON contract
   with complete `json_payload`, metadata, deterministic status, and derived
   artifact paths; required meaning is absent from scratchpads.
3. The approved shared chart extension renders pure inline SVG with inline CSS
   only, no charting library, and no JavaScript beyond copy-button clipboard
   actions. The report has one distinct line per `(packets_per_connection,
   host)` pair plus an accessible table.
4. A report generated from the fixture exists at
   `site/reports/<report-name>.html`, with the sibling JSON sidecar and XHTML
   fragments under `site/reports/<report-name>/`; historical fixture runs do
   not collide.
5. `site/reports/README.md` documents the scaffolding, naming, retention, and
   future site boundary; full website enablement is not claimed as complete.
6. `html-validate` passes for the main report and `xmllint --noout` passes for
   every XHTML fragment; malformed schema, non-finite data, duplicate lines,
   and path traversal are covered by negative tests.
7. The sc-compose owner has either confirmed naming/index conventions or the
   implementation issue records the unresolved reconciliation and blocks
   publication beyond the local report package.

## Authoritative validation plan

1. Validate the schema and fixture with the selected JSON Schema validator,
   including malformed-version, missing-provenance, mixed-unit, non-finite,
   duplicate-series, and path-traversal cases.
2. Run the adapter twice from identical input and assert byte-identical fenced
   JSON, HTML, sidecar, and fragment outputs.
3. Render through the background html-report-generator using `sc-compose`; do
   not hand-author or concatenate a complete HTML document.
4. Run `html-validate site/reports/<report-name>.html` and
   `xmllint --noout site/reports/<report-name>/*.xhtml`; optionally run `tidy`
   as an additional smoke check.
5. Parse the output HTML and assert: `<!DOCTYPE html>` first, charset before
   CSS, no external stylesheets/iframes/scripts other than the copy handler,
   inline SVG present, one polyline per series, and a matching accessible data
   table.
6. Verify the JSON sidecar is the complete machine-readable source of truth,
   fragment links are relative, and every required section remains complete
   if scratchpad content is removed.
7. Exercise report-name derivation with repeated timestamps, different SHAs,
   multi-host campaigns, and hostile campaign/host strings.
8. Reconcile the sc-compose response and record the decision before enabling
   any catalog, archive, or cross-product publication path.

## File-by-file implementation and estimate

* `docs/benchmarks/benchmark-run.schema.json`: versioned input schema and
  examples (0.5 day).
* `docs/benchmarks/README.md`: producer/consumer contract for arch-ctm's
  `just benchmark`, merge semantics, units, and provenance (0.25 day).
* `scripts/benchmarks/prepare_html_report.py` (or an equivalent small adapter):
  validation, campaign merge, report-name derivation, fenced JSON preparation,
  and render/validation orchestration; no HTML generation (1–1.5 days).
* `site/reports/README.md` and `site/reports/.gitkeep`: minimal scaffolding,
  retention, and future-site boundary (0.25 day).
* `justfile`: `benchmark-report INPUT=...` and optional `benchmark-report-open`
  wrapper that delegates rendering to the shared backend; browser opening stays
  outside sc-compose (0.25 day).
* `.gitignore`: ignore generated reports by default while allowing a documented
  curated fixture/baseline location (0.1 day).
* `.claude/skills/html-report/` and
  `.claude/agents/html-report-generator.md` in the shared account: optional
  `chart` contract, SVG renderer, and validation fixtures; separate
  cross-cutting PR/coordination, estimated 1–2 days.
* `scripts/benchmarks/fixtures/benchmark-run-v1.json` and adapter tests:
  deterministic positive/negative fixtures and byte-stability checks (0.5–1
  day).

Expected atm-core reporting implementation effort is 2.5–4 engineer-days after
the shared chart contract is approved, plus 1–2 days for that shared contract
and one sc-compose reconciliation review. Benchmark execution remains owned by
the separate arch-ctm work and is not included in this estimate.

## Open questions and risks

* No sc-compose owner reply was available at finalization. Naming, index,
  latest/archive, and catalog integration remain open until explicitly
  reconciled.
* The shared html-report contract currently has no chart field. Implementing a
  local `body_html` SVG would violate the standardization goal; the cross-cutting
  contract change must be accepted or the chart sprint remains blocked.
* Multi-host charts require campaign merging. A host capture arriving late or
  with incompatible units must produce an explicit DRIFT/INFO report, not a
  silently incomplete comparison.
* SVG accessibility and deterministic color assignment need shared-template
  ownership; color alone must not be the only series distinction.
* `html-validate` and `xmllint` availability differs across macOS/Linux/Windows;
  local recipes should report actionable dependency errors and CI should use a
  supported validation image.
* Reports may contain host names, CPU details, or benchmark data unsuitable for
  publication. Default retention must be local/CI artifact-only until a
  publication policy is approved.
* The future `site/` may acquire an index or static-site generator. Keep this
  plan limited to `site/reports/` so that enablement does not become accidental
  scope.
