# Templates

This folder contains `sc-compose` templates for crate architecture ADRs,
boundary inventories, and sprint-plan documents.

Recommended document layout per crate:
- `docs/<crate>/architecture.md`
  - hand-written crate architecture document
  - contains one or more rendered ADR records from `architecture-adr.md.j2`
- `docs/<crate>/boundaries.md`
  - hand-written boundary inventory document
  - contains one rendered boundary record per major trait/facade from
    `boundary-record.md.j2`

The intended default workflow is:
- keep crate documents mostly hand-written
- render one ADR record per major architectural decision
- render one boundary record per major trait/facade
- paste those records into the crate-local docs

This keeps the docs readable while giving the parser and lint tooling a stable,
machine-authoritative schema.

## Sprint Plan Template

Use `sprint-plan.md.j2` for one sprint-scoped execution plan when a phase has
multiple remaining implementation sprints.

This template is for:
- one worktree
- one phase
- one sprint id expressed as a string
- one concrete list of sub-tasks

The sprint id must stay a string so values like `R.13`, `R.10.3-FIX-R6`, or
`PR-PLANNING-MF-R1` remain exact.

Each sprint plan should include:
- governing requirements
- governing ADRs
- governing boundaries
- prerequisites
- hard dependencies
- non-goals
- concrete sub-tasks
- split recommendation
- acceptance criteria
- validation commands
- required document updates
- risks and watchouts

## Boundary Record Template

Use `boundary-record.md.j2` inside `docs/<crate>/boundaries.md`.

The YAML block is the authoritative machine-readable contract for:
- public trait/facade
- concrete implementation
- visibility
- composition roots
- forbidden dependencies and references
- lint/review enforcement

Semantics:
- `owner_package` is the Cargo package name, e.g. `atm-rusqlite`
- `owner_crate_path` is the Rust crate/module path root, e.g. `atm_rusqlite`
- `allowed_dependents` means crates allowed to depend on the owner package
- `allowed_dependencies` means crates the owner package may depend on directly
- `references.scope` should normally be `outside_owner_crate`, so forbidden
  references are interpreted as external-bypass checks rather than global bans
- `implementation.visibility: private` and `implementation.constructor: private`
  mean no public struct, no public constructor, and no public re-export

### Render One Boundary Record

```bash
_VARS=$(mktemp)
cat > "$_VARS" <<'JSON'
{
  "boundary_name": "MailStore",
  "boundary_id": "BOUNDARY-MailStore",
  "owner_package": "atm-rusqlite",
  "owner_crate_path": "atm_rusqlite",
  "public_trait": "MailStore",
  "public_facade": "null",
  "impl_type": "SqliteMailStore",
  "impl_module": "atm_rusqlite::mail_store",
  "impl_visibility": "private",
  "impl_constructor": "private",
  "composition_roots": "    - atm_app::compose",
  "io_owns": "    - sqlite",
  "io_forbidden": "    - config_json\n    - sockets\n    - process_spawn",
  "allowed_dependents": "    - atm-app",
  "allowed_dependencies": "    - atm-core\n    - rusqlite",
  "forbidden_edges": "    - atm -> atm-rusqlite\n    - atm-daemon -> atm-rusqlite",
  "references_scope": "outside_owner_crate",
  "forbidden_references": "    - SqliteMailStore\n    - SqliteMailStore::open\n    - rusqlite::Connection",
  "request_types": "    - MailStore method inputs",
  "response_types": "    - atm-core store DTOs",
  "error_types": "    - AtmError",
  "allowed_test_double_paths": "    - atm_core::test_support::InMemoryMailStore",
  "forbidden_test_bypasses": "    - rusqlite::Connection",
  "lint_rules": "    - LINT-BOUNDARY-001\n    - LINT-BOUNDARY-002",
  "review_gates": "    - no_public_impl\n    - no_public_constructor\n    - no_forbidden_imports",
  "state": "planned",
  "status_notes": "    - none",
  "purpose": "- Owns durable mailbox state access.",
  "notes": "- Used by lint tooling as the crate-local source of truth."
}
JSON

sc-compose render \
  --root docs/templates \
  --file boundary-record.md.j2 \
  --var-file "$_VARS"

rm -f "$_VARS"
```

## Architecture ADR Template

Use `architecture-adr.md.j2` inside `docs/<crate>/architecture.md`.

The YAML block is the authoritative machine-readable record for:
- decision identity and status
- related boundaries
- code/module references
- downstream follow-up work

### Render One ADR Record

```bash
_VARS=$(mktemp)
cat > "$_VARS" <<'JSON'
{
  "adr_title": "Strict trait boundaries are mechanically enforced",
  "adr_id": "ADR-ATM-RUSQLITE-001",
  "crate": "atm-rusqlite",
  "status": "accepted",
  "date": "2026-05-04",
  "deciders": "  - team-lead\n  - arch-ctm",
  "tags": "  - boundaries\n  - lint\n  - privacy",
  "related_boundaries": "  - BOUNDARY-MailStore\n  - BOUNDARY-TaskStore\n  - BOUNDARY-RosterStore",
  "code_references": "  - crates/atm-rusqlite/src/lib.rs\n  - crates/atm-rusqlite/src/mail_store.rs",
  "context": "- Earlier SQLite/daemon drift showed that prose-only architecture was not enough to prevent concrete SQLite leakage.",
  "decision": "- All crate boundaries must be documented in machine-parsable records, and concrete implementations remain private behind the documented trait/facade.",
  "consequences": "- Lint and review can enforce forbidden references directly from crate-local boundary records.",
  "alternatives_considered": "- Keep boundary rules only in top-level architecture prose.\n- Infer boundary contracts from code instead of documenting them explicitly.",
  "follow_up_work": "- Populate `docs/atm-rusqlite/boundaries.md` for each major boundary.\n- Wire lint checks to read those records."
}
JSON

sc-compose render \
  --root docs/templates \
  --file architecture-adr.md.j2 \
  --var-file "$_VARS"

rm -f "$_VARS"
```

### Notes

- The YAML block inside each rendered record is the authoritative machine
  contract for parsers and tooling.
- The prose below the YAML is for human explanation and migration notes.
- The older `boundary-section.md.j2` and `crate-boundary-architecture.md.j2`
  templates are retained as transitional scaffolds, but the preferred pattern
  is `architecture.md` + `boundaries.md` with one rendered record per section.
