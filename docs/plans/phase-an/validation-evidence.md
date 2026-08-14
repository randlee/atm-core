# AN.8 validation evidence ledger

This ledger records the reproducible Phase AN validation. It intentionally
does not claim a physical cross-host template-sync protocol: that is a
non-goal of Phase AN.

For the AN.12 workflow-metadata extension, the governing references are the
[`Template Workflow Metadata` authoring guide](../../template-workflow-metadata.md),
[`ADR-046`](../../adr/ADR-046-template-declared-workflow-metadata.md),
[`requirements.md`](../../requirements.md), and the
[`AN.12 sprint contract`](./sprint-AN12-workflow-validation-evidence.md).

## Q1–Q4 and query boundary

`crates/atm-query-python/src/lib.rs` runs the exact SQL artifacts under
`fixtures/queries/` through `atm_query.open_readonly`. The `an8_fixture`
corpus contains decomposed task-assignment and QA-task-shaped records, with
the captured `dev-task` and `qa-task` public template names asserted by test.
The test verifies the hand-computed `expected-results.json` answers for:

| Query | Expected result |
| --- | --- |
| Q1 sprint span | `AN.1` 08:00–10:00; `AN.2` 08:00–11:00 |
| Q2 QA iterations | `AN.1` = 2; `AN.2` = 1 |
| Q3 severity rollup | AN.1: one Blocking, Minor, Important; AN.2: one Blocking |
| Q4 developer | `AN.1` = dev-alpha; `AN.2` = dev-beta |

The same test uses a separate `cycle`/`owner`/`risk` corpus, proving that
analogous span and rollup questions require only the generic
`decomposed_messages` plus JSON surface—not an ATM workflow type in core.
`ReadonlyDatabase` exposes no filesystem API; the query call opens the
SQLite database read-only and its authorizer permits only the public view and
schema introspection.

The historical `claude_inbox_tmpfile_parser.py` capture is retained as a
file-oriented baseline. AN.8's audit corrected the sprint's initial wording:
the captured helper parses and atomically writes a JSON inbox, but does not
produce Q1–Q4 answers. The replacement therefore takes its answers from
durable decomposed records rather than parsing an inbox file; it does not
claim impossible parser-answer equivalence. The actual AN.1 task template and
variables are separately exercised byte-for-byte by
`crates/atm/tests/compose_passthrough.rs`, which compares `atm compose` with
the pinned `sc-compose` CLI.

## Templated routing matrix

`template_routing_matrix_persists_only_same_team_same_host_as_decomposed` in
`crates/atm-http-runtime/src/storage_and_nudge_router.rs` dispatches the
Tokio runtime's four Decision-5 cells and reads SQLite only through
`atm-runtime-test-support`:

| Sender/recipient relationship | Durable result |
| --- | --- |
| same team, same host | one catalog registration and decomposed record |
| same team, cross host | rendered ordinary row; `template_sha` and `vars_json` NULL |
| foreign team, same host | rendered ordinary row; `template_sha` and `vars_json` NULL |
| foreign team, cross host | rendered ordinary row; `template_sha` and `vars_json` NULL |

The assertion additionally proves the three ordinary rows all preserve the
verified rendered body and that their sends do not create a catalog admission.
It is an ordinary Tokio/Axum runtime test—not a legacy daemon test—and runs
in the CI test matrix on Linux, macOS, and Windows.

## Required commands

```sh
cargo test -p atm-query-python --lib
cargo test -p atm-http-runtime template_routing_matrix_persists_only_same_team_same_host_as_decomposed
cargo test -p agent-team-mail --test compose_passthrough
just test
```

The branch diff must remain free of production changes in `crates/atm-core`;
the synthetic-vocabulary proof is satisfied by the test and reviewable diff.

## AN.12 workflow-metadata extension

AN.12's retained local proof is recorded in
[`workflow-validation-evidence.md`](./workflow-validation-evidence.md). It
adds two unrelated, hand-computed lifecycle/provenance fixture families and
exercises bounded CLI/HTTP, local read-only Python, atomic admission,
revision/migration compatibility, four-cell Tokio routing, and isolated
telemetry behavior. It does not claim a physical cross-host workflow proof.
