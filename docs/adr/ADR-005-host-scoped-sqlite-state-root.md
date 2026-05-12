# ADR-005 — Host-Scoped ATM SQLite State Root

| Field | Value |
|---|---|
| ID | ADR-005 |
| Status | **Accepted** |
| Date | 2026-05-07 |
| Deciders | Rand Lee |
| Relates to | REQ-P-RUNTIME-002, REQ-CORE-RUNTIME-001, REQ-RUSQLITE-STORE-001, ADR-002 |
| Supersedes | Per-team SQLite path assumption in earlier planning prose |

---

## Context

Two planning assumptions currently pull against each other:

- daemon runtime is host-wide singleton
- older SQLite planning prose still points at one `mail.db` under each team
  directory

That shape is awkward.

If one host-wide daemon owns the ATM runtime and serializes or coordinates
writes centrally, scattering one SQLite database under every team directory
adds operational complexity without improving ownership clarity.

Problems with the per-team database model:

- one daemon must discover, open, migrate, checkpoint, and health-report
  multiple databases
- backup, restore, migration, and corruption handling become more fragmented
- schema/version/state drift can occur across team-local roots on one machine
- runtime ownership becomes split awkwardly across one daemon and many local
  database files
- cross-team routing, roster, task, and health queries become harder to reason
  about than one host-owned durable state root

## Decision Drivers

- daemon singleton is host-wide
- SQLite ownership should align with daemon ownership
- one writer and one WAL/checkpoint path are easier to reason about than many
  local database roots
- team and agent are logical tenancy keys, not good reasons to fragment the
  physical durable store
- fewer local database roots reduce migration, backup, and repair complexity

## Options Considered

### Option 1 — One SQLite Database Per Team

Keep one physical `mail.db` under each team directory.

**Rejected.** This keeps durable state fragmented across the host while the
daemon remains host-scoped. It increases operational complexity and weakens the
alignment between singleton runtime ownership and durable store ownership.

### Option 2 — One Host-Scoped SQLite Database Per Machine

Use one host-scoped ATM durable state root with one physical SQLite database,
while keeping team and agent as logical partition keys inside that database.

**Accepted.**

### Option 3 — One SQLite Database Per Agent

Further split durable state into per-agent roots.

**Rejected.** This multiplies the same problems as the per-team model and makes
runtime coordination even worse.

## Decision

ATM adopts one host-scoped SQLite durable state root per machine for the Phase
R daemon/runtime line.

Required invariant:
- one host-scoped ATM runtime owns one physical SQLite database for durable ATM
  mail, roster, task, replay, and related state on that host

Required shape:
- team and agent remain first-class logical keys inside the database
- team directories remain ingress, compatibility, config, and recovery
  surfaces; they do not define the durable database ownership boundary
- daemon/runtime health, crash recovery, migration, and checkpoint behavior
  must be reasoned about against one host-owned database

Pathing rule:
- the default on-disk host-scoped durable-state root is `~/.atm/db/`
- the canonical database file under that root is `~/.atm/db/mail.db`
- the durable database path must no longer be derived as one `mail.db` per team
  root

Testing rule:
- tests must not depend on the production durable-state root
- most in-process SQLite tests should use a dedicated in-memory database
  fixture
- test fixtures must keep setup and cleanup explicit so state cannot leak
  across tests
- when filesystem behavior, migration behavior, or restart/reopen behavior must
  be exercised, tests may provision an explicit temporary database root
- the on-disk database suite should remain small and deliberate so filesystem
  coverage exists without dragging most tests into slower disk-backed setup
- test database paths must stay explicit test seams rather than silent fallback
  to the production `~/.atm/db/` root

## Consequences

### Positive

- daemon singleton ownership and SQLite durable ownership align
- one WAL/checkpoint/recovery path per host
- simpler health reporting and migration behavior
- simpler cross-team joins and queries where the product needs them
- fewer local durable-state files to discover and repair

### Negative

- team-scoped backup and restore must operate against one shared host database
  rather than a naturally isolated per-team file
- corruption blast radius is larger than in the per-team model
- existing docs that still describe per-team `mail.db` paths must be updated

## Follow-Up Work

| Action | Owner | Gate |
|---|---|---|
| Replace per-team `mail.db` assumptions in requirements, architecture, and project-plan docs | `arch-ctm` | planning doc review PASS |
| Add explicit boundary and migration notes for team-scoped backup/restore against one host-owned database | `arch-ctm` | SQLite closeout sprint acceptance |
| Keep test DB paths explicit and in-memory-first in SQLite requirements and sprint docs | `arch-ctm` | sqlite test-plan review PASS |

*ADR-005 | agent-team-mail | 2026-05-07*
