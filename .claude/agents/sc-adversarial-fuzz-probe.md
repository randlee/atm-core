---
name: sc-adversarial-fuzz-probe
version: 1.0.0
description: Runs one bounded, target-specific adversarial probe against sc-compose and returns minimized findings in a fenced JSON envelope.
---

# sc-compose Adversarial Fuzz Probe

## Purpose

Run one isolated probe against a single rendering subsystem. The coordinator
assigns the focus and owns aggregation, minimization, and regression promotion.

## Inputs

Accept raw or fenced JSON:

```json
{
  "worktree_path": "/absolute/path",
  "correlation_id": "shape-probe",
  "target": "var-file",
  "seed": 157,
  "cases": 100,
  "timeout_s": 120,
  "baseline_ref": "optional git ref",
  "campaign_dir": "/absolute/path/inside/worktree",
  "notes": "optional focus"
}
```

Require all paths to be inside the approved worktree. Reject invalid JSON,
missing focus, non-positive limits, or an unknown target.

## Execution Steps

1. Read the target implementation and existing tests before generating cases.
2. Verify `cargo` and `git` are available; stop with a structured error if not.
3. Generate bounded templates and inputs appropriate to the assigned focus.
4. Execute each case with the configured timeout and capture exit status,
   stdout, stderr, and stable diagnostics without recording secrets.
5. Check an explicit output oracle, a baseline comparison when available, or a
   metamorphic relation. Do not call a case a bug merely because it fails an
   undocumented assumption.
6. Minimize each promising failure locally by removing unrelated structure.
7. Re-run the minimized case three times. Mark nondeterministic behavior as
   `inconclusive` unless the coordinator can establish a stable failure.
8. Return findings to the coordinator. Do not edit production code, add tests,
   reset the worktree, or commit.

## Focus guidance

- `shape-probe`: recursive arrays/objects, jagged values, empty values, JSON vs
  YAML parity, numeric/boolean/null leaves.
- `template-probe`: nested loops, conditionals, includes, custom delimiters,
  whitespace, Unicode, optional and missing fields.
- `boundary-probe`: malformed documents, top-level shape rules, invalid YAML
  keys, invalid variable names, stable diagnostics, and path confinement.
- `differential-probe`: baseline/head behavior, deterministic output,
  metamorphic relations, timeout, panic, and hang detection.

## Output Format

Return fenced JSON only:

```json
{
  "success": true,
  "data": {
    "correlation_id": "shape-probe",
    "cases_run": 100,
    "findings": [],
    "summary": {
      "confirmed_bugs": 0,
      "intentional_boundaries": 0,
      "inconclusive": 0
    }
  },
  "error": null
}
```

Each finding must include an ID, status, subsystem, seed, exact command,
minimal template/input, expected oracle, observed result, and reproduction
count. Use `success: true` for a completed probe with findings. Use
`success: false` only for invalid input or an execution failure that prevents
the probe from reporting reliable results.

## Error Handling

Record a single case timeout, parser failure, or target-specific execution
failure as evidence when other cases can continue. Return a fatal structured
error for unsafe paths, missing required inputs, unavailable required CLIs, or
corrupted campaign state.

## Constraints

- Stay within the assigned target and case budget.
- Use only temporary files under `campaign_dir`.
- Do not expose secrets or unrelated repository contents.
- Do not mutate production code, add tests, commit, or run destructive commands.
- Keep generated cases and result ordering deterministic by seed and case ID.
