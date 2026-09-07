# Member naming, alias and Herdr agent-name — test permutation matrix

Requirements: `docs/requirements.md` §3.3.2 (`REQ-ROSTER-NAME-001..010`).
Invariant under test: `unique_name(member) = alias ?? agent_name` is unique
across every team roster in the ATM database (Rand, 2026-09-07: "herdr agent
name = alias. if alias is null/empty, alias would be equal to member name";
"there should be a query across all team roster for 'unique-name' which
would return alias ?? name").

Status column: `covered` names the existing test; `GAP` is owed by AY.15.
Paths are relative to `crates/`. Line numbers as of integrate/phase-ay
f6c9d47b8.

## A. Roster write: uniqueness of `unique_name` (REQ-ROSTER-NAME-001..004)

Notation: `T1:bob` = member `bob` in team T1; `(x)` = alias x.

| ID | Existing rows | Write | Expected | Status |
|----|---------------|-------|----------|--------|
| A-01 | T1:bob | add T1:bob | reject, same-team duplicate | covered `unique_name_permutations` (atm-storage-rusqlite/src/roster_store.rs:437) |
| A-02 | none | add T1:bob (no alias) | accept, first of its name | covered `cross_team_member_names_require_alias_but_first_occurrence_does_not` (member_mutation.rs:1215) |
| A-03 | T1:bob | add T2:bob (no alias) | reject, names T1 and `--alias` remedy | covered (same test) |
| A-04 | T1:bob | add T2:bob (bobby) | accept | covered (same test) |
| A-05 | T1:bob | add T2:robert (bob) | reject, alias equals a unique_name | covered `unique_name_permutations` (atm-storage-rusqlite/src/roster_store.rs:437) |
| A-06 | T1:robert (bob) | add T2:bob (no alias) | reject, canonical equals an existing alias | **GAP** — AY14-QA-003 (quality-mgr, 2026-09-07); `ensure_canonical_member_name_available` (member_mutation.rs:480) ignores alias fields |
| A-07 | T1:robert (bob) | add T1:bob (no alias) | reject, same team, canonical equals alias | **GAP** — same defect, own team is skipped |
| A-08 | T1:robert (bob) | add T2:sam (bob) | reject, alias vs alias | covered `unique_name_permutations` (atm-storage-rusqlite/src/roster_store.rs:437) |
| A-09 | T1:bob (bobby) | add T2:bob (no alias) | accept: unique_names are bobby and bob | **GAP** — currently rejected; over-strict, `ensure_canonical_member_name_available` compares canonical names only |
| A-10 | T1:bob (bobby) | add T2:sam (bob) | accept: unique_names bobby, bob | **GAP** — currently rejected by `ensure_alias_available` (alias vs canonical of an aliased member) |
| A-11 | T1:bob (bobby) | add T2:bob (bobby) | reject, alias vs alias | covered (member_mutation.rs:1181) |
| A-12 | T1:bob, T2:bob (bobby) | set-member T2:bob clear alias | reject, would collide with T1:bob | covered `clearing_alias_is_rejected_when_the_canonical_name_is_cross_team_duplicate` (member_mutation.rs:1260) |
| A-13 | T1:bob (bobby) | set-member T1:bob clear alias | accept | covered `update_member_can_clear_an_existing_alias` (member_mutation.rs:1295) |
| A-14 | T1:bob (bobby), T2:sam | set-member T2:sam alias bobby | reject, alias vs alias | **GAP** — set-member alias-change path against an existing alias |
| A-15 | T1:bob (bobby), T2:sam | set-member T2:sam alias bob | accept (unique_names bobby, bob) | **GAP** |
| A-16 | T1:bob, T2:sam | set-member T2:sam alias bob | reject, alias vs canonical unique_name | **GAP** — set-member path |
| A-17 | T1:bob (bobby) | add T1:sam (bobby) | reject, same-team alias duplicate | covered `validate_database_wide_aliases` in-roster check (roster_store.rs:204) |
| A-18 | T1:bob | add T2:bob with `--alias ""` / whitespace | same as A-03: reject | **GAP** — empty alias must equal "no alias" (REQ-ROSTER-NAME-002 definition) |
| A-19 | T1:bob (bobby) | remove T1:bob, then add T2:sam (bobby) | accept, name freed | **GAP** |
| A-20 | T1:bob | delete team T1, then add T2:bob | accept, name freed | **GAP** |
| A-21 | none | concurrent add T1:bob and T2:bob, no alias | exactly one succeeds | **GAP** — `roster_aliases_are_globally_unique_under_the_single_writer_lane` (atm-storage-rusqlite/src/lib.rs:3887) covers alias vs alias only; canonical check runs outside the store transaction |
| A-22 | none | concurrent add T1:robert (bob) and T2:bob | exactly one succeeds | **GAP** |
| A-23 | T1:bob | daemon/HTTP member-add T2:bob, no alias | reject in the store, CLI not bypassable | **GAP** — check lives in `team_admin` only; must move into `save_roster` transaction (roster_store.rs:91) |
| A-24 | T1:bob | restore/import a roster containing T2:bob | reject | **GAP** |
| A-25 | T1:Bob | add T2:bob | accept, exact comparison, no case folding | **GAP** — document; Herdr grammar excludes uppercase anyway (B-03) |
| A-26 | pre-upgrade db already holds T1:bob and T2:bob (no aliases) | open db, `atm members`/read paths | no error, no migration, rows unchanged | covered `unique_name_a26_legacy_collision_is_readable_but_next_write_fails` (roster_store.rs) |
| A-27 | same seed | add T2:carol (the hmux launch add-member) | reject naming (T1,bob)/(T2,bob) and `--alias` remedy; roster unchanged | covered `unique_name_a27_legacy_collision_blocks_an_unrelated_next_write` (roster_store.rs) |
| A-28 | same seed | set T2:bob alias=bobby, then add T2:carol | both accept | covered `unique_name_a28_aliasing_the_legacy_conflict_allows_the_next_write` (roster_store.rs) |
| A-29 | same seed | remove T2:bob, then add T2:carol | both accept | covered `unique_name_a29_removing_the_legacy_conflict_allows_the_next_write` (roster_store.rs) |

## B. Grammar (REQ-ROSTER-NAME-005)

| ID | Input | Expected | Status |
|----|-------|----------|--------|
| B-01 | alias with `/`, space, `@`, `.` | reject (ATM segment rule) | covered `unique_name_b01_rejects_invalid_atm_aliases_at_add_and_update` (atm-core/src/team_admin/member_mutation.rs:1166) |
| B-02 | Herdr member, alias `Team_Lead` (uppercase) | reject | covered `herdr_alias_uses_herdr_agent_name_validation` (member_mutation.rs:1158) |
| B-03 | Herdr member, no alias, canonical `Team-Lead` | reject at add (effective name fails Herdr grammar) | covered `unique_name_b03_rejects_invalid_canonical_herdr_member_name` (member_mutation.rs) |
| B-04 | Herdr member, alias 33 chars / leading digit / leading `-` | reject | covered `unique_name_b04_rejects_invalid_herdr_aliases_at_add_and_update` (atm-core/src/team_admin/member_mutation.rs:1219) |
| B-05 | non-Herdr member, alias `Team_Lead` | accept (ATM rule only) | covered `add_member_persists_alias_without_a_herdr_backend` (member_mutation.rs) |
| B-06 | set-member backend → Herdr on a member whose effective name fails Herdr grammar | reject | covered `unique_name_b06_rejects_switching_an_invalid_canonical_name_to_herdr` (member_mutation.rs) |
| B-07 | alias equal to reserved `atm-daemon` | reject | covered `unique_name_b07_rejects_reserved_daemon_alias_at_add_and_update` (member_mutation.rs) |
| B-08 | alias equal to a team name | accept (teams and agents are separate namespaces) — confirm | **GAP** (decision recorded as accept unless Rand objects) |

## C. Herdr targeting (REQ-ROSTER-NAME-006)

| ID | Case | Expected | Status |
|----|------|----------|--------|
| C-01 | member with alias: prompt / get / wait / list / presence | Herdr call uses alias | covered `shared_herdr_server_prompts_each_team_by_its_roster_alias` (herdr_queue_wake.rs:1318), `local_message_received_backend_reads_optional_herdr_alias` (delivery_channel.rs:444) |
| C-02 | member without alias | Herdr call uses canonical, byte-identical to pre-AY.14 | covered AC1 (existing tests unchanged) |
| C-03 | two teams, same canonical, one aliased, one Herdr server | each receives own prompts and presence | covered (herdr_queue_wake.rs:1318) |
| C-04 | stored alias invalid for Herdr (legacy row) | skipped with log, no panic, other members unaffected | covered `local_message_received_backend_treats_invalid_herdr_alias_as_absent` (delivery_channel.rs:465) + AY14-QA-002 closure |
| C-05 | doctor presence probe for aliased member | probes alias, reports both names | covered `unique_name_c05_presence_serializes_the_effective_herdr_agent` (doctor/mod.rs) |

## D. Ingress replacement and resolution (REQ-ROSTER-NAME-007, -010)

Substitution point (Rand, 2026-09-07): daemon ingress against the in-memory roster; the CLI does not query for aliases before sending. Rows below that name a CLI test path are satisfied at the daemon request boundary those commands hit.

| ID | Input | Expected | Status |
|----|-------|----------|--------|
| D-01 | `atm send <alias>` same team | canonical recipient | covered `roster_alias_resolves_to_canonical_member` (send/recipient.rs:81) |
| D-02 | `atm send <alias>@<team>` | canonical recipient in that team | covered `roster_alias_resolves_for_implicit_and_explicit_team_targets` (send/recipient.rs:96) |
| D-03 | `atm send <alias>` from a different team, no `@team` | resolves database-wide to the alias owner's team | covered `write_ingress_resolves_a_bare_alias_to_its_remote_owner` (send/tests.rs) |
| D-04 | `atm send <name>` where name is a canonical member of the addressed team AND an alias elsewhere | canonical in the addressed team wins | covered `canonical_name_wins_over_historical_alias_collision` (send/recipient.rs:127) |
| D-05 | unknown alias | existing canonical error unchanged | covered `unknown_roster_alias_preserves_the_canonical_parse_result` (send/recipient.rs:113) |
| D-06 | `.atm.toml` `[atm].aliases` present | ignored everywhere in atm; the only `.atm.toml` alias use is the doctor pane-alias consistency warning (F-01..F-05) | covered `load_config_ignores_retired_aliases` (config/mod.rs) and `resolve_target_forwards` (mailbox/source.rs); AY-QA-005 closure |
| D-07 | `ATM_IDENTITY=<alias>` | canonical sender; persisted `from` canonical; observation dropped | covered `canonicalize_caller_context_replaces_an_ingress_alias_and_drops_alias_attestation` (caller_context.rs:389) |
| D-08 | `--as <alias>` | same as D-07 | covered `send_sender_identity_applies_alias_to_hook_identity` (identity/mod.rs:195) — CLI `--as` end-to-end **GAP** |
| D-09 | `atm read --as <alias>`, `--from <alias>`, peek | canonicalised before mailbox lookup | covered `read_ingress_canonicalizes_alias_caller_target_and_from_filter` (read/mod.rs:833), `resolve_target_canonicalizes_alias_before_mailbox_lookup` (mailbox/source.rs:252) |
| D-10 | `atm ack` as alias | canonical | **GAP** |
| D-11 | `set-member <alias>` / `remove-member <alias>` | canonical before persistence | covered `update_and_remove_member_canonicalize_alias_arguments_before_persistence` (member_mutation.rs:1334) |
| D-12 | self-send via own alias | rejected as self-send after replacement | **GAP** |
| D-13 | `alias@team.host` (cross-host) | delivered to the canonical member on the remote host; persisted rows canonical on both hosts; sending daemon forwards the alias unchanged, receiving daemon ingress substitutes | **GAP** — Rand (2026-09-07): "alias@team.host works" |
| D-14 | alias used as `--chat-id`/qualified identity forms | canonical | **GAP** |
| D-15 | inventory: every clap argument/option/env var in `crates/atm/src` that names a member, listed here by command and flag | each entry is exercised by D-16 | **GAP** — arch-ctm produces the inventory in this row's sub-table (REQ-ROSTER-NAME-010) |
| D-16 | for every D-15 entry: run once with canonical name, once with alias (bare and `@team`) | CLI forwards the token unchanged; daemon ingress substitutes; identical daemon-side handling and identical persisted rows; observation attested to alias dropped | **GAP** (REQ-ROSTER-NAME-010; Rand: "if all prompts are written for either member name or alias, it will work the same"; substitution at daemon ingress, not the CLI) |
| D-17 | `--from <alias>` and `--to <alias>` filters on read/peek/inbox | same result set as canonical | **GAP** (D-09 covers `--from` on read only) |

## E. Persistence (REQ-ROSTER-NAME-007: alias stored in roster row + RAM roster; message/ack/audit/task rows canonical only)

| ID | Case | Expected | Status |
|----|------|----------|--------|
| E-01 | send via alias and as alias identity; inspect message rows | `from`/`to` canonical only | covered `send_aliases_are_resolved_before_any_message_is_persisted` (send/tests.rs:807) |
| E-02 | ack, audit, task-state rows after alias use | canonical only | **GAP** |
| E-03 | `rg alias` over atm-storage and mailbox write paths | roster metadata is the only write | covered AC6 (review check; keep as a lint or test) |

## F. Doctor pane-alias check (REQ-ROSTER-NAME-008)

| ID | Case | Expected | Status |
|----|------|----------|--------|
| F-01 | pane alias == roster alias | no line | covered `pane_alias_mismatch_omits_matching_alias` (commands/doctor.rs:298) |
| F-02 | differ | one line, both values | covered (commands/doctor.rs:317, output.rs:1154) |
| F-03 | roster alias absent | one line | covered (commands/doctor.rs:317) |
| F-04 | pane without alias key / other team's pane | ignored | covered (commands/doctor.rs:346) |
| F-05 | no `.atm.toml` | check skipped | covered (commands/doctor.rs:369) |
| F-06 | `--all-teams` | check not widened | **GAP** |
| F-07 | db holds duplicate effective names involving the caller's team | doctor lists each conflicting `(team, member)` pair and the `--alias` remedy | covered `unique_name_f07_reports_legacy_effective_name_conflicts_for_the_scoped_team` (doctor/mod.rs) |

## G. Operator-facing errors (REQ-ROSTER-NAME-003)

| ID | Case | Expected | Status |
|----|------|----------|--------|
| G-01 | A-03 error text | names conflicting team and member, states `--alias` remedy | covered (member_mutation.rs:1215) |
| G-02 | A-06 error text | names the member that owns the alias and its team | **GAP** |
| G-03 | error is identical from CLI and daemon paths | same code and message | **GAP** |

## H. Decision points (fenix critical review, Rand's rulings 2026-09-07)

| ID | Question | Ruling |
|----|----------|--------|
| H-01 | `atm teams add-member`/`set-member` open the roster store in-process from the CLI (`crates/atm/src/commands/retained_roster.rs`) | no change: the store transaction is the enforcement point for roster writes; daemon-ingress substitution applies to message/identity paths |
| H-02 | bare canonical name of an aliased member from a third team | Rand: "`atm send bob` would send bob based on ATM_TEAM just like today" |
| H-03 | pre-upgrade duplicate unique_names in teams already live in Herdr | Rand: "we need to change members from tmux->herder before launch today. herder rejection already exists, we simply haven't run more than 1 team per computer yet." No new mechanism |
| H-04 | alias change on a member live in Herdr | Rand: "alias change won't get picked up by herdr until team restarted (no mid-session concern)" |
| H-05 | `.atm.toml [atm].aliases` | Rand: ".atm.toml is ONLY used by hmux and 'atm doctor' to display a warning if alias is not consistent. NOTHING else in atm uses .atm.toml alias."; `[atm].aliases` removed from atm (D-06) |
| H-06 | "alias never in the database" scope | Rand: "alias MUST be in database AND in immutable roster in RAM"; message/ack/audit/task rows carry the canonical name only. The phrase "never in the database" is retired from all docs |
