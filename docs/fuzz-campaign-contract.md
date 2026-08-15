# AI48 fuzz campaign contract

`just fuzz` validates this bounded campaign input and emits a deterministic,
structured worker-result envelope. The original AI48 targets are contract-only:
they do not invoke a product campaign or mutate an approved worktree. AN.15
adds the one exception, `atm-template-checked-emission`, which may be executed
with `--execute`: it runs four fixed Cargo-test seams in the approved worktree.
It never shells out to `sc-compose`, accepts a caller-provided command, or lets
a worker edit production code.

```json
{
  "worktree_path": "/absolute/path/to/approved/worktree",
  "target": "var-file | frontmatter | resolver | renderer | includes | cli | local-http-framing | atm-template-checked-emission | full",
  "baseline_ref": "optional git ref",
  "seed": 157,
  "max_workers": 4,
  "cases_per_worker": 100,
  "per_worker_timeout_s": 120,
  "promote_regressions": true,
  "notes": "optional target-specific context"
}
```

`worktree_path` must resolve inside the repository or an approved sibling
worktree, `max_workers` is capped at four, and all numeric limits are bounded.
Worker results retain timeout and malformed-result failures instead of
discarding them. Use `--campaign <path>` to validate a JSON file and
`--output <path>` to save the emitted envelope under the repository.

For the AN.15 execution lane, `max_workers` must be four and each fixed worker
uses the campaign's bounded case count and timeout. A nonzero test exit or a
timeout becomes a structured candidate (`worker_test_failure` or
`worker_timeout`); it is never silently converted to a pass. Real execution is
fail-closed for every other target.
