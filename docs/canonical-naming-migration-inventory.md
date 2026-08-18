# Canonical naming migration inventory

This inventory records ATM Core's historical naming exceptions while the
canonical rules remain owned by the [Synaptic Canvas SSOT](https://github.com/randlee/synaptic-canvas/blob/develop/docs/ATM-NAMING-CONVENTIONS.md).
It is deliberately an inventory of repository locations and migration state,
not a second grammar.

| Area | Historical form observed | Canonical replacement | State / action |
| --- | --- | --- | --- |
| Phase AN plan filenames | `sprint-AN1-*.md`, `sprint-AN10-*.md` | `sprint-AN.1-*.md`, `sprint-AN.10-*.md` | Preserve published paths; use dotted IDs for new plans and migrate on an owning-doc edit. |
| Phase AN frontmatter | branch names such as `feature/pan-s8-*` and `feature/an12-*` | Retain the actual branch; add canonical `sprint: AN.<n>` metadata | Do not infer sprint identity from a branch; add/repair metadata when touched. |
| Worktree frontmatter | absolute paths and repository-relative paths mixed | `../<repo>-worktrees/<branch>` | Preserve historical evidence; new frontmatter uses the relative form. |
| TTL/report ingestion | `AN-S1`, compact `AN1`, or lowercase `an.1` | `AN.1` | Reject new non-canonical persisted values; validator reports the raw value and candidate. |

Migration is complete for an inventory row only when all new ingestion uses the
canonical value and the owning historical records have either been migrated
or explicitly retained as immutable compatibility evidence.  The validator
must continue to diagnose a legacy value when it appears in a new record.

The machine-readable exception list is
`.just/ttl-naming-legacy-allowlist.txt`.  It is intentionally path-scoped and
value-scoped: adding a new record with a legacy value in another path still
fails validation and requires an explicit migration decision.
