# Sprint AO2.13 — Canonical Benchmark-Run Skill and Operator Workflow

Status: draft · Branch: `feature/ao2-13-benchmark-run-skill` off
`integrate/phase-ao2` (after AO2.11 merge-forward) · PR target:
`integrate/phase-ao2`
recommended_agent: Cipher-311d · recommended_model: fast

Fixes audit finding A.1 (no canonical procedure; five sprint docs with
diverging lifecycles left to agent interpretation). Implements D4, D9, D10.
After this sprint, "how to run and publish a benchmark" has exactly one
authoritative source; agents follow it mechanically.

## Deliverables

1. **Repo skill** `.claude/skills/benchmark-run/SKILL.md` — the single
   canonical procedure, superseding the procedural text in AI.40, AI.49,
   AI.52, AL.9, AO2.5.4, AO2.7, and AO2.8 (those docs remain as history; the
   skill states the supersession and each of those seven docs gains a
   one-line pointer to the skill). The skill covers, for macOS and Windows:
   - Preconditions: dedicated benchmark account (ADR-052), daemon-switch
     status check, clean git state, current branch expectations.
   - The run: exact command (`just benchmark`), required environment
     (`ATM_CAPACITY_HOST_LABEL` values per host, e.g. `rand-m5`,
     `windows-x64-01`), OS target matrix (Windows: no uds).
   - Publication: `git add` of the per-target JSON, `.campaign.json`,
     panels, phase report, index, envelopes; commit message convention;
     push; `just reports-index --check` must pass before push.
   - INCOMPLETE runs (D4): still rendered, committed, and pushed with the
     reason note; never deleted ad hoc; never counted as evidence.
   - Review step (D9): run `just benchmark-show` and display the newest
     panel in wyvern for the operator immediately after every run — this is
     a required step, not optional.
   - Failure handling: below-baseline is a FAIL result to publish, not a
     reason to withhold (memory: benchmark-ledger publish gap — every
     attempt is committed); harness errors before measurement are fixed and
     rerun silently, only measured results are reported.
2. **Justfile completion:** `just benchmark` performs the full flow
   (build → sign → run → render → index-check) and `just benchmark-show`
   (from AO2.11) is documented; a new `just benchmark-publish` recipe stages
   exactly the report artifacts and runs `just reports-index --check`,
   so the manual step list in the skill is two commands + push.
3. **Docs alignment:** `docs/cross-platform-guidelines.md` gains the
   Windows-benchmark specifics currently only in AI.52 (host label, TCP-only,
   native invocation, symlink daemon-switch); the superseded sprint docs
   (deliverable 1's seven-doc list) get their pointer lines; `docs/plans/phase-ao2/README` (or phase index, if
   present) links the skill.

## Skill outline (normative structure)

```markdown
# benchmark-run
1. Preflight (account, daemon-switch status, git state)
2. Run: ATM_CAPACITY_HOST_LABEL=<host> just benchmark
3. Review: just benchmark-show   # wyvern displays newest panel (required)
4. Publish: just benchmark-publish && git commit ... && git push
5. INCOMPLETE handling
6. Failure classification and rerun policy
7. Windows appendix (matrix, env, daemon-switch symlinks)
```

## Acceptance criteria

1. The skill contains every command an agent needs, copy-pastable, for both
   OSes; no step says "as appropriate" or defers to another document for a
   command (req-qa can verify by enumerating steps 1–7 against the
   deliverable list).
2. `just benchmark-publish` stages only report artifacts (test: dirty
   unrelated file is not staged) and fails when `reports-index --check`
   fails.
3. Each of the seven superseded docs (AI.40, AI.49, AI.52, AL.9, AO2.5.4,
   AO2.7, AO2.8) contains the pointer line (grep gate:
   `benchmark-run/SKILL.md` appears in all seven).
4. A dry-run transcript of the full skill executed on macOS (steps 1–4
   against a real run) is committed as sprint evidence.
5. `.just/tests` green on macOS and Windows CI lanes.

## Required validation

- Live macOS end-to-end: one real `just benchmark` following the skill text
  verbatim, including the wyvern display step (live-verify gate before
  quality-mgr dispatch).
- If no Windows operator slot is available this sprint, Windows verification
  is: (a) the Windows CI lane green on `.just/tests` for this branch, and
  (b) a named reviewer's step-by-step walkthrough of the skill's Windows
  appendix recorded as a PR review comment confirming every command is valid
  for `os.name == "nt"`. A live Windows execution is then recorded as an
  explicit follow-up validation row in this doc — not silently waived.

## Non-closure / out of scope

- Automated scheduled benchmark runs (not requested).
- Deleting INCOMPLETE artifacts (future cleanup).
- Rewriting the seven superseded sprint docs beyond the pointer line. (The
  AO2.5.4 snapshot/restore *mechanism* remains mandatory and unchanged; only
  its operator-procedure text is superseded by the skill.)

## Dependencies

- must_follow: AO2.11 (documents its `just benchmark-show` and template
  outputs). Merge-forward trigger: AO2.11 dev push; AO2.11 PR merges first.
- parallel_safe: AO2.12 (disjoint files; see AO2.12 doc).
