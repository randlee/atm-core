---
status: complete
branch: fix/agent-team-name-charset-validation
worktree: /Users/randlee/Documents/github/atm-core-worktrees/fix/agent-team-name-charset-validation
---

# Sprint: agent/team name charset hardening

## Motivation

Cross-host addressing parses `<agent>@<team>.<host>` and uses `:` and `.` as
reserved delimiters. PR #572 (companion branch off `feature/pAG-s15-othermac-smoke`)
already closed the narrow case of `.` in team/agent names causing inline-host-syntax
ambiguity (AG-FIND-007). The user has asked for the general rule to be applied
repo-wide, off `develop`, independent of the phase-AG branch: `<agent>` and `<team>`
must be treated as path-segment-like identifiers, not free-form labels.

## Rule

`<agent>` and `<team>` must reject:
- path delimiters: `/` `\`
- traversal forms: `.` `..`
- reserved address delimiters: `.` `:`
- whitespace
- wildcard/pattern characters if any parser might treat them specially later (e.g. `*` `?` `[` `]`)

Practical safe set: allow only ASCII letters, digits, `-`, `_`.

## Deliverables

1. Identify every validation site that currently constrains agent/team name
   charset (this repo already has at least two copies of `validate_path_segment`
   — `crates/atm-storage/src/validation.rs` and `crates/atm-core/src/address.rs`
   — plus any daemon/CLI-side duplicate checks) and converge them on the safe
   set above (ASCII letters, digits, `-`, `_` only), rejecting everything else
   including whitespace and wildcard/pattern characters, not just `.`/`:`/`/`/`\`.
2. Confirm this supersedes/broadens PR #572's `.`-only fix rather than
   conflicting with it — if PR #572 merges to develop before this branch, merge
   develop forward into this worktree and adjust; if this branch lands first,
   note that PR #572 becomes redundant with this fix.
3. Update or add regression tests proving each rejected character class is
   actually rejected (not just `.`), and that the safe set (letters, digits,
   `-`, `_`) is still accepted.
4. Update requirements/ADR text describing agent/team naming constraints if it
   currently under-specifies the charset (grep for the existing team/agent name
   validation requirement and tighten the wording to state the full safe set,
   not just "no dots").

## Implementation Addendum

This sprint closes with a centralized execution path: `atm-storage` owns the
canonical identifier validator and `atm-core` delegates to that shared rule.
Any future follow-up must preserve that single-policy ownership instead of
reintroducing crate-local charset drift. The constraints remain:

1. Centralize the identifier-charset rule in one authoritative validation
   policy instead of letting `atm-core` and `atm-storage` drift through
   hand-maintained duplicate allowlists.
2. Any retained wrapper/helper in another crate may delegate to that policy,
   but must not redefine the accepted or rejected character set locally.
3. The code change must preserve the existing valid names already in active
   use (`team-lead`, `arch-ctm`, `quality-mgr`, `atm-dev`) while rejecting the
   newly reserved delimiter set consistently everywhere.
4. If the eventual implementation cannot remove duplication entirely, the
   sprint must document the exact remaining duplication seam and why it is
   temporarily unavoidable.

### Phase-AG code scan: centralization inventory

The current Phase-AG line already shows exactly where identifier validation and
delimiter handling have drifted into multiple local seams. The follow-up code
sprint should centralize these rather than patching each one independently.

1. Duplicate path-segment validators exist in two crates today.
   - `crates/atm-core/src/address.rs::validate_path_segment`
   - `crates/atm-storage/src/validation.rs::validate_path_segment`
   These two functions currently carry duplicated character-policy logic and
   have already drifted historically. The code sprint should make one
   authoritative identifier policy and have the second seam delegate to it
   instead of preserving two hand-maintained allowlists.

2. Historical AG send-target parser references were removed before this
   develop-based fix branch was cut.
   - `crates/atm-core/src/send/mod.rs::validate_send_target_segment`
   - `crates/atm-core/src/send/mod.rs::parse_send_target_impl`
   do not exist in this worktree. They remain historical AG planning notes,
   not active seams on this branch.

3. The current live split seams on this branch are narrower than the AG notes
   originally described.
   - `crates/atm-core/src/address.rs::AgentAddress::from_str` uses
     `split_once('@')`
   - `crates/atm-core/src/ack/mod.rs::ReplyTarget::deserialize` uses
     `split_once('@')`
   - `crates/atm-storage/src/validation.rs::validate_agent_at_team` now owns
     the shared `agent@team` split+validate helper used by
     `crates/atm-storage/src/types.rs::AgentId::new`
   Future work should decide whether these should collapse into one typed
   parser boundary so later forms such as `<agent>:<session>@<team>.<host>` do
   not spread delimiter ownership across unrelated modules.

4. Storage-side typed constructors depend directly on the storage validator.
   - `crates/atm-storage/src/types.rs::AgentName::from_str`
   - `crates/atm-storage/src/types.rs::AgentId::new`
   - `crates/atm-storage/src/types.rs::TeamName::from_str`
   Any centralization plan must preserve these typed constructors as the
   durable-entry gate, but the underlying charset policy they call should stop
   being storage-local policy drift.

5. Multiple other call sites consume the validator and should remain
   delegation-only.
   Current examples on the AG line:
   - `crates/atm-core/src/home.rs`
   - `crates/atm-core/src/read/seen_state.rs`
   - `crates/atm-core/src/team_admin/filesystem.rs`
   - `crates/atm-core/src/config/discovery.rs`
   - `crates/atm-core/src/doctor/mod.rs`
   These are not separate policy owners today; they are downstream consumers.
   The code sprint should keep them that way and avoid introducing additional
   local character checks there.

6. The centralization sprint should explicitly decide the ownership boundary:
   either
   - one shared identifier-policy type/function exported across the relevant
     crates, or
   - one canonical crate-level validator plus thin delegating adapters
   but not two independent implementations that must be kept in sync by
   convention.

## Closure Notes

- `REQ-SEC-001`, `REQ-CORE-TRANSPORT-002`, and `ADR-031` now use the same
  safe-set wording: ASCII letters, ASCII digits, `-`, `_` only.
- `crates/atm-storage/src/validation.rs` is the authoritative validator.
- `crates/atm-core/src/address.rs` now delegates to the shared validator
  instead of carrying a second hand-maintained implementation.
- `crates/atm-storage/src/validation.rs::validate_agent_at_team` is now the
  shared `agent@team` validation helper used by `AgentId::new`; the stale AG
  send-target parser references in the earlier inventory were historical and
  are not present on this branch.
- PR #572 is superseded by this broader fix if this branch lands first.

## Acceptance Criteria

- Every agent/team name validation site in the workspace enforces exactly the
  safe set (ASCII letters, digits, `-`, `_`); no site allows `.`, `:`, `/`, `\`,
  whitespace, or wildcard characters.
- New/updated tests cover rejection of each forbidden character class and
  acceptance of the safe set.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  and `cargo fmt --all --check` all pass.
- No regression to existing valid agent/team names already in use (e.g.
  `team-lead`, `arch-ctm`, `quality-mgr`, `atm-dev`).
