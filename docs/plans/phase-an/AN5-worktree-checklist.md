# AN.5 execution checklist

This is the worktree-local closure checklist for `feature/pan-s5-search-infrastructure`.
It is intentionally kept with the sprint evidence so a reviewer can distinguish completed
implementation from inherited AN.2/AN.4 work.

- [x] Merge the current `integrate/phase-an` line (including AN.4 QA fixes).
- [x] Add validated, sealed, backend-neutral search DTOs and synchronous/async capabilities.
- [x] Record the authorized capability and SQLite implementation/test-double boundary manifests.
- [x] Create external-content message and template FTS5 projections, including historical backfill.
- [x] Make every message/template/decomposed admission transaction synchronize its projection.
- [x] Implement deterministic recursive JSON variable flattening and validate projection scope.
- [x] Implement typed FTS/query/filter/aggregate/cursor semantics in the SQLite adapter.
- [x] Provide a fake contract implementation and fake/SQLite parity coverage.
- [x] Provide the backend-owned async reader lane with deadline/cancellation coverage.
- [x] Provide recovery rebuild (`reindex-search`) and prove rebuild/backfill equivalence.
- [x] Run mutation/interleaving drift coverage, the live FTS snippet/highlight spike, and negative validation cases.
- [x] Re-read the AN.5 plan, record and close any second-pass gaps.
- [x] Run `just test`, then send the final commit to team-lead.

## Critical re-audit — 2026-08-12

The first closure pass established the capability. This independent pass compares
the delivered behavior to every explicit AN.5 contract rather than treating the
previous checkmarks as evidence.

- [x] Make the authorized in-memory fake evaluate the complete typed contract:
  expression, structured filters, aggregate grouping, deduplication, and stable
  cursor continuation.
- [x] Move structured `tags`, `vars`, and template-metadata filters into the
  rusqlite adapter's private JSON1 query compilation, retaining no public SQL
  surface.
- [x] Carry an async caller's absolute deadline into the bounded reader lane so
  queued work that has already expired is not executed after cancellation.
- [x] Add deterministic regression coverage for every FTS projection column,
  equal-timestamp cursor continuation, per-mailbox/default dedup, and
  backfill-versus-rebuild equivalence including template projections.
- [x] Re-read the sprint after the fixes; run the full `just test` gate, then
  commit, push, open the PR, and send the team-lead completion report.
