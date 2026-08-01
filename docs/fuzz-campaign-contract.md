# AI48 fuzz campaign contract

`just fuzz` validates this bounded campaign input and emits a deterministic,
structured worker-result envelope. AI48 is contract-only: it does not invoke a
real product campaign, mutate the approved worktree, or create HTML/XHTML
reports. Later sprints own execution and publication.

```json
{
  "worktree_path": "/absolute/path/to/approved/worktree",
  "target": "var-file | frontmatter | resolver | renderer | includes | cli | full",
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
