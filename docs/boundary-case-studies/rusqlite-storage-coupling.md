# Case Study: rusqlite Leaking Above the Storage Boundary

**Source**: `RUSQLITE-CORE-COUPLING-001` (`.triage/phase-AD/findings/RUSQLITE-CORE-COUPLING-001.ttl`,
phase-AD, triaged 2026-07-10, closed 2026-07-11) and the earlier, narrower
`ATM-QA-BOUNDARY-001` (`.triage/phase-T/findings/ATM-QA-BOUNDARY-001.ttl`,
phase-T, triaged 2026-05-10). Fix commits verified in `git log`:
`03acab1c` (Converge SQLite backend to atm-storage-rusqlite),
`ef873180` (fix: centralize rusqlite timestamp parsing),
`cd3f7a92` (docs: add rusqlite coupling fix triage closure records).

Citations below are drawn directly from the triage record's own
occurrence entries, which quote exact file/line/snippet at the time of
triage. They are treated as verified-by-triage-record rather than
independently re-read against current HEAD, since the coupling was
fixed and the exact lines have since moved/been deleted. Where noted,
"approximate" means the citation is inferred from commit diffs rather
than an exact line-numbered snippet.

## (a) What storage boundary existed

`atm-storage` defines the storage-neutral schema/contract layer (rows,
traits, error mapping) that any backend (SQLite today) implements.
`atm-storage-rusqlite` is the concrete SQLite backend. `atm-core` is the
business/facade layer that is supposed to depend *downward* on `atm-storage`'s
neutral contracts, never the reverse. ADR-018 codifies this as a forbidden
graph edge: **`atm-storage-* -> atm-core` must not exist**, mechanically
enforced by `.just/lint_boundaries.py`.

The trait/abstraction meant to hide the concrete engine: `atm-storage`'s
contract types (e.g. `SharedDbTarget`, storage-neutral row types) are what
callers outside the backend should see. Nothing outside `atm-storage-rusqlite`
should need to know it's rusqlite specifically, and — the tighter direction
this finding is actually about — the backend crate itself should never need
to reach *upward* into `atm-core` business logic to do its job.

## (b) How it concretely leaked

Two related but distinct leaks, found in the historical record:

### Leak 1 (earlier, narrower): a concrete `rusqlite::Connection` type in a public-ish signature

`ATM-QA-BOUNDARY-001` (phase-T, 2026-05-10): a test helper
`fn message_count_in_connection(connection: &rusqlite::Connection) -> i64`
was introduced at `crates/atm-rusqlite/src/writer/mod.rs:542` during T.3, and
the finding calls out that this violates `BOUNDARY-ServerTransport` and
`BOUNDARY-ClientTransport-CLI` rules — a concrete external-crate type
(`rusqlite::Connection`) appearing directly in a function signature instead
of the storage-neutral handle (`SharedDbTarget` / `Arc<SharedDbTarget>`). The
finding was still open, semantically unchanged (`sqlite::Connection` via a
`use rusqlite as sqlite;` alias — literally the same violation renamed to
dodge grep), on the T.6 graft-client-surface branch before being closed by
commit message "fix: remove sqlite connection helper boundary leak" (in the
squashed T.6 PR #237 commit series, per `git log`).

### Leak 2 (broader, later): the backend crate depends on `atm-core` at the Cargo level

`RUSQLITE-CORE-COUPLING-001` (phase-AD, integrate/phase-AD @ `51a8b82e`):
this is the deeper version of the same failure mode — not just a type
appearing in a signature, but a real `Cargo.toml` dependency edge from the
storage backend up into the facade crate it should never need. Per the
triage record's occurrences (approximate — quoted from the triage snapshot,
not re-verified against current HEAD since this was fixed and the code moved):

- `crates/atm-storage-rusqlite/Cargo.toml:19` — a normal
  `atm-core = { package = "agent-team-mail-core", path = "../atm-core", ... }`
  dependency.
- `crates/atm-storage-rusqlite/src/mailbox_metadata.rs:3` —
  `use atm_core::derive_ack_requirement;`
- `crates/atm-storage-rusqlite/src/mailbox_metadata.rs:198` —
  `let ack_requirement = derive_ack_requirement(&InboxMessage { ... })`
- `crates/atm-storage-rusqlite/src/nudge_template_override_store.rs:3` —
  `use atm_core::boundary::{ BuiltInNudgeTemplateKind, NudgeTemplateOverrideStore, TeamNudgeTemplateOverrideMode, TeamNudgeTemplateOverrideRow };`
- `crates/atm-storage-rusqlite/src/lib.rs:516` —
  `-> Arc<dyn atm_core::boundary::NudgeTemplateOverrideStore + Send + Sync>`
- Several more `atm_core::types::IsoTimestamp`/`atm_core::error::AtmError`
  imports in the same two files plus `shared_db.rs:803`.

Root cause per the finding: a nudge-override-store trait contract
(`NudgeTemplateOverrideStore`) was designed under `atm_core::boundary`
instead of `atm-storage`, forcing the backend that implements it
(`atm-storage-rusqlite`) to depend on `atm-core` just to satisfy the trait's
home crate — plus a separate mailbox-metadata helper that imported
`atm_core::schema::InboxMessage` and `atm_core::derive_ack_requirement`
instead of using the canonical `atm-storage` schema or a backend-local
classifier.

## (c) File:line citations

See the two bullet lists above. The phase-T (`ATM-QA-BOUNDARY-001`) citation
(`crates/atm-rusqlite/src/writer/mod.rs:542`) is triage-record-verified at the
time of that finding; the file no longer exists at that path today (the
`atm-rusqlite` crate was superseded by `atm-storage-rusqlite`, see fix commit
`03acab1c` "Converge SQLite backend to atm-storage-rusqlite," which deletes
`crates/atm-rusqlite/src/lib.rs`, `mailbox_metadata.rs`, `roster_store.rs`,
`boundary_assembly.rs` wholesale and replaces them under
`crates/atm-storage-rusqlite/`). The phase-AD
(`RUSQLITE-CORE-COUPLING-001`) citations are likewise quoted from the triage
record's occurrence entries against `integrate/phase-AD @ 51a8b82e` and were
not re-read from that historical commit in this pass — treat them as
triage-sourced, not independently re-verified line-by-line.

## (d) Why this is a boundary leak and not a legitimate cross-boundary need

A legitimate cross-boundary need would be: the storage backend depends on
`atm-storage`'s neutral contracts (which it must implement) and nothing else.
What happened instead:

- The backend reached *upward* past its own contract layer into the facade
  crate (`atm-core`) that is supposed to sit *above* storage, not be a
  dependency of it. This is the wrong direction entirely — `atm-core` is meant
  to compose storage backends, not be composed into one.
- The specific trigger (a trait defined under `atm_core::boundary` that the
  backend must implement) is a classic "the abstraction lives in the wrong
  crate" leak: the trait's *location* forced an otherwise-avoidable dependency
  edge. The type itself (`NudgeTemplateOverrideStore`) is storage-neutral and
  belongs in `atm-storage` — its accidental home in `atm-core` is what created
  the leak, not any real need for storage to know about `atm-core` business
  logic.
- The mailbox-metadata leak (`derive_ack_requirement`, `InboxMessage` imported
  from `atm_core`) is the same pattern from the other direction: convenience
  reuse of an already-written classifier instead of moving (or duplicating
  minimally) the storage-neutral piece of that logic into `atm-storage` where
  the backend could reach it without an upward edge.
- The `rusqlite::Connection` signature leak (Leak 1) is the narrowest form of
  the smell list's core signal: a concrete external-crate type owned by one
  module (`atm-storage-rusqlite`, the module that "owns" its rusqlite
  dependency) appearing in a signature that other code has to touch — even a
  test helper — instead of the module's own opaque handle type
  (`SharedDbTarget`).

## (e) Recommended fix direction / pattern actually used

Per the finding's `resolutionNote` (fix landed on
`fix/phase-AD-pm-boundary-violation`, closed 2026-07-11) and verified by the
commit history:

1. **Opaque handle over concrete connection type**: replace
   `&rusqlite::Connection` parameters with `&SharedDbTarget` /
   `Arc<SharedDbTarget>` and open the connection internally inside the
   function that owns the rusqlite dependency. Never let the concrete
   connection type cross the function boundary.
2. **Move the trait to the crate it actually belongs to**: relocate the
   storage-neutral contract family (`NudgeTemplateOverrideStore` and its
   supporting enum/row types) from `atm_core::boundary` into `atm-storage`,
   with `atm-storage`-owned sealing, and keep `atm-core` as a
   **compatibility re-export only** during the cutover so downstream
   consumers (`atm-runtime`, `atm-daemon-bootstrap`, `atm`) don't break
   mid-move.
3. **Move the shared classifier, don't duplicate it**: relocate the minimal
   backend-neutral ack-intent classifier into `atm-storage` rather than
   letting the backend keep importing `atm-core`'s copy or hand-rolling a
   second one — avoiding both the forbidden edge and duplicated logic drift
   across `atm-storage-rusqlite`, `atm-runtime`, and `atm-core`.
4. **Delete the Cargo edge last, not first**: the normal
   `atm-storage-rusqlite -> atm-core` dependency in `Cargo.toml` was only
   removed once every import site had been repointed at the relocated
   `atm-storage` contract — treating "no Cargo edge" as the final proof of
   closure, not a step you can fake by hiding imports behind feature flags.
5. Timestamp parsing was likewise centralized into `atm-storage::types`
   (`ef873180`, "fix: centralize rusqlite timestamp parsing") rather than
   left duplicated between `atm-core` and the backend.

The general pattern: when a concrete backend needs a contract type, that
contract must live in the neutral layer the backend already depends on
(`atm-storage`), never in the layer above the backend (`atm-core`). If a
trait ends up implemented by a backend crate and it lives in the wrong crate,
that is the leak — move the trait, don't add a dependency edge to reach it.
