# Develop Deltas Since AB Plan

## Bracket

This document records what changed on `develop` between the two PRs that bracket the
gap:

- **PR #389** (`plan/phase-AB`, merged 2026-06-04) — merged the AB plan documents
  into the repo. This is the reference point: the AB plan was authored against the
  codebase state at that merge.
- **PR #420** (`integrate/phase-AC`, merged 2026-06-09) — merged the full Phase AC
  integration into develop. This is the upper bound.

The staging branch `feature/phase-AB-smoke-staging` is cut from `origin/develop` at
commit `c451afe4`, which is post-AC and includes all changes described below.

Approximately 158 non-merge commits landed on develop between these two PRs.

---

## Crates Added by Phase AC

| Crate | Purpose |
|---|---|
| `atm-storage` | Storage abstraction traits and core types |
| `atm-storage-claude` | Claude-backed storage implementation |
| `atm-storage-rusqlite` | SQLite-backed storage implementation (replaces `atm-rusqlite`) |
| `atm-storage-sqlserver-proof` | SQL Server proof-of-concept storage implementation |
| `atm-architecture` | Architecture specification crate |
| `atm-runtime` | Runtime orchestration and composition support |

## Crates Removed by Phase AC

| Crate | Replacement |
|---|---|
| `atm-rusqlite` | Replaced by `atm-storage-rusqlite` |

---

## CLI Surface Changes

The CLI command surface is unchanged. The following commands retain their signatures:

- `atm doctor` / `atm doctor --json`
- `atm send`
- `atm read` / `atm read --all --json`
- `atm list`
- `atm clear`
- `atm ack`

Environment variable names are also unchanged.

---

## Substantially Changed Files (AC impact on smoke rows)

The following files changed substantially in Phase AC and affect the expected behavior
or output of smoke checklist rows:

| File | Change Summary |
|---|---|
| `crates/atm/src/commands/doctor.rs` | Composition-aware doctor output; health surface reflects new storage layer |
| `crates/atm-daemon/src/composition.rs` | ~150 lines of changes; storage and runtime wiring added |
| `crates/atm/src/commands/members.rs` | Updated for AC storage model |
| `crates/atm/src/commands/teams.rs` | Updated for AC storage model |
| `crates/atm-core/src/ack/mod.rs` | ~82 lines changed; ack now routes through `StorageNotifier` trait |

The `StorageNotifier` trait is a new AC abstraction. Anything that previously triggered
notifications directly now calls through this trait. This affects ack, send confirmation,
and degraded-state reporting.

---

## Windows SQLite Contention Fixes

Two Windows-specific SQLite mailbox contention fixes landed inside Phase AC:

| Commit | Message |
|---|---|
| `e32b6546` | Fix AC4 Windows sqlite mailbox contention |
| `80f96f85` | Fix AC5 Windows sqlite mailbox contention |

These fixes are relevant to Windows host smoke testing. They address contention
conditions that could produce spurious failures when the daemon and CLI race on the
SQLite mailbox file. Both fixes are present in the `c451afe4` staging base.

---

## Workspace Version Bump

The workspace `Cargo.toml` version was bumped from `1.2.1` to `1.2.2` during Phase AC.
The `rust-version` pin remains `1.94.1`.

---

## Documentation Restructure

Phase AC reorganized the documentation hierarchy. Phase plan folders were moved from
`docs/` top-level placement to `docs/plans/<phase>/`. The AB plan documents are located
at `docs/plans/phase-AB/` in the current tree, consistent with this restructure.

---

## Impact on AB Plan Assumptions

The AB plan was authored against the pre-AC codebase. The CLI surface and environment
variable names are stable, so the procedural steps in the smoke checklist remain
structurally valid. However, the internal composition and storage changes mean that
expected output from `atm doctor --json` and ack-related commands may differ from what
was anticipated when the AB plan was written. See `ac-freshness-flags.md` for a
row-by-row risk assessment.
