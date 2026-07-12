---
id: AD.20
title: Read Body-Search Metadata Consistency Repair
status: complete
branch: feature/pAD-s20-read-body-search-metadata-consistency-repair
worktree: ../atm-core-worktrees/feature/pAD-s20-read-body-search-metadata-consistency-repair
target: integrate/phase-AD
---

# Sprint AD.20 — Read Body-Search Metadata Consistency Repair

## Goal

- make metadata-backed `atm read --contains` and matching selector flows honor
  the documented full-body search contract instead of silently degrading to
  summary-only text

## Hard Dependencies

- `AD.11` complete
- `AD.18` complete
- `AD.19` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/violation-inventory.md`

## Exact Targets

- `crates/atm-core/src/read/filters.rs`
- `crates/atm-core/src/read/metadata_selection.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-storage-rusqlite/src/mailbox_metadata.rs`
- `crates/atm-core/tests/mailbox_locking.rs`
- `scripts/smoke/run.py`
- `scripts/smoke/run_thorough.py`
- `docs/atm-rusqlite/query-diagrams.md`
- `docs/atm-rusqlite/sql_query-mailbox-metadata-rows.mmd`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`

## Interfaces To Add Or Modify

The accepted metadata-backed contains-filter contract after this sprint is:

```rust
pub struct MetadataBackedReadSelection {
    pub summary_text: Option<String>,
    pub message_text: Option<String>,
}
```

with these invariants:

- `--contains` matches both summary text and full durable message body text
  even when the bounded metadata path is used
- metadata-backed selector evaluation must not reconstruct a fake body from
  summary text and then treat that degraded projection as equivalent to the
  durable message body
- if the bounded metadata row cannot satisfy the documented full-body search
  contract by itself, the implementation must fetch enough durable body detail
  to preserve correctness before final contains-filter selection
- boundedness rule: body fetch is forbidden for rows whose summary already
  matches the needle and forbidden for rows rejected by earlier metadata-only
  filters; body fetch is permitted only for rows that survive metadata-only
  filters and still need a durable-body check to satisfy the documented
  `--contains` contract

## Paths To Delete

- metadata-path behavior that reconstructs `InboxMessage.text` from summary-only
  data and then applies the documented `--contains` filter as if full body text
  were present
- any smoke/test expectation that treats summary-only contains matching as an
  acceptable substitute for full durable message body matching

## Deliverables

- `atm read --contains <needle>` can find a message whose durable body contains
  the needle even when the summary does not
- metadata-backed contains filtering preserves the accepted bounded fetch
  pattern by limiting durable-body fetches to surviving candidate rows that
  still need a body-only contains check
- regression coverage proves summary-only and body-only matches both behave
  correctly on the accepted read path

## This Sprint Does Not Close

- read-state mutation semantics already owned by `AD.19`
- raw CLI runtime-root behavior beyond consuming the `AD.18` contract
- graft boundary reset

## Acceptance Criteria

- a targeted contains-filter regression test proves:
  - `atm read --contains <needle>` returns a message when the summary contains
    the needle
  - `atm read --contains <needle>` also returns a message when only the durable
    body contains the needle
  - metadata-backed selection does not return a false negative merely because
    the bounded row lacked body text
- a targeted boundedness regression test proves:
  - rows rejected by earlier metadata-only filters do not trigger durable-body
    fetch
  - rows whose summary already matches do not trigger durable-body fetch just
    to re-check the same needle
  - durable-body fetch occurs only for surviving candidate rows whose summary
    did not already satisfy the `--contains` contract
- targeted regression coverage proves list/read selector behavior remains
  consistent for summary-only versus body-only matches
- `docs/atm-rusqlite/query-diagrams.md` and
  `docs/atm-rusqlite/sql_query-mailbox-metadata-rows.mmd` describe the updated
  metadata-query-plus-selective-body-fetch pattern accurately
- `docs/requirements.md`, `docs/architecture.md`,
  `docs/atm-core/requirements.md`, and `docs/atm-core/architecture.md`
  describe `--contains` as a full-body-correct selector even on metadata-backed
  read/list paths

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted contains-filter regression coverage
- targeted metadata-row/body-fetch regression coverage
- targeted boundedness/query-pattern regression coverage
- `just smoke normal`
- `git diff --check`
