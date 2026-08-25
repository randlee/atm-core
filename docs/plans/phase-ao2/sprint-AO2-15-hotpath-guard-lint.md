# Sprint AO2.15 — Hot-Path Guard Lint + Evidence-Gated Diffs

Status: draft · Branch: `feature/ao2-15-hotpath-guard-lint` off
`integrate/phase-ao2` · PR target: `integrate/phase-ao2`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

First of three guardrail sprints (AO2.15/16/17, split per plan-scope round
1) motivated by 3–4 incidents of ~50% measured throughput loss and 1–2
hours of senior-dev recovery each. This sprint fences the **code path**:
changes inside declared hot-path regions are mechanically constrained and
evidence-gated. AO2.16 fences the measurement/comparison; AO2.17 handles
the sc-lint extraction and ops runbook.

## Deliverables

1. **Hot-path manifest** `boundaries/atm-core/hot-path-admission.toml`.
   Normative schema (sample is the contract):

```toml
schema_version = 1

[[region]]
name = "admission-commit-write"
file = "crates/atm-http-runtime/src/storage_and_nudge_router.rs"
sentinel = "HOT-PATH: admission"        # brackets: BEGIN/END pair in source
# lexically-enforced bans (literal-scan classes; see deliverable 3):
banned_tokens = ["dbg!", "println!", "std::fs::", "tracing::debug!",
                 "tracing::trace!", "spawn_blocking"]
# semantically-enforced bans (clippy classes; see deliverable 2):
clippy_denies = ["await_holding_lock", "await_holding_invalid_type"]

[[region]]
name = "storage-writer-batching"
file = "crates/atm-storage-rusqlite/src/writer/mod.rs"
sentinel = "HOT-PATH: writer"
banned_tokens = ["dbg!", "println!", "tracing::debug!", "tracing::trace!"]
clippy_denies = ["await_holding_lock"]
```

   Initial region list (which regions are protected) is a policy decision:
   quality-mgr signs off the manifest contents (Required validation).
2. **Semantic classes via clippy, not a hand-rolled scanner** (honesty
   about what literal scans can prove — plan-crit finding 008): guard-
   across-await detection uses `#![deny(clippy::await_holding_lock)]` /
   `await_holding_invalid_type` module-level attributes on the modules
   containing sentinel regions; the manifest's `clippy_denies` list is
   cross-checked by the lint test (attribute present in the named module).
   Literal scanning makes no guard-liveness claims.
3. **Lexical classes via literal-scan test** (new test in
   `crates/atm-architecture/tests/`, same idiom as
   `boundary_enforcement.rs`): inside each `BEGIN…END` sentinel span, any
   `banned_tokens` match fails with the region name and line. Also
   enforced: sentinel integrity (a BEGIN/END pair removed, unbalanced, or
   moved out of the manifest's file without a same-PR manifest change
   fails) and manifest/source agreement (every region resolves; every
   sentinel in source is manifested). Rustfmt note: line comments are not
   relocated by `cargo fmt`; a fixture asserts sentinel spans survive
   `cargo fmt --check`.
4. **Touch-triggers-evidence CI gate**: a job using the repo's existing
   `ci-scope`-style pattern — the job ALWAYS registers (required-check
   safe; workflow-level `on.paths:` filtering is explicitly forbidden for
   this job) and its steps no-op unless `git diff --name-only` against the
   base sha intersects a manifest region. When it triggers, the PR must
   carry either a `benchmark-evidence: <v4-campaign-id>` line in the PR
   description, or a **waiver that CI attributes to a real actor**: a PR
   comment exactly matching `benchmark-evidence-waiver: <reason>` whose
   `user.login` (fetched via the GitHub API in the job) is in the
   repo-committed approver list `boundaries/atm-core/evidence-waivers.toml`
   (initially quality-mgr's bot/operator login and Rand). A
   description-embedded waiver token is NOT accepted — self-service
   waivers were the round-1 blocking hole.

## Acceptance criteria

1. (D1/D3) Fixture tests: each banned-token class fails inside a region
   and passes outside; unbalanced/removed sentinel without manifest change
   fails; manifest region naming a missing file/sentinel fails; sentinel
   span survives `cargo fmt --check` (fixture).
2. (D2) The clippy deny attributes exist in every module the manifest
   lists, verified by the lint test; a fixture holding a `MutexGuard`
   across `.await` in a sentinel module fails workspace clippy.
3. (D4) Job-registration semantics: on a synthetic PR not touching any
   region, the job registers and passes as a no-op (never left pending);
   touching a region without evidence fails; with a campaign id passes;
   with a waiver comment from a login in the approver list passes; the
   same comment from a non-listed login fails (API attribution test using
   recorded fixtures).
4. All suites green on all three CI lanes.

## Required validation

- Live-verify on a scratch PR exercising the gate end-to-end (trigger,
  evidence pass, waiver pass, waiver-rejection) before quality-mgr
  dispatch.
- quality-mgr sign-off on the initial manifest region list and the
  waiver-approver list.

## Non-closure / out of scope

- Measurement/comparison guardrails (AO2.16); sc-lint extraction and
  nightly runbook (AO2.17).
- Extending regions beyond admission/writer (future manifest revisions,
  quality-mgr-gated like baselines).

## Dependencies

- must_follow: AO2.14 (PR #1020) merge — sentinel placement lands on the
  post-pooling shape of the router/client files. PR-completion trigger.
- parallel_safe: AO2.16, AO2.17 (disjoint files; AO2.16 touches only the
  Python benchmark pipeline + benches/, AO2.17 only docs/sc-lint proposal).
