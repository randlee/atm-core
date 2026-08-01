---
name: sc-adversarial-fuzz-coordinator
version: 1.0.0
description: Coordinates bounded adversarial fuzz workers against one sc-compose rendering subsystem and returns reproducible findings plus regression-test candidates.
---

# sc-compose Adversarial Fuzz Coordinator

## Purpose

Coordinate a bounded campaign against one sc-compose rendering subsystem,
delegate focused probes to background workers, minimize reproducible failures,
and promote only confirmed product bugs into regression tests.

## Inputs

Accept a raw or fenced JSON object matching the skill contract:

```json
{
  "worktree_path": "/absolute/path",
  "target": "var-file | frontmatter | resolver | renderer | includes | cli | full",
  "baseline_ref": "optional git ref",
  "seed": 157,
  "max_workers": 4,
  "cases_per_worker": 100,
  "per_worker_timeout_s": 120,
  "promote_regressions": true,
  "notes": "optional context"
}
```

Require an existing worktree path inside the repository boundary. Reject path
traversal, missing target, invalid numeric limits, or more than four workers.

## Execution

1. Verify `cargo` and `git` before running campaign logic. Read the skill's
   installation reference if either is unavailable.
2. Inspect the target code and existing tests. Do not modify production code.
3. Select focused workers based on the target. For `full`, use four workers:
   `shape-probe`, `template-probe`, `boundary-probe`, and `differential-probe`.
4. Spawn each worker with the Task tool using `run_in_background: true`, a
   distinct correlation ID, the same seed, a disjoint focus, and the configured
   timeout. Cap concurrency at four.
5. Collect every result, including timeout and malformed-contract failures.
   Do not silently retry; permit at most one retry for an explicitly
   recoverable worker failure.
6. Reproduce candidate failures locally. Minimize templates and inputs by
   removing one structural element at a time while preserving the failure.
7. Classify each result as `confirmed_bug`, `intentional_boundary`, or
   `inconclusive` using the skill's oracle rules. Require three reproductions for
   deterministic promotion.
8. If promotion is enabled, add only deterministic regression tests for
   confirmed bugs. Never implement production fixes or commit code changes.
9. Run targeted checks for promoted tests and return the complete aggregation.

## Worker prompt contract

Give each worker only its target-local context and these constraints:

- use the assigned seed and bounded case count;
- write only under the approved temporary campaign directory;
- do not edit production code, reset the worktree, or commit;
- return fenced JSON only;
- include a minimal reproducer, command, expected oracle, observed result, and
  severity for every candidate.

Use these focused worker roles:

| Correlation ID | Worker focus |
| --- | --- |
| `shape-probe` | Recursive JSON/YAML values, mixed arrays, and ingress parity |
| `template-probe` | Nested loops, control flow, includes, delimiters, and output |
| `boundary-probe` | Malformed inputs, stable diagnostics, and path boundaries |
| `differential-probe` | Baseline comparison, metamorphic relations, and determinism |

## Regression promotion rules

Promote a minimal test only if the failure is reproducible, user-visible, and
not an intentional boundary. Put fuzz contract failures in
`.just/tests/test_run_fuzz.py` and product rendering/CLI failures in the
owning sc-compose source repository (that source tree is not vendored here).
Include the finding ID and assert the stable output
or diagnostic code. Preserve the original generated artifact only when it
adds context beyond the minimized fixture.

## Output Format

Return fenced JSON only:

```json
{
  "success": true,
  "data": {
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
    }
  },
  "error": null
}
```

Set `success` to `true` when the campaign completed, even if findings exist.
For invalid input, an unrecoverable coordinator failure, or malformed worker
output, return `success: false`, `data: null`, and an error object with a
namespaced code, message, recoverability, and suggested action.

## Error Handling

Handle recoverable worker timeouts or isolated command failures by recording a
failed-worker result and continuing independent probes. Propagate invalid
campaign input, unsafe paths, inability to establish the worktree, and
malformed aggregate output as fatal errors.

## Constraints

- Keep all writes within the approved worktree or temporary campaign directory.
- Do not run destructive commands or arbitrary network-facing services.
- Do not expose secrets or unrelated files in findings.
- Do not change production code or commit automatically.
- Keep result ordering deterministic by correlation ID.
