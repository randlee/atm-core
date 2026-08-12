# AN.6 worktree closure checklist

This file is the implementation checklist for `feature/pan-s6-query-surface`.
It is intentionally checked only when the implementation and its regression
coverage are both present.  The frozen daemon is explicitly out of scope;
every network exercise below uses the Tokio/Axum `atm-http-runtime` path.

## First pass — plan-to-code inventory

- [x] Define the core, transport-neutral query/introspection contracts and
  one bounded parser from CLI/HTTP primitives to AN.5 `MessageSearchQuery`.
- [x] Install the search and template-catalog capabilities through runtime
  assembly and make peer search reject before capability selection.
- [x] Add `atm templates list` / `atm templates schema` with text and JSON
  renderers, exact-SHA schema lookup, and type-filter coverage.
- [x] Add `atm search`, including literal versus `--raw-match` parsing, every
  generic filter, aggregations, cursor/per-mailbox semantics, and text/JSON
  renderers.
- [x] Register the core-owned `GET /v1/atm/messages/search` codec/route and
  make the Axum runtime a thin async adapter.
- [x] Document and snapshot the public HTTP and query contracts.
- [x] Add the separate local-only `atm-query-python` Maturin crate with
  SQLite read-only, authorizer, single-statement, and resource-budget gates.
- [x] Add corpus/parity, parser/key/injection, aggregate/cursor, local/peer,
  and Python security acceptance tests.

## Second pass — closure review

- [x] Re-read the sprint plan against the completed diff and close uncovered
  requirements before declaring the sprint ready.
  - The second pass found and closed two omissions: serialization coverage now
    round-trips every simple aggregate form, and the Python fixture proves the
    three documented analyst queries against the stable view using bound
    parameters.
  - Confirmed no raw SQL crosses CLI, HTTP, peer, `atm-core`, or graft; peer
    rejection precedes search-store selection; the frozen daemon remains
    untouched.
- [x] Run formatter, focused tests, and the complete `just test` suite.
  - Passed `cargo fmt --all --check`, targeted clippy with `-D warnings`, and
    `CARGO_TARGET_DIR=/tmp/atm-an6-clean-target just test`.
