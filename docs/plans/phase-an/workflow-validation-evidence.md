# AN.12 workflow-metadata validation evidence

This is the retained evidence ledger for the optional, local workflow-metadata
extension. It supplements the historical AN.8 ledger; it neither rewrites
AN.8 evidence nor claims a physical cross-host workflow lane.

## Fixture corpus

[`fixtures/workflow-metadata-evidence.json`](./fixtures/workflow-metadata-evidence.json)
contains two deliberately unrelated families:

| Family | Literal vocabulary | Hand-computed result |
| --- | --- | --- |
| `release-train` | `queued` → `shipped`, `prepare`/`release`, `enter`/`exit` | 420,000 ms completed cycle; one incomplete second iteration |
| `field-operations` | `mobilized` → `secured`, `dispatch`/`recovery`, `begin`/`finish` | 210,000 ms completed cycle |

`retained_unrelated_vocabulary_fixtures_prove_generic_lifecycle_facts` parses
the checked-in corpus and verifies every listed duration, incomplete fact,
iteration count, applied-template tag, and canonical effective-tag projection
through the same generic pairing and provenance implementation. It also
matches the fixture's hand-computed count map for the four and only four
permitted aggregate dimensions: `scope_kind`, `state`, `stage`, and
`transition`; `scope_id` and `iteration` remain filters. The test knows no
process-specific vocabulary.

## Same-host, local-only path

The following bounded interfaces are exercised together by the retained tests:

| Surface | Evidence |
| --- | --- |
| Template registration and atomic admission | `retained_an12_fixture_admits_two_unrelated_workflow_vocabularies` and `workflow_admission_persists_canonical_snapshot_and_tag_provenance` in `atm-storage-rusqlite` |
| Immutable revision and mismatch rejection | `workflow_template_revisions_preserve_prior_snapshot_and_provenance` and `workflow_admission_rejects_mismatched_projection_without_mutation` in `atm-storage-rusqlite` |
| CLI lifecycle filter/projection | `lifecycle_cli_surface_compiles_a_generic_projection` and `lifecycle_projection_inherits_the_search_time_window` in `agent-team-mail` |
| Local HTTP/core lifecycle response | `local_search_exposes_requested_lifecycle_projection` in `agent-team-mail-core` and `search_route_decodes_the_shared_core_request_contract` in `atm-http-runtime`; the HTTP decoder carries a nontrivial opaque lifecycle selector through the same `SearchRequest`/`SearchResponse` contract |
| Read-only Python query | `an12_python_surface_uses_parameterized_workflow_scope_and_tag_provenance` in `atm-query-python` |
| Telemetry no-op/configured/failing sink isolation | `disabled_telemetry_is_inert_and_has_no_worker_side_effects`, `configured_sink_receives_only_the_redacted_record_contract`, and the remaining `workflow_telemetry::tests` in `atm-runtime` |

Reproducible local commands and the assertions they must satisfy:

```sh
cargo test -p agent-team-mail-core retained_unrelated_vocabulary_fixtures_prove_generic_lifecycle_facts
cargo test -p agent-team-mail --bin atm commands::search
cargo test -p atm-query-python an12_python_surface_uses_parameterized_workflow_scope_and_tag_provenance
cargo test -p atm-storage-rusqlite workflow_admission
cargo test -p atm-runtime workflow_telemetry::tests
```

The CLI contract is intentionally bounded and shares its time window with the
ordinary search query:

```sh
atm search --lifecycle-scope-kind release-train \
  --lifecycle-start-state queued --lifecycle-end-state shipped \
  --since 2026-08-10T00:00:00Z --until 2026-08-11T00:00:00Z --json
```

The expected lifecycle result contains only stored workflow facts and a
duration; it never contains rendered body text or merged template variables.
The Python fixture uses `WHERE workflow_scope_kind = ? AND
workflow_scope_id = ?`, demonstrating parameter binding over the stable
`decomposed_messages` view rather than an unbounded database handle.

## Compatibility and safety

- The AN.8 migration/reopen test proves an historical decomposed row has all
  AN.9+ snapshot/projection columns `NULL`; a `NULL` effective projection is
  not silently substituted with legacy `tags_json`.
- `template_routing_matrix_persists_only_same_team_same_host_as_decomposed`
  retains the four Tokio/Axum routing cells. Only same-team/same-host admits
  a decomposed workflow row; the other three remain rendered ordinary rows.
- `TemplateTagDeclaration` tests reject reserved derived-prefix spoofing.
- The telemetry tests cover queue capacity 1/default/4,096, drain deadlines
  1 ms/default/30 s, invalid configuration, full queues, timeouts, bounded
  shutdown, and a failing sink. Configured export JSON is asserted to contain
  no body, message text, or merged-variable field.

Physical Windows cross-host proof remains blocked by the VPN path and is not
inferred from this same-host evidence.
