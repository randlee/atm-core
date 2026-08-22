---
phase: AO2
sprint: AO2.9
title: Benchmark report template, publication, and aggregate procedure
branch: plan/ao2-9-benchmark-report-template
integration_branch: integrate/phase-ao2
status: ready_for_review
must_follow: AO2.5.4 and AO2.6 merged to integrate/phase-ao2
parallel_safe: false
depends_on:
  - sprint: AO2.5.4-mandatory-benchmark-snapshot-restore
    relation: must_follow
  - sprint: AO2.6-admission-writer-batching-regression
    relation: must_follow
---

# AO2.9 — Benchmark report template, publication, and aggregate procedure

## Decision and scope

AO2.9 closes the benchmark-evidence process gap. It specifies one durable
single-run report contract, one immutable publication path, one aggregate
index, and one guarded finalizer for every physical `just benchmark` attempt.
This is a bounded tooling/documentation sprint: it may change the benchmark
runner, report/index scripts, Justfile, CI contract tests, host manifest, and
ADRs, but it must not change the daemon, client, storage, transport, TLS,
benchmark algorithm, or performance thresholds.

The benchmark remains an explicitly isolated operation under AO2.5.4. It must
not use the interactive account, the interactive daemon, an alternate data
root, or a benchmark-only production behavior. A result is evidence only when
its intent, report, raw artifacts, and publication commits satisfy this plan.

AO2.9 must complete before AO2.7 or AO2.8 begins physical execution. Their
target-specific threshold and safety documents defer to this plan for report
generation and publication. AO2.7/AO2.8 therefore list AO2.9 as a
`must_follow` dependency and include an explicit finalizer acceptance row.

## Dependency and ordering contract

| Predecessor | Relation | Gate |
| --- | --- | --- |
| AO2.5.4 mandatory snapshot/restore | `must_follow` | Its dedicated-account lifecycle is merged to `integrate/phase-ao2`; AO2.9 does not redefine recovery. |
| AO2.6 writer-batching regression | `must_follow` | Its tested benchmark subject is merged to `integrate/phase-ao2`; report tooling is validated against that interface. |
| AO2.7 M5 evidence | downstream | AO2.7 must follow AO2.9 and uses the finalizer; it is not an AO2.9 predecessor. |
| AO2.8 Windows evidence | downstream | AO2.8 must follow AO2.9 and AO2.7; it is not an AO2.9 predecessor. |

AO2.9 is `parallel_safe: false` with respect to AO2.7/AO2.8 because it owns
the only publication seam those sprints are allowed to call. Documentation
review may proceed in parallel with unrelated AO2 work, but no physical
benchmark target may run until this plan and its implementation PR are merged.

## Existing assets and implementation boundary

The repository already contains the intended sc-compose assets:

| Asset | Contract |
| --- | --- |
| `templates/benchmark-report/benchmark-run.xhtml.j2` | Single-run XHTML panel; its front matter is the variable contract. |
| `templates/benchmark-report/benchmark-report.html.j2` | Aggregate benchmark report shell. |
| `scripts/smoke/benchmark_report.py` | Schema validation and rendering only after the finalizer split. |
| `.just/generate_report_index.py` | Public report-envelope discovery and root index generation. |
| `Justfile` recipe `benchmark` | Isolated benchmark entry point; it must invoke the finalizer in all exit paths. |

The existing implementation is a starting point, not proof that AO2.9 is
complete: it writes a flat historical directory and only validates `uds` and
`tcp`. The implementation must extend these seams and must not create a
second renderer, result writer, or git publisher.

## Risk split and ownership seam

The security-sensitive work is explicitly split into two independently
reviewed packages in this sprint:

1. **Report package:** schema migration/validation, sc-compose panels,
   nested index discovery, aggregate rendering, and path/public-data tests.
2. **Publication package:** host binding, pre-execution intent, pending-run
   lock, outcome classification, commit/push protocol, crash recovery, and
   branch/PR reachability checks.

The second package requires ADR-054 and its own reviewer sign-off before the
implementation PR can merge. Keeping both packages in AO2.9 is intentional:
AO2.7/AO2.8 cannot safely run until the publication package exists. They share
one named module and one writer; no sprint may add an alternate `result.json`
writer or finalizer.

The actual shared publication module is
`scripts/smoke/benchmark_publication.py`. Its only public write API is:

```python
begin_run(spec: BenchmarkRunSpec, git: GitPublisher) -> PendingRun
finalize_run(pending: PendingRun, outcome: BenchmarkOutcome,
             git: GitPublisher) -> PublishedRun
```

`Justfile`'s `benchmark` recipe calls these functions around the existing
`scripts/smoke/run_admission_capacity.py` runner. AO2.7 and AO2.8 both call
the same recipe/API; neither may write `result.json`, render a panel, or run
`git commit`/`git push` independently. A CI boundary test must fail if a
second result writer or git-publish implementation is introduced.

## Single-run template and result contract

The canonical template is
`templates/benchmark-report/benchmark-run.xhtml.j2`. It is rendered with
`sc-compose render --root <repo> --file <template> --var-file <vars.json>
--output <run.xhtml>`. The producer validates all variables before rendering
and never interpolates untrusted values as HTML fragments.

The template front matter is the authoritative required-variable list:

| Variable | Type and rule |
| --- | --- |
| `title` | Non-empty string; escaped by the template. |
| `artifact_id` | Safe opaque run identifier; `[A-Za-z0-9][A-Za-z0-9._-]{0,127}`. |
| `generated_at` | UTC ISO-8601 timestamp with `Z`; generated once for the attempt. |
| `host_label` | Stable label from the checked-in host manifest; never an operator-minted per-run value. |
| `transport` | One of `sqlite`, `uds`, `tcp`, `tcp+tls`. |
| `frames_per_connection` | Non-negative integer; `0` when not applicable. |
| `run_duration_s` | Non-negative number covering the measured interval only. |
| `passed` | Boolean projection of machine-classified `status`; never caller-selected. |
| `failure` | Escaped diagnostic string; empty when no failure occurred. |
| `cleanup_failure` | Escaped cleanup/restore diagnostic string; empty when cleanup succeeded. |
| `sample_html` | Renderer-owned, escaped table fragment; never caller HTML. |
| `direct_sqlite_html` | Renderer-owned, escaped SQLite fragment; never caller HTML. |

`result.json` is the immutable source of truth. Its required schema is
`schema_version: 4` and these fields:

```json
{
  "schema_version": 4,
  "run_id": "20260822T200000Z-a1b2c3d4",
  "host_label": "mac-arm64-01",
  "os_family": "macos",
  "target": "tcp",
  "status": "PASS | FAIL | INCOMPLETE",
  "passed": false,
  "started_at": "2026-08-22T20:00:00Z",
  "finished_at": "2026-08-22T20:08:00Z",
  "source_revision": "<40 lowercase hex>",
  "binary_hashes": {"cli": "<sha256>", "daemon": "<sha256>"},
  "benchmark_account_label": "m5-benchmark",
  "profile": {"frames_per_connection": 8},
  "lifecycle": {
    "intent_pushed": true,
    "measurement_started": true,
    "measurement_completed": true,
    "cleanup_completed": true,
    "restore_completed": true,
    "interruption_reason": null
  },
  "threshold": {"name": "tcp-f8-p50", "expected": ">15000"},
  "metrics": {},
  "failure": null,
  "cleanup_failure": null,
  "raw_evidence": ["raw/stdout.log", "raw/stderr.log"]
}
```

`binary_hashes`, metrics, and threshold details are populated by the runner;
the operator cannot override them in a finalizer argument. `host_label` and
`benchmark_account_label` are public opaque labels only; raw host/account
facts remain local raw evidence and are not serialized into `site/`.

### Machine-classified FAIL versus INCOMPLETE

The finalizer derives status from structured lifecycle facts and the target
oracle; it does not accept a caller-provided status. The rules are:

- `FAIL`: the required measured interval completed and a measured assertion
  failed (for example, TCP f8 p50 missed its threshold), or the completed
  runner returned a non-zero validation/error result after producing a valid
  measurement. A threshold failure can never be relabelled `INCOMPLETE`.
- `INCOMPLETE`: the intent was pushed but build, preflight, launch, required
  measurement, cleanup, restore, or finalizer publication did not complete;
  this includes timeout, cancellation, process crash, and lost connection.
  The remaining intent marker is the evidence of registration.
- `PASS`: every required lifecycle phase completed and the target-specific
  oracle passed.

The aggregate retains every `FAIL` and `INCOMPLETE` record. A later pass never
collapses, overwrites, or hides either status; only the current-candidate
summary may select the newest complete campaign.

## Authoritative target matrix

`sqlite` is a direct storage-admission target, not a silent substitute for a
public transport. `uds`, `tcp`, and `tcp+tls` are public transport targets.

| Operating system | Required targets |
| --- | --- |
| macOS | `sqlite`, `uds`, `tcp`, `tcp+tls` |
| Linux | `sqlite`, `uds`, `tcp`, `tcp+tls` |
| Windows | `sqlite`, `tcp`, `tcp+tls` |

The schema, CLI validation, aggregate, and tests use exactly this matrix.
Windows rejects `uds`; an unsupported or unattempted target is missing, never
a synthetic pass. A campaign is complete only when all required targets for
that OS have machine-classified `PASS` results.

## Host identity and public labels

`host_label` is provisioned, not self-reported. The implementation adds
`tools/benchmark-hosts.toml`, a reviewed allowlist of stable opaque labels,
OS/architecture, benchmark-account label, and a non-reversible host-binding
digest. The raw machine facts used to calculate the digest remain local.

`begin_run` accepts only a label present in that manifest and verifies the
executing OS, architecture, benchmark-account manifest, and binding digest.
Adding or changing a host label is a separate reviewed manifest commit; a
run cannot modify the manifest and publish in the same operation. Reusing a
label for multiple attempts is therefore required and launder-by-new-label
behavior is rejected before an intent marker is written.

## Canonical branch, site reachability, and immutable layout

The canonical operator evidence branch is
`evidence/ao2-benchmark-reports`. The finalizer refuses any other current
branch and refuses to start unless that branch has an open PR into
`integrate/phase-ai-31-33`, the branch named by `docs/github-pages.md` as the
GitHub Pages publisher. A pushed run is therefore reachable from the
published-site branch through that PR; it is not considered published merely
because it exists on a private local branch. The operator must merge/update
that PR under the normal review gate before claiming public-site publication.

Every attempt receives one `run_id` before any build or benchmark command. It
is `YYYYMMDDTHHMMSSZ`; a same-second collision appends `-<8 lowercase hex
nonce>`. The nonce is generated only by `begin_run` using the OS CSPRNG and is
recorded in the intent marker and result; callers cannot supply it.

```text
site/reports/benchmark/
  index.html                              # aggregate/final report
  .pending/<run_id>.json                  # one active lock; safe public metadata
  <host_label>/<run_id>/
    intent.json                           # pushed before execution
    result.json                           # immutable validated source
    result.envelope.json                  # public-index envelope
    run.xhtml                             # sc-compose single-run report
    raw/                                  # stdout, stderr, daemon logs, traces
```

The envelope's `report_html` is the safe repository-relative path
`benchmark/<host_label>/<run_id>/run.xhtml`; it rejects `..`, absolute paths,
backslashes, and unmanifested labels. The envelope records
`schema_version`, `report_type: "benchmark"`, `generated_at`, and
`host_label`. The aggregate links JSON, XHTML, and raw evidence relative to
`site/reports`.

The implementation extends `.just/generate_report_index.py` to validate
nested benchmark envelopes, intent/result pairing, safe paths, and raw
evidence. An intent with no result is rendered as a flagged `INCOMPLETE`
process violation. The root `site/reports/index.html` links to
`benchmark/index.html`; historical reports are immutable and never pruned.

The root index is a cross-report directory, not a benchmark-only page. It must
remain browsable even when only one report family has new output and must expose
three category sections for `benchmark`, `smoke`, and `fuzz`. Category order is
defined by the generator's `REPORT_TYPES` constant (currently
`benchmark`, `fuzz`, `smoke`) rather than duplicated in a second template.
Within each section, entries are sorted newest-first by their canonical
`generated_at` timestamp, with the report path as a deterministic tie-breaker.
Each entry links to the rendered HTML report; benchmark entries may link to a
nested run report while smoke/fuzz links retain their existing layout.

The current `.just/generate_report_index.py` already declares
`REPORT_TYPES = ("benchmark", "fuzz", "smoke")`, groups entries by that
category in `render_index`, and sorts each category by descending timestamp in
`aggregate_entries`. Therefore category grouping and recency ordering for
existing smoke/fuzz output are an existing behavior to preserve and test, not a
new renderer to invent. AO2.9 still needs to extend discovery/validation for
the nested benchmark envelopes and prove that the same root index renders
benchmark, smoke, and fuzz sections together. Existing smoke/fuzz output is in
scope for regression coverage, but their producer contracts are not rewritten.

## Durable intent, lock, and finalizer protocol

`begin_run` is an external pre-execution registration, not an in-memory
callback:

```text
begin_run(spec, git):
  verify current branch == evidence/ao2-benchmark-reports
  verify open PR targets integrate/phase-ai-31-33
  fetch the remote evidence branch
  reject if any remote .pending/<run_id>.json exists
  verify the host manifest and generate run_id/nonce
  atomically write intent.json and .pending/<run_id>.json
  commit "benchmark: register <run_id> (<host_label>)"
  push and verify the remote branch contains that commit
  return PendingRun(intent_path, run_id, expected_remote_sha)
```

The pending marker is the single-flight lock. A second target is refused
until the prior finalizer has successfully pushed its completion and lock
release; the check is against the fetched remote branch, not just local
filesystem state. A test must mock a failed push and prove the next
`begin_run` refuses while the remote marker remains.

`finalize_run` is the only result writer and git publisher:

```text
finalize_run(pending, outcome, git):
  classify PASS/FAIL/INCOMPLETE from lifecycle + target oracle
  write immutable result.json and sc-compose run.xhtml
  rebuild benchmark/index.html and site/reports/index.html
  commit report + aggregate while .pending/<run_id>.json remains
  push; verify remote SHA and open-PR reachability
  remove .pending/<run_id>.json only after the report push succeeds
  commit/push lock release; verify no pending marker remains remotely
  return PublishedRun(remote_report_sha, status)
```

If the runner is killed with `SIGKILL`, crashes, loses power, or loses the
network, `finalize_run` cannot run; the already-pushed intent remains and the
index flags the registered run as `INCOMPLETE`/process violation. If report,
commit, or either push fails, the marker remains remotely and the next target
is blocked. Recovery must finish or explicitly close that run with a machine
classified incomplete result; operators may not delete the marker to unblock
the matrix.

## Aggregate/final report

`site/reports/benchmark/index.html` is regenerated after every finalization
from all immutable results and intents. It shows:

- current campaign status (`PASS`, `FAIL`, or `INCOMPLETE`),
- every attempted target, including failed/incomplete runs and intent-only runs,
- missing targets from the authoritative OS matrix,
- host label, run ID, tested SHA, target/profile, and measured status,
- links to JSON, XHTML, and raw evidence, and
- historical count, pending-lock state, and generation timestamp.

A failed historical run is never collapsed when a later run passes; the same
rule applies to incomplete runs. A complete campaign is `PASS` only when all
required targets for that OS have passed target-specific gates.

The finalizer also regenerates the root `site/reports/index.html`. That file
must present browsable HTML sections for **benchmark**, **smoke**, and **fuzz**
reports, using the generator-defined category order, with each section sorted
most-recent-first by `generated_at`. It must link both the new nested benchmark
reports and the existing smoke/fuzz reports; an empty category is rendered
explicitly rather than omitted. The generator's existing
`REPORT_TYPES`/`aggregate_entries`/`render_index` behavior is the baseline for
smoke/fuzz and receives regression tests while benchmark discovery is added.

## AO2.7 and AO2.8 relationship

AO2.7 remains the M5 TCP f8 throughput sprint and AO2.8 remains the Windows
parity sprint. Their thresholds, dedicated-account safety, and target evidence
remain authoritative. They defer to AO2.9 for schema, host identity, path,
intent/lock, finalization, and index behavior and must call the shared
`benchmark_publication.begin_run`/`finalize_run` path through `just benchmark`.

AO2.8 additionally records the accepted AO2.7 artifact and calculated floor in
its result JSON. Neither sprint may add a result writer, alternate finalizer,
or direct site publisher.

## ADR and trust model

ADR-054 (`docs/adr/ADR-054-benchmark-report-finalizer-trust-model.md`) records
the accepted trust model: the local benchmark account/finalizer is trusted to
classify and publish its own evidence through the canonical branch, while
GitHub branch protection, PR review, immutable intent/result paths, schema
validation, and generated-index checks provide independent repository gates.
There is no independent measurement verifier in AO2.9. The residual risk is
that a compromised operator account can falsify a measurement before pushing;
the plan accepts that risk for this local performance lane and makes it
visible through the immutable intent, source/binary hashes, raw evidence,
reviewable PR, and explicit trust statement. A future independent verifier is
deferred, not implied by this plan.

## Deliverables

- [ ] Hardened `templates/benchmark-report/benchmark-run.xhtml.j2` contract
      and schema-version migration, with the aggregate template retained.
- [ ] `scripts/smoke/benchmark_publication.py` implementing the named
      `begin_run`/`finalize_run` APIs and the single writer/publisher seam.
- [ ] `tools/benchmark-hosts.toml` reviewed stable-label/host-binding manifest.
- [ ] `Justfile` benchmark wrapper invoking the finalizer on every exit path.
- [ ] Nested benchmark discovery and aggregate support in
      `.just/generate_report_index.py`.
- [ ] Root `site/reports/index.html` remains a browsable directory with
      benchmark, smoke, and fuzz sections, each newest-first; existing
      smoke/fuzz output is preserved and covered by regression tests.
- [ ] ADR-054 and its entry in `docs/adr/INDEX.md`.
- [ ] Contract tests for schema, path safety, host binding, matrix, lock,
      crash/incomplete recovery, finalizer, aggregate, and one-writer guard.
- [ ] AO2.7 and AO2.8 metadata/acceptance rows updated to require AO2.9
      finalizer publication.

## Acceptance criteria

- [ ] `begin_run` requires the canonical evidence branch and an open PR into
      `integrate/phase-ai-31-33`; a run is not called published before that
      PR reaches the Pages publisher.
- [ ] A pushed `.pending/<run_id>.json` intent exists before any build or
      measurement and remains flaggable after `SIGKILL`, crash, or partition.
- [ ] A second target is refused while any pending marker is outstanding;
      the refusal is covered by a failed-push test.
- [ ] Status is machine-classified: completed threshold failures are `FAIL`,
      missing lifecycle phases are `INCOMPLETE`, and neither can be relabelled
      to evade review.
- [ ] Stable manifest-bound host labels reject per-attempt identity minting.
- [ ] Every PASS, FAIL, and INCOMPLETE attempt emits immutable JSON, XHTML,
      envelope, and raw evidence under the canonical host/run path.
- [ ] The aggregate retains all historical FAIL/INCOMPLETE records, flags
      intent-only runs, displays missing targets, and links all evidence.
- [ ] The generated root `site/reports/index.html` contains browsable
      benchmark, smoke, and fuzz sections in the generator-defined category
      order, each sorted
      newest-first; smoke/fuzz category and ordering behavior is verified
      against the existing generator while nested benchmark links are tested.
- [ ] CI prevents a second result writer/finalizer and validates nested
      envelope paths and generated indexes.
- [ ] AO2.7 and AO2.8 each have a checked `published via AO2.9 finalizer`
      acceptance row and `must_follow: AO2.9` metadata.
- [ ] ADR-054 is reviewed and the residual trust risk is explicit.
- [ ] Quality-mgr reviews this corrected plan before implementation work or
      physical benchmark execution is dispatched.

## Test files and commands

The implementation must add or update these named tests:

- `.just/tests/test_benchmark_report.py` — template variables, schema/status,
  failed/incomplete rendering, and immutable result writes.
- `.just/tests/test_benchmark_publication.py` — intent push, two-phase lock,
  branch/PR gate, host binding, failed-push recovery, and crash classification.
- `.just/tests/test_benchmark_publication_boundary.py` — exactly one result
  writer/finalizer and no alternate `git push` site publisher.
- `.just/tests/test_benchmark_matrix.py` — macOS/Linux four targets and
  Windows three-target rejection of UDS.
- `.just/tests/test_generate_report_index.py` — nested paths, intent-only
  incomplete rows, envelope safety, aggregate links, stale-index checks, and
  a root-index fixture containing benchmark/smoke/fuzz entries that asserts
  category sections plus newest-first ordering for the existing smoke/fuzz
  producers.
- `scripts/smoke/test_run_admission_capacity.py` — existing runner behavior.

Required commands are:

```text
python3 -m unittest .just/tests/test_benchmark_report.py
python3 -m unittest .just/tests/test_benchmark_publication.py
python3 -m unittest .just/tests/test_benchmark_publication_boundary.py
python3 -m unittest .just/tests/test_benchmark_matrix.py
python3 -m unittest .just/tests/test_generate_report_index.py
just reports-index --check
just lint
```

No physical M5 or Windows execution is required to complete AO2.9 tooling;
those runs belong to AO2.7/AO2.8 after this plan and implementation merge.

## References

- `templates/benchmark-report/benchmark-run.xhtml.j2`
- `templates/benchmark-report/benchmark-report.html.j2`
- `scripts/smoke/benchmark_report.py`
- `scripts/smoke/benchmark_schema.py`
- `.just/generate_report_index.py`
- `Justfile` (`benchmark`, `reports-index`, and the finalizer wrapper)
- `tools/benchmark-hosts.toml`
- `docs/adr/ADR-044-public-verification-report-classification.md`
- `docs/adr/ADR-052-benchmark-account-isolation-and-snapshot-policy.md`
- `docs/adr/ADR-054-benchmark-report-finalizer-trust-model.md`
- `docs/github-pages.md`
- `docs/plans/phase-ao2/sprint-AO2-7-m5-tcp-benchmark-parity.md`
- `docs/plans/phase-ao2/sprint-AO2-8-windows-tcp-benchmark-parity.md`
