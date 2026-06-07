---
sprint: AA-mailbox-cleanup
title: "Mailbox Implementation Cleanup and Crate Extraction Assessment"
status: complete
branch: feature/pAA-mailbox-cleanup
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pAA-mailbox-cleanup
base: develop
pr_target: develop
owner: arch-ctm
---

# Sprint AA-mailbox-cleanup: Mailbox Cleanup & Crate Extraction Assessment

## Goal

Clean up `crates/atm-core/src/mailbox/` and produce a concrete assessment of whether the mailbox subsystem should be extracted into a standalone `atm-mailbox` crate. If extraction is architecturally sound, perform it.

## Scope

### Phase 1 — Audit (required)

Survey all 7 sub-modules in `crates/atm-core/src/mailbox/`:

| Module | Responsibility (to be confirmed) |
|--------|----------------------------------|
| `mod.rs` | Top-level mailbox read/write, salvage logic |
| `atomic.rs` | Atomic file operations |
| `hash.rs` | Content hashing |
| `lock.rs` | File locking (acquire/release) |
| `source.rs` | Source file resolution |
| `store.rs` | Storage abstraction |
| `surface.rs` | Public surface / API boundary |

For each module, document:
- Current responsibilities
- Public vs private surface
- Dependencies on other `atm-core` internals (imports from outside `mailbox/`)
- Dependencies flowing INTO mailbox from other modules

### Phase 2 — Cleanup (required)

Apply these improvements regardless of extraction decision:

1. **RULE-002 (function-length)**: `salvage_mailbox_array` in `mod.rs` is 106 lines. Split into helpers — each under 80 lines. Suggested split: extract the JSON object scanning loop into `fn scan_object_spans(raw: &str) -> (Vec<(usize, usize)>, Option<usize>)`. Keep algorithm identical, no behavior change.

2. **General cleanup**: Remove dead code, simplify overly complex error paths, ensure consistent naming per existing project conventions.

3. **Lint gate**: After cleanup, `python3 .just/run_lint.py function-length` must exit 0 with no new hard violations in the mailbox module diff.

### Phase 3 — Extraction assessment (required)

Write a short assessment section in this sprint doc (under `## Extraction Assessment`) answering:

- Is the mailbox subsystem self-contained? (i.e., can it compile with minimal `atm-core` leakage?)
- What `atm-core` types would need to move with it or be re-exported?
- What would the `Cargo.toml` dependency graph look like? (`atm-core` → `atm-mailbox` or `atm-mailbox` → `atm-core`?)
- Recommendation: **Extract** / **Defer** / **Do not extract** — with one-paragraph rationale

### Phase 4 — Extraction (conditional)

**Only if Phase 3 assessment recommends "Extract":**

1. Create `crates/atm-mailbox/` with:
   - `Cargo.toml` (edition 2021, workspace member)
   - `src/lib.rs` (re-export public API)
   - Move mailbox sub-modules from `crates/atm-core/src/mailbox/` into `crates/atm-mailbox/src/`

2. Update `crates/atm-core/Cargo.toml` to depend on `atm-mailbox`

3. Update `Cargo.toml` workspace `members` array

4. Update all import paths in `atm-core` and `atm` crates that referenced the old `atm_core::mailbox` paths

5. Ensure `cargo build --workspace` and `cargo test --workspace` pass

## Deliverables

- [ ] Phase 1: Audit notes committed to this sprint doc (under `## Audit Notes`)
- [ ] Phase 2: Mailbox cleanup committed — `python3 .just/run_lint.py function-length` clean
- [ ] Phase 3: Extraction assessment written in this sprint doc
- [ ] Phase 4 (conditional): `crates/atm-mailbox/` created and wired into workspace, OR explicit "Defer/Do not extract" in assessment
- [ ] `cargo test --workspace` PASS
- [ ] `cargo clippy --workspace -- -D warnings` PASS
- [ ] Sprint doc frontmatter updated: `status: complete`

## Acceptance Criteria

- `salvage_mailbox_array` (and all mailbox functions) under 80 lines after cleanup
- `python3 .just/run_lint.py function-length` exits 0 (no new RULE-002 hard violations in diff vs develop)
- `cargo test --workspace` PASS
- `cargo clippy --workspace -- -D warnings` PASS  
- Extraction assessment present in this doc
- If extracted: `cargo build --workspace` PASS with `atm-mailbox` as workspace member; no dead re-exports
- If not extracted: rationale documented; no functional changes beyond cleanup

## Constraints

- Do NOT change observable mailbox behavior — same inputs must produce same outputs
- Do NOT widen grandfathered RULE-002 violations outside the mailbox module
- Do NOT break the `atm-daemon` or `atm` crates
- Base is `develop` — this sprint is independent of integrate/phase-AA

---

## Audit Notes

### Module inventory

| Module | Current responsibility | Public vs private surface | Depends on outside `mailbox/` | Inbound dependencies |
|--------|-------------------------|---------------------------|-------------------------------|----------------------|
| `mod.rs` | Top-level mailbox read/load dispatch, JSONL vs JSON-array parsing, ATM field sanitization, test-only append/lock rewrite seams | Exports test-only `append_message`, crate-visible load/import/export helpers, keeps parse helpers private | `crate::error`, `crate::schema::{AtmMessageId, MessageEnvelope}`, `crate::types::{AgentName, TeamName}`, `serde_json`, `tracing` | `boundary_support`, `store`, tests |
| `atomic.rs` | Low-level atomic rewrite and append primitives (`write_messages`, `append_message`, `append_jsonl_record`) | Public only inside mailbox ownership boundary; no parsing logic | `crate::persistence`, `crate::error`, `crate::schema::MessageEnvelope`, `crate::schema::inbox_message::SharedInboxExportPolicy` | `store` |
| `hash.rs` | Placeholder only; no live hashing logic | No live items | none | none |
| `lock.rs` | Sentinel-path derivation, single/multi-lock acquisition, stale-lock sweeping, in-process lock registry | Crate-visible lock primitives; many helpers already private | `crate::error`, `crate::process::process_is_alive`, `fs2`, `same_file`, `tracing` | `workflow`, `mod.rs`, `team_admin::restore`, tests |
| `source.rs` | Resolve read target, discover inbox/source paths, load source files, test-only source-set drift validation | Crate-visible structs/functions for projection loading; discovery helpers private where possible | `crate::address::AgentAddress`, `crate::config`, `crate::home`, `crate::error`, `crate::schema::MessageEnvelope`, `crate::types::{AgentName, SourceIndex, TeamName}` | `list`, `read`, `clear`, `store`, tests |
| `store.rs` | Mailbox-owner write boundary, export policy lookup, recovered-message-set validation, source projection write/read orchestration | Crate-visible write/load entry points; policy/validation helpers private | `crate::config`, `crate::error`, `crate::schema::MessageEnvelope`, `crate::schema::inbox_message::SharedInboxExportPolicy`, `crate::types::{AgentName, TeamName}` | `service_runtime`, `boundary_support`, `mod.rs`, tests |
| `surface.rs` | Deduplicate merged mailbox surfaces by `message_id` and timestamp | One crate-visible canonicalization helper, rest private/tests | `crate::schema::AtmMessageId`, `crate::types::IsoTimestamp` | `read::build_message_items_from_sources`, tests |

### Cleanup applied

1. `locked_read_modify_write(...)` in `mod.rs` is now private test-only surface instead of `pub(crate)`.
2. `default_lock_timeout()` in `lock.rs` is now private test-only surface instead of `pub(crate)`.
3. `python3 .just/run_lint.py function-length` already passed on this branch state before edits, and still passes after cleanup.

### RULE-002 note

The sprint template referenced splitting `salvage_mailbox_array(...)`, but that function does not exist on this branch. `feature/pAA-mailbox-cleanup` is based on `develop @ d5559468`, which still uses the older mailbox reader that:

- parses JSON arrays through `parse_mailbox_array(...)`
- parses JSONL through `parse_mailbox_jsonl(...)`
- skips malformed records with warning logs instead of the later salvage-item pipeline

Because this branch does not carry the later integrate-era salvage refactor, importing that larger change just to satisfy the template would widen scope and violate the sprint constraint to avoid observable behavior changes. The cleanup gate for this branch is therefore the actual lint result, not a nonexistent function split.

## Extraction Assessment

### Self-contained?

No. The mailbox subsystem is not currently self-contained enough to extract cleanly.

- Parsing and source discovery depend directly on `atm-core` domain types such as `AtmError`, `MessageEnvelope`, `AgentAddress`, `AgentName`, `TeamName`, `SourceIndex`, and `IsoTimestamp`.
- Write paths depend on `crate::config`, `crate::home`, `crate::persistence`, and `crate::process`.
- The read/write call graph is embedded in higher-level commands and runtime paths (`read`, `list`, `clear`, `workflow`, `service_runtime`, `team_admin::restore`, `boundary_support`).

### What would need to move or be re-exported?

At minimum, a real extraction would need a shared dependency layer for:

- `AtmError` and mailbox-specific error helpers
- `MessageEnvelope` and related schema helpers
- validated identity/value types (`AgentName`, `TeamName`, `SourceIndex`, `IsoTimestamp`)
- config/home/persistence seams currently owned by `atm-core`

Without that shared layer, `atm-mailbox` would either depend back on `atm-core` or force broad re-export leakage from `atm-core`.

### Likely dependency graph

The only acceptable graph would be:

- `atm-core -> atm-mailbox`

Current code shape does not support that graph. Today’s implementation naturally wants either:

- `atm-mailbox -> atm-core` (bad: reverse layering), or
- a new lower shared crate that both `atm-core` and `atm-mailbox` depend on

That second option is a separate architecture project, not a sprint-local cleanup.

### Recommendation

**Defer.** The mailbox subsystem is too entangled with `atm-core` domain types, config/home resolution, persistence helpers, and command/runtime call sites to extract safely inside this sprint. Extraction now would mostly relocate files while preserving the same coupling through re-exports or reverse dependencies, which is architectural churn rather than simplification. The right prerequisite is a separate boundary-design pass that first isolates mailbox-facing value types, persistence seams, and error contracts into a lower shared layer; only then would `atm-core -> atm-mailbox` become a sound crate graph.
