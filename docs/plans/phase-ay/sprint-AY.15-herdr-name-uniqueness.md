---
id: AY.15
phase: AY
sprint: AY.15
title: unique_name invariant — alias ?? name unique across the ATM database
branch: feature/ay15-herdr-name-uniqueness
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay15-herdr-name-uniqueness
integration_branch: integrate/phase-ay
stack_parent: none
pr_target: integrate/phase-ay
target: integrate/phase-ay
status: dispatched
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: serial
parallel_with: []
dependency_relations:
  - prerequisite: AY.14
    dependent: AY.15
    relation: must_follow
    rationale: closes AY14-QA-003 (blocking) and the naming permutation gaps found on integrate/phase-ay f6c9d47b8; AY.14 is merged (#1305).
---

# AY.15 — unique_name invariant

## Requirement (Rand, 2026-09-07, verbatim)

- "This really is a simple UX abstraction.  Both names work user facing,
  everything under the hood used member-name except herdr which uses
  unique-name"
- "the requirement comes from herdr agent name MUST be unique which means
  herdr agent name must be unique on atm database."
- "herdr agent name = alias.  if alias is null/empty, alias would be equal
  to member name"
- "this allows us to have the same name on different teams (we should
  guarantee uniqueness of names per team already) by simply adding an alias
  for the conflicting name."
- "we call this 'alias' because using the alias for cross-team messaging
  has value independent of herdr."
- "so basically, there should be a query across all team roster for
  'unique-name' which would return alias ?? name."
- "if the list of unique-name collides with a proposed alias ?? name, add
  member must fail"
- "where we will run into issues are when upgrade occurs.  if non-unique
  names show up in database, hmux launch will certainly fail (hmux calls add
  member), so that should force team to be re-constructed before team can
  actually go live in herdr."
- "From any cli command accepting team-member name, alias must be allowed
  AND substituted before sending over wire.  i.e. atm send team-lead-alias
  <message> || atm send team-lead-alias@team <message> or any args i.e. --as
  team-lead-alias, --from team-lead-alias ..."
- "basically if all prompts are written for either member name or alias, it
  will work the same"
- "alias@team.host works" (cross-host, matrix D-13: in scope)
- "I would probably allow the daemon to do the replacement.  cli doesn't
  need to query for alias before sending.  alias would be in immutable
  roster, so replacement on ingress to the daemon is the logical single
  point to translate"

Requirements text: `docs/requirements.md` §3.3.2 `REQ-ROSTER-NAME-001..008`.
Permutation matrix: `docs/plans/phase-ay/herdr-naming-test-matrix.md`.
Every row marked **GAP** there is owed by this sprint (D-13 included
since Rand's 2026-09-07 ruling).

## Defects on integrate/phase-ay f6c9d47b8

- AY14-QA-003 (blocking, quality-mgr): `ensure_canonical_member_name_available`
  (`crates/atm-core/src/team_admin/member_mutation.rs:480`) ignores other
  members' alias fields and skips the caller's own team, so a new canonical
  name equal to an existing alias is accepted (matrix A-06, A-07).
- The canonical-name check runs in `team_admin` outside the roster store
  transaction: concurrent writers and non-CLI write paths bypass it (A-21,
  A-22, A-23, A-24).
- `ensure_canonical_member_name_available` and `ensure_alias_available`
  are stricter than the rule: they compare against canonical names of
  members that carry an alias, whose unique_name is the alias (A-09, A-10).
- Empty/whitespace alias is not normalised to "no alias" (A-18).
- Bare alias resolution is team-scoped (`resolve_roster_alias`,
  `crates/atm-core/src/caller_context.rs:59`); a database-wide unique alias
  must resolve without `@team` from any team (D-03).

## Deliverables

- D1 `crates/atm-storage/src` + `crates/atm-storage-rusqlite/src/roster_store.rs`:
  one roster-store query `unique_names()` across every team returning
  `(team, agent_name, unique_name)` where
  `unique_name = COALESCE(NULLIF(TRIM(json_extract(metadata_json,'$.alias')),''), agent_name)`.
  `save_roster` validates, inside its write transaction, that every
  unique_name of the roster being written is absent from every other
  team's unique_names and unique within the roster. This replaces
  `validate_database_wide_aliases`. No SQLite schema change; if you choose a
  generated column + UNIQUE index instead, record it as an ADR-061 minor
  bump in the PR body and say why.
- D2 `crates/atm-core/src/team_admin/member_mutation.rs`: delete
  `ensure_canonical_member_name_available` and `ensure_alias_available`;
  `add_member_with_roster_store` and `update_member` pre-check with the same
  `unique_names()` query only to produce the operator error (REQ-ROSTER-NAME-003:
  conflicting team, member, `--alias` remedy); the store transaction is the
  enforcement. Empty/whitespace `--alias` is `None` at parse time (CLI and
  request DTO). Error code and text identical from CLI and daemon paths.
- D3 `crates/atm-core/src/caller_context.rs` `resolve_roster_alias` and its
  callers: a canonical name in the addressed team wins; otherwise a roster
  alias resolves database-wide to its owner's `(team, agent_name)`; the
  resolved team replaces the implicit caller team for the recipient. Explicit
  `@team` still restricts to that team. `.atm.toml` `[atm].aliases` is
  no longer consulted (D-06, D4d).
- D4a One table-driven permutation test (`unique_name_permutations`) that
  seeds a fixture database with existing members `{team, name, alias|None}`
  and drives every proposed `{team, name, alias|None}` case from matrix
  section A through the roster store, asserting accept/reject and the
  conflicting `(team, member)` named in the error. Rand: "with the
  requirement added, it should be fairly easy to create all permutations of
  name/alias that are valid/invalid for unit tests". The permutation set is
  the product of: alias present/empty/whitespace/absent × collides with
  other-team canonical / other-team alias / same-team canonical / same-team
  alias / nothing × whether the colliding member itself carries an alias.
- D4 Tests for every **GAP** row in the matrix (A-06, A-07, A-09, A-10,
  A-14..A-16, A-18..A-25, B-01, B-03..B-07, C-05, D-03, D-06, D-08, D-10,
  D-12, D-14, E-02, F-06, G-02, G-03). Name each test after its matrix id
  (`unique_name_a06_...`). Concurrency tests (A-21, A-22) use explicit
  synchronisation and a hard bounded deadline, never retries or widened
  timeouts. Update the matrix file: flip each row from **GAP** to
  `covered <test>` with path:line.
- D4b Upgrade (REQ-ROSTER-NAME-009, matrix A-26..A-29, F-07): no
  migration touches existing rows; a database seeded with duplicate
  effective names opens and reads normally; the next roster write that
  leaves a duplicate in place fails with the REQ-ROSTER-NAME-003 error
  listing every conflicting pair; aliasing or removing one side clears it.
  Because the check is on the written roster's unique_names against other
  teams, a write to T2 must also fail while T2 itself still contains a row
  colliding with T1 (A-27), and succeed once T2's own conflict is aliased
  (A-28). `atm doctor` (team scope, existing pane-alias section) lists
  pre-existing duplicates for the caller's team.
- D4c Alias parity (REQ-ROSTER-NAME-010, matrix D-15..D-17): inventory
  every clap argument, option and env var in `crates/atm/src` that names a
  member (write the inventory into matrix row D-15). The single
  substitution point is daemon ingress (`atm-http-runtime` request
  handlers and peer receive) against the in-memory roster; the CLI
  forwards tokens unchanged and performs no alias query. Rand
  (2026-09-07): "the substitution point should be the point where
  team-member is checked against immutable roster in RAM already."; "It
  should simply change to instead of returning a bool/enum
  (member-valid), it would return (member-valid, member-name)". Do not
  add a separate resolution pass: change the existing RAM-roster
  membership check to return `(valid, canonical member name)` and make
  its callers carry the returned name forward. Existing CLI-side
  canonicalisation may stay only where it is not a roster query; any
  CLI-side roster lookup for alias resolution is removed. Add a parity
  test that drives each D-15 entry through the daemon request boundary
  with the canonical name and with the alias (bare and `@team`) and
  asserts identical daemon-side handling and persisted rows. D3's
  database-wide alias resolution lives at that same ingress point.
- D4d Remove `.atm.toml [atm].aliases` from atm (Rand: ".atm.toml is
  ONLY used by hmux and 'atm doctor' to display a warning if alias is not
  consistent. NOTHING else in atm uses .atm.toml alias."): delete the
  alias-table resolution and its config type (`config/aliases.rs` and
  callers in send/read/mailbox/identity); the existing doctor pane-alias
  mismatch warning (REQ-ROSTER-NAME-008) is the only `.atm.toml` alias
  reader left; matrix D-06.
- D5 `docs/requirements.md` §3.3.2 wording corrections only if the
  implementation forces one; quote Rand, never author a rule.
- D6a AY-QA-001 (blocking, phase gate): bump `HTTP_API_VERSION` once (minor) covering the additive wire changes of AY.3/AY.13/AY.14 and this sprint; state it in the PR body ADR-061 section. Also closes RBQA-AY-F001 and ATM-QA-AY-002 (matrix A-06/A-07 at the store layer, D-03).
- D6 Close AY14-QA-003: quality-mgr owns closure; reference the record in
  the PR body.

## Out of scope

- B-08 alias equal to a team name: accept as today, no new check.
- `crates/atm-daemon/**` (frozen legacy): empty diff.
- Any change to what is persisted in message/ack/audit/task rows beyond
  asserting they are canonical (E-02).

## Acceptance criteria

- AC1 Every matrix row A-01..A-25 has a named passing test; A-06 and A-07
  fail on f6c9d47b8 and pass on the sprint head.
- AC2 Every GAP row in sections B–G (minus B-08) has a named passing
  test, A-26..A-29 and F-07 included, and the matrix file shows no remaining **GAP** except B-08.
- AC3 `rg 'ensure_canonical_member_name_available|ensure_alias_available|validate_database_wide_aliases' crates/` returns nothing; the store transaction is the only enforcement point (A-23 test drives the store directly).
- AC4 `just validate && cargo test -p atm-core -p atm -p atm-herdr -p atm-http-runtime -p atm-storage-rusqlite -p atm-daemon-bootstrap --all-features` green; fmt and clippy clean before every push.
- AC5 Empty diff under `crates/atm-daemon/`.
- AC6 PR body: ADR-061 statement (no governed-interface change, or the minor bump from D1), and the list of matrix ids covered.

## Dispatch and PR topology

- Worktree from `integrate/phase-ay` at f6c9d47b8 or later; single branch,
  PR targets `integrate/phase-ay`; push with `gh stack push` after
  `gh stack link --base integrate/phase-ay`.
