---
name: adversarial-fuzzing
version: 1.1.0
description: Generate and triage adversarial templates, var-files, and rendering inputs against sc-compose by coordinating bounded background agents, differential and metamorphic checks, minimization, and regression-test promotion. Use when trying to break a rendering subsystem, validating a risky change, hunting parser/validator/rendering edge cases, or turning a confirmed fuzz failure into a unit or CLI test.
---

# Adversarial Fuzzing

Use this skill to attack one sc-compose rendering subsystem with several
focused, isolated probes. Preserve useful failures as minimal regression tests
only after reproducing and classifying them as real product bugs.

## Step 1 — Verify required CLI dependencies

Run this before selecting agents or executing a fuzz campaign:

```bash
which cargo && cargo --version
which git && git --version
```

If either command is unavailable, stop and read
`references/installation-and-troubleshooting.md` before proceeding. Do not
silently run a degraded campaign.

## Campaign contract

Require a JSON input object with these fields. Reject missing or unsafe paths
before delegating work.

```json
{
  "worktree_path": "/absolute/path/to/sc-compose-worktree",
  "target": "var-file | frontmatter | resolver | renderer | includes | cli | full",
  "baseline_ref": "optional git ref for differential checks",
  "seed": 157,
  "max_workers": 4,
  "cases_per_worker": 100,
  "per_worker_timeout_s": 120,
  "promote_regressions": true,
  "notes": "optional target-specific context"
}
```

Require `worktree_path` to be an existing absolute path inside the repository
or an explicitly approved isolated worktree. Resolve and validate all generated
paths beneath it; reject path traversal and paths outside the worktree.

Use a deterministic seed by default, cap concurrency at four workers, set a
per-worker timeout, and record the seed and campaign correlation ID in every
result. Use a fresh temporary directory for generated inputs and clean it up
after the campaign unless a failure artifact is being preserved.

## Workflow

1. Verify the CLI dependencies above.
2. Inspect the target subsystem, current tests, repository boundary rules, and
   the requested baseline. Keep the user's existing changes intact.
3. Use the Agent Runner to invoke the registered
   `sc-adversarial-fuzz-coordinator` agent as the primary coordinator with the
   campaign contract. Do not invoke an unregistered agent path.
4. Have the primary coordinator spawn a swarm of focused workers in the
   background with `run_in_background: true`. Each worker gets a distinct fuzz
   task, runs one bounded fuzz test, and returns exactly one structured JSON
   result with its test inputs, iteration count, pass/fail counts, and any
   candidate finding.
5. Aggregate worker results in correlation-ID order. Surface partial failures;
   never discard a timeout, malformed response, or worker error.
6. Reproduce each candidate failure in the primary coordinator's worktree,
   minimize the template and input while retaining the failure, and classify
   the outcome using the oracle rules below. For every candidate that remains
   plausible, the primary may deploy background explore agents to locate the
   relevant requirement, ADR, or NFR, establish root cause, and recommend the
   next change. Merge their conclusions into the worker's structured result.
7. When a candidate is a confirmed product bug and `promote_regressions` is
   true, add the smallest durable test to the nearest existing test suite.
   Prefer inline fixtures for small cases and checked-in fixtures for complex
   templates. Do not change production code or silently implement a fix during
   a fuzz campaign.
8. Run targeted tests, then the repository's required formatting, test, lint,
   and boundary checks when a regression test was added.
9. Return the coordinator's fenced JSON report and summarize confirmed bugs,
   promoted tests, unresolved candidates, and campaign limits.
10. After the campaign summary is complete, have the primary coordinator
    investigate every candidate failure before producing the report. It may
    deploy background explore agents to locate the relevant requirement, ADR,
    or NFR; establish the evidence-backed root cause; and identify the
    recommended change. Preserve those conclusions in the worker's structured
    JSON envelope. Then generate one report package for the fuzz session. Each
    swarm worker runs one bounded fuzz test and returns one structured result;
    the package contains one XHTML panel per worker. Use the reusable
    `.claude/skills/html-report/templates/fuzz-run-agent.xhtml.j2` template for
    those panels and delegate the single main HTML/JSON package to the
    `html-report-generator` background agent. Its top-level `summary_html` must
    be a compact table with the fuzz-run description, iteration count, pass
    fraction (`passed/iterations`), and a simple PASS/FAIL result. Keep each
    panel's `json_payload` equal to the worker's durable evidence envelope and
    provide `context_text`; do not invent a second campaign schema. Write real
    session artifacts to `site/reports/`, assigning a 1-based sequence in
    deterministic session order and resetting it for each campaign day. The
    filename stem must be `YYYYMMDD-N-fuzz-report`, for example
    `site/reports/20260729-1-fuzz-report.html`; keep the matching `.json`
    sidecar and one companion `.xhtml` panel per worker under the derived
    `site/reports/20260729-1-fuzz-report/` directory, using a deterministic
    worker suffix when more than one panel is present. The report generator
    must validate the HTML output with `html-validate` and every XHTML panel
    with `xmllint --noout` before the session is reported complete. Review-only
    examples belong under `docs/examples/fuzz-run-report/`, not
    `site/reports/`.

## Worker portfolio

Ask the coordinator to deploy only the workers relevant to `target`; use all
four for `full`:

| Worker | Focus | Primary probes |
| --- | --- | --- |
| `shape-probe` | Values and ingress | recursive JSON/YAML trees, mixed arrays, empty values, numeric edge cases, object/map nesting |
| `template-probe` | Jinja behavior | nested loops, conditionals, includes, delimiters, whitespace, missing/optional fields, Unicode |
| `boundary-probe` | Negative contract | malformed files, non-object var-files, invalid YAML keys, invalid names, stable diagnostics, path confinement |
| `differential-probe` | Change/regression oracle | baseline-vs-head behavior, JSON/YAML parity, deterministic output, timeout/panic/hang detection |

Each worker must stay within its target, use bounded generation, and return a
standard fenced JSON envelope. Workers may create temporary inputs and logs,
but must not edit production code, commit changes, or delete user files.

When a finding cannot be traced to an existing requirement or ADR, record that
absence explicitly and assess whether it is a genuine contract gap. The
recommended action must be one of: create or update a requirement/ADR before
implementation when the behavior is supported and the gap is real; document
why no new requirement/ADR is needed when the behavior is intentionally
unsupported or too narrow; or leave the assessment pending with a named owner
when product intent is not yet established. Do not create documentation merely
because a finding lacks a reference.

## Oracle and triage rules

Classify a candidate as a confirmed bug only when at least one condition holds:

- the process panics, hangs, exceeds the configured timeout, or exits with an
  unexplained internal error;
- a valid input that the documented contract accepts renders incorrectly;
- equivalent JSON and YAML inputs produce different semantics without a
  documented reason;
- a metamorphic relation fails, such as adding an unused object field changing
  output or reordering independent inputs changing deterministic output;
- a formerly valid input regresses against `baseline_ref`;
- a stable diagnostic code or top-level boundary is violated.

Do not file a bug for an intentional validation error, a malformed test input,
an unsupported feature explicitly documented by the target contract, or an
oracle that cannot establish expected behavior. Preserve such cases as
negative-coverage notes when they reveal a missing contract.

For every candidate, require:

```json
{
  "id": "FUZZ-001",
  "status": "confirmed_bug | intentional_boundary | inconclusive",
  "subsystem": "var-file",
  "seed": 157,
  "command": "cargo run ...",
  "minimal_template": "...",
  "minimal_input": "...",
  "observed": "...",
  "expected_oracle": "...",
  "diagnostic": "optional code",
  "requirement_trace": "existing requirement/ADR, or explicit no-coverage statement",
  "requirement_follow_up": "create/update, no-new-doc rationale, or named decision owner",
  "reproduction_count": 3,
  "recommended_test": "..."
}
```

Minimize by deleting unrelated template blocks, variables, fields, array
members, and nesting while re-running the exact command. Keep the smallest
reproducer that fails at least three times, or record nondeterminism explicitly
if it cannot be reproduced deterministically.

## Regression-test promotion

Promote only `confirmed_bug` candidates. Place tests according to behavior:

- pure value conversion/validation: `crates/sc-composer` unit tests;
- CLI ingress, diagnostics, or output: `crates/sc-compose/tests/cli.rs` or
  `json_cli.rs`;
- cross-platform path or boundary behavior: the existing boundary test suite.

Every promoted test must include the minimized input, expected output or stable
diagnostic, and a short comment naming the fuzz finding ID. Avoid comments that
leak campaign-only assumptions; document the user-visible invariant instead.

After promotion, run:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --all-targets --all-features -- -D warnings
```

Also run the repository's boundary checks and retain the generated campaign
report as an artifact when the team workflow requires it. A test that cannot
be made deterministic or cross-platform should remain a finding, not be
promoted as a flaky regression.

## Aggregation contract

The coordinator must return fenced JSON only, with results ordered by
`correlation_id`:

```json
{
  "parallel": true,
  "concurrency": 4,
  "per_task_timeout_s": 120,
  "results": [],
  "summary": {
    "all_successful": true,
    "confirmed_bugs": 0,
    "promoted_tests": 0,
    "inconclusive": 0,
    "failed_workers": []
  },
  "error": null
}
```

Use `success: true` when the campaign completed even if findings exist. Use
`success: false` for invalid input, coordinator failure, or a malformed worker
contract. Represent timeouts as recoverable worker failures and do not retry
more than once.

## First-Campaign Checklist And Evidence

Use this checklist for the first real campaign (owned by E.3) and for later
independent quality-mgr passes:

1. Confirm the requested worktree is an existing approved absolute path and
   record the baseline ref. Do not claim E.1 is merged unless the target
   integration branch actually contains it.
2. Record the seed, target, worker cap, cases per worker, timeout, promotion
   flag, campaign ID, and generated campaign directory before launching.
3. Run the full target with the four registered workers when `target` is
   `full`; record every correlation ID, worker target, case count, status,
   timeout, and error.
4. Record every candidate with the exact command, minimized input/template,
   expected oracle, observed result, diagnostic, and reproduction count.
5. Record classification and promotion decisions. A confirmed bug needs three
   reproductions and a durable owning-crate test; an unresolved confirmed bug
   needs a next owner. Do not claim the pipeline is proven in E.2.
6. Run the relevant validation gates and preserve the report outside temporary
   files when E.3 or quality-mgr requires durable evidence.

The durable report must be a JSON object matching this contract:

```json
{
  "schema_version": "adversarial-fuzzing/v1",
  "campaign": {
    "campaign_id": "e3-20260729-0001",
    "worktree_path": "/absolute/approved/worktree",
    "target": "full",
    "baseline_ref": "optional git ref",
    "seed": 157,
    "max_workers": 4,
    "cases_per_worker": 100,
    "per_worker_timeout_s": 120,
    "promote_regressions": true
  },
  "workers": [
    {
      "correlation_id": "shape-probe",
      "target": "var-file",
      "status": "success | failed | timed_out",
      "cases_run": 100,
      "finding_ids": ["FUZZ-001"],
      "error": null
    }
  ],
  "findings": [
    {
      "finding_id": "FUZZ-001",
      "worker_correlation_id": "shape-probe",
      "classification": "confirmed_bug | intentional_boundary | inconclusive",
      "command": "cargo run ...",
      "minimal_template": "...",
      "minimal_input": "...",
      "expected_oracle": "...",
      "observed_result": "...",
      "diagnostic_code": null,
      "reproduction_count": 3
    }
  ],
  "promoted_tests": [
    {
      "finding_id": "FUZZ-001",
      "test_path": "crates/sc-compose/tests/cli.rs"
    }
  ],
  "unresolved_candidates": [
    {
      "finding_id": "FUZZ-002",
      "next_owner": "team-lead"
    }
  ],
  "summary": {
    "all_successful": true,
    "confirmed_bugs": 0,
    "intentional_boundaries": 0,
    "inconclusive": 0,
    "failed_workers": 0
  }
}
```

Order workers and findings deterministically by correlation ID and finding ID.
`all_successful` is false for any worker failure or timeout. A report with no
findings but incomplete execution is not a passing no-finding campaign.

## Safety and cleanup

- Never run destructive commands, network-facing services, or arbitrary code
  from generated templates.
- Do not expose secrets, environment dumps, or unrelated repository contents in
  findings.
- Keep all writes inside the approved worktree and temporary campaign directory.
- Never reset, clean, or overwrite the user's existing changes.
- Remove temporary inputs after collecting confirmed artifacts.
- Do not commit production fixes automatically; commit only promoted tests when
  the user or repository workflow explicitly authorizes that mutation.
