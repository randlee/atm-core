# ADR-061 — Governed Interface Schema Versioning And Breaking-Change Approval

| Field | Value |
| --- | --- |
| ID | ADR-061 |
| Status | Accepted (Rand, 2026-09-05) |
| Scope | The three managed interfaces: HTTP/peer API, Herdr IPC, SQLite storage schema |
| Relates to | ADR-036, ADR-057, ADR-060, `docs/atm-daemon/http-api.md`, `docs/atm-rusqlite/requirements.md`, phase-ay plan, GitHub issue #1217 (discussion record only) |

## Context

`HTTP_API_VERSION` was set to `1.0.0` on 2026-07-24 and never bumped while
the wire gained new request variants and optional fields. Nothing in the
process required a bump, nothing required approval for a breaking change,
and nothing checked shipped wire changes against the approved plan. The
Herdr IPC adapter (`crates/atm-herdr`) has no version constant at all and
reports any mismatch as a bare protocol-mismatch error. The SQLite schema is
migrated by idempotent additive DDL in
`crates/atm-storage-rusqlite/src/shared_db.rs` with pre-migration unit
tests, but has no declared version and no rule for what a breaking change is.

Rand's rulings (2026-09-05): the interfaces are semver versioned; adding an
optional parameter is a minor bump; breaking changes need explicit recorded
approval; breaking changes that force every host to upgrade in lockstep are
unacceptable; new capability must be expressed as optional arguments
wherever possible; there is no 2.0, the HTTP wire as of v1.4.13 / 1.5.1 is
defined as `1.1.0` before AY.15; Herdr drifts outside our control and every Herdr
release at or above `HERDR_MINIMUM_VERSION` must stay supported; the SQLite
schema gets the same scrutiny; "all should be managed by the same
gates/rules"; "all have the potential to cause breaking changes on
computers or between computers or between applications".

## Decision

### D1. Three governed interfaces, one rule set

Each interface can break something different: the SQLite schema breaks a
computer against its own earlier binary, the HTTP/peer API breaks hosts
against each other, and the Herdr IPC breaks applications against each
other. They are governed identically because the failure is the same kind:
a consumer that worked yesterday stops working without anyone having
approved that.

| Interface | Version constant | Source of truth | Consumer that must keep working |
| --- | --- | --- | --- |
| HTTP/peer API | `HTTP_API_VERSION` (semver), `CLI_SCHEMA_VERSION` (local envelope) in `crates/atm-core/src/protocol.rs` | Rust serde types (`RequestEnvelope`, `ResponseEnvelope`, `WriteRequest`); `openapi.yaml` is derived documentation | Every deployed daemon and CLI speaking the same major |
| Herdr IPC | `HERDR_MINIMUM_VERSION` (Herdr release semver from `ping.version`; Herdr `PROTOCOL_VERSION` recorded per release as a secondary fact) in `crates/atm-herdr` | Herdr's published wire, outside our control | Every Herdr release `>= HERDR_MINIMUM_VERSION`, simultaneously, from one daemon build |
| SQLite storage schema | `STORAGE_SCHEMA_VERSION` (semver) declared by `atm-storage-rusqlite` and persisted in the database | `DB_MIGRATIONS` and the `ensure_*` migration functions | The previous supported release binary opening the same database (daemon-switch rollback) |

`HERDR_MINIMUM_VERSION` is `0.8.0` today (`crates/atm-herdr/src/lib.rs`),
set by Rand on 2026-09-05 from the M5 agents' drift review of Herdr
v0.8.0..v0.8.2. The phase-ay plan lands the runtime checks around it (AY.1,
AY.3). `STORAGE_SCHEMA_VERSION` does not exist yet and needs its own planned
sprint; until it lands, the SQLite rules below are applied against the
migration functions directly.

### D2. Classification

- **Minor (additive):** a new optional field, argument, request variant,
  route, table, column with a default, or index. Older consumers must keep
  working unchanged: receivers default omitted fields and ignore unknown
  fields; an older binary opening a newer database must still be able to
  read and write the rows it understands.
- **Major (breaking):** removing or renaming a field, variant, route, table,
  or column; changing a type, constraint, status, or meaning; tightening
  validation an older peer would fail; raising `HERDR_MINIMUM_VERSION`;
  any storage change the previous supported release binary cannot operate
  against.
- **Patch:** documentation or test-only change with no wire or DDL effect.

### D3. Approval gate

- A minor change requires the matching version bump in the same change set,
  updated documentation (`openapi.yaml` and surface baseline; the storage
  schema document; the Herdr version matrix) and a test proving the older
  consumer still works.
- A major change requires Rand's explicit, recorded approval and sign-off
  before plan approval, cited by message id, issue comment, or ADR, and
  cited again at phase end. It must ship with a co-existence window: the new
  build still serves the previous major (or negotiates down) for the
  duration Rand approves. No change may require every host to upgrade
  together.
- Design rule: before proposing a major change, the author must show why the
  capability cannot be expressed additively.

### D4. Enforcement

- `schema-reviewer` (`.claude/agents/schema-reviewer.md`) is a mandatory
  reviewer in every `quality-mgr` plan review and every phase-ending review.
  It classifies every change to the three interfaces, verifies the bump,
  documentation and tests, and at phase end diffs shipped changes against
  the approved plan. Unapproved major changes and unapproved drift are
  Blocking.
- Every interface change is documented and tested the same way SQLite
  migrations already are: a pre-change fixture, the change, and a test that
  the older consumer still works.

### D5. HTTP API version record

- **2026-09-09 — issue #1378 canonical agent state:** `HTTP_API_VERSION` moves
  from `1.3.0` to `1.4.0`. The runtime-status member projection adds
  `revision`, `availability`, `last_observation_attempt_by`,
  `last_observation_attempt_at`, `last_observed_by`, and `last_observed_at`.
  Receivers default every added field when it is omitted, and older consumers
  may ignore the additive fields. The retained pre-1.4 payload test proves the
  same-major compatibility path. This is a minor, backward-compatible bump.
- **2026-09-09 — DOCTOR-HERDR-TARGET-R1:** `HTTP_API_VERSION` moves from
  `1.2.0` to `1.3.0`. The optional `findings` collection on each Herdr endpoint
  observation carries target-resolution and named-session diagnostics. Empty
  collections are omitted, so older payloads remain readable and older
  consumers may ignore the additive field. This is a minor, backward-compatible
  bump.
- **2026-09-07 — AY.15:** `HTTP_API_VERSION` moves from `1.1.0` to `1.2.0`.
  The additive doctor-presence field `herdr_agent` reports the durable
  effective Herdr identity. It is optional when deserializing and omitted
  when absent, so 1.2.0 readers accept 1.1.0 payloads and older readers may
  ignore the new field. This is therefore a minor, backward-compatible bump.

### D6. Storage-schema approval record

- **2026-09-09 — Phase AZ (withdrawn):** Rand approved a planned
  `STORAGE_SCHEMA_VERSION = 2.0.0` task-domain migration as an ADR-061 major
  change (ATM `1.6.0` canonical v2 tables with a v1 bridge through `1.6.x`;
  `1.7.0` earliest bridge removal).
- **2026-09-11 — withdrawal:** Phase AZ was retired unmerged and that approval
  was withdrawn with it. Nothing under it shipped. No storage major change is
  currently approved. Phase BA (`docs/plans/phase-ba/nudge-task-design.md`,
  ADR-062 Phase BA amendment) changes the `tasks` table in place and is
  classified under D2 and gated under D3 on its own record; ADR-063 is
  superseded and is not approval for any storage change.

## Consequences

- `HTTP_API_VERSION` first moved to `1.1.0` as the current wire, AY.15 moved it
  to `1.2.0` for the additive doctor-presence field, and
  DOCTOR-HERDR-TARGET-R1 moved it to `1.3.0` for endpoint findings. Issue
  #1378 moves it to `1.4.0` for canonical runtime-state revision and freshness
  fields; it is bumped on every later governed-interface change.
- Herdr support becomes a matrix, not a single version; per-release
  conformance fixtures are required.
- SQLite migrations gain a declared version and an explicit rollback test
  against the previous release binary.
- Plan documents must list their intended interface changes so phase-end
  drift review has something to diff against.
