# Member naming, alias and Herdr agent-name — test permutation matrix

Requirements: `docs/requirements.md` §3.3.2 (`REQ-ROSTER-NAME-001..008`).
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
| A-01 | T1:bob | add T1:bob | reject, same-team duplicate | covered `ensure_member_absent` (member_mutation.rs:441) |
| A-02 | none | add T1:bob (no alias) | accept, first of its name | covered `cross_team_member_names_require_alias_but_first_occurrence_does_not` (member_mutation.rs:1215) |
| A-03 | T1:bob | add T2:bob (no alias) | reject, names T1 and `--alias` remedy | covered (same test) |
| A-04 | T1:bob | add T2:bob (bobby) | accept | covered (same test) |
| A-05 | T1:bob | add T2:robert (bob) | reject, alias equals a unique_name | covered `alias_must_not_collide_with_member_name_or_team_alias` (member_mutation.rs:1181) |
| A-06 | T1:robert (bob) | add T2:bob (no alias) | reject, canonical equals an existing alias | **GAP** — AY14-QA-003 (quality-mgr, 2026-09-07); `ensure_canonical_member_name_available` (member_mutation.rs:480) ignores alias fields |
| A-07 | T1:robert (bob) | add T1:bob (no alias) | reject, same team, canonical equals alias | **GAP** — same defect, own team is skipped |
| A-08 | T1:robert (bob) | add T2:sam (bob) | reject, alias vs alias | covered (member_mutation.rs:1181) |
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
| A-26 | pre-upgrade db already holds T1:bob and T2:bob (no aliases) | open db, `atm members`/read paths | no error, no migration, rows unchanged | **GAP** (REQ-ROSTER-NAME-009) |
| A-27 | same seed | add T2:carol (the hmux launch add-member) | reject naming (T1,bob)/(T2,bob) and `--alias` remedy; roster unchanged | **GAP** (REQ-ROSTER-NAME-009) |
| A-28 | same seed | set T2:bob alias=bobby, then add T2:carol | both accept | **GAP** (REQ-ROSTER-NAME-009) |
| A-29 | same seed | remove T2:bob, then add T2:carol | both accept | **GAP** (REQ-ROSTER-NAME-009) |

## B. Grammar (REQ-ROSTER-NAME-005)

| ID | Input | Expected | Status |
|----|-------|----------|--------|
| B-01 | alias with `/`, space, `@`, `.` | reject (ATM segment rule) | covered `resolve_agent_name_rejects_invalid_alias_target`, `resolve_recipient_rejects_invalid_alias_target` (send/tests.rs:1127) — add-member path **GAP** |
| B-02 | Herdr member, alias `Team_Lead` (uppercase) | reject | covered `herdr_alias_uses_herdr_agent_name_validation` (member_mutation.rs:1158) |
| B-03 | Herdr member, no alias, canonical `Team-Lead` | reject at add (effective name fails Herdr grammar) | **GAP** |
| B-04 | Herdr member, alias 33 chars / leading digit / leading `-` | reject | covered partially (`herdr_agent_name_uses_the_live_agent_grammar`, delivery_channel.rs:323, grammar only) — add-member **GAP** |
| B-05 | non-Herdr member, alias `Team_Lead` | accept (ATM rule only) | covered `add_member_persists_alias_without_a_herdr_backend` (member_mutation.rs:1122) — uppercase variant **GAP** |
| B-06 | set-member backend → Herdr on a member whose effective name fails Herdr grammar | reject | covered `validate_effective_herdr_agent_name` (member_mutation.rs:500) — test **GAP** |
| B-07 | alias equal to reserved `atm-daemon` | reject | **GAP** |
| B-08 | alias equal to a team name | accept (teams and agents are separate namespaces) — confirm | **GAP** (decision recorded as accept unless Rand objects) |

## C. Herdr targeting (REQ-ROSTER-NAME-006)

| ID | Case | Expected | Status |
|----|------|----------|--------|
| C-01 | member with alias: prompt / get / wait / list / presence | Herdr call uses alias | covered `shared_herdr_server_prompts_each_team_by_its_roster_alias` (herdr_queue_wake.rs:1318), `local_message_received_backend_reads_optional_herdr_alias` (delivery_channel.rs:444) |
| C-02 | member without alias | Herdr call uses canonical, byte-identical to pre-AY.14 | covered AC1 (existing tests unchanged) |
| C-03 | two teams, same canonical, one aliased, one Herdr server | each receives own prompts and presence | covered (herdr_queue_wake.rs:1318) |
| C-04 | stored alias invalid for Herdr (legacy row) | skipped with log, no panic, other members unaffected | covered `local_message_received_backend_treats_invalid_herdr_alias_as_absent` (delivery_channel.rs:465) + AY14-QA-002 closure |
| C-05 | doctor presence probe for aliased member | probes alias, reports both names | **GAP** — assert log/doctor line shows `member` and `herdr_agent` |

## D. Ingress replacement and resolution (REQ-ROSTER-NAME-007)

| ID | Input | Expected | Status |
|----|-------|----------|--------|
| D-01 | `atm send <alias>` same team | canonical recipient | covered `roster_alias_resolves_to_canonical_member` (send/recipient.rs:81) |
| D-02 | `atm send <alias>@<team>` | canonical recipient in that team | covered `roster_alias_resolves_for_implicit_and_explicit_team_targets` (send/recipient.rs:96) |
| D-03 | `atm send <alias>` from a different team, no `@team` | resolves database-wide to the alias owner's team | **GAP** — `resolve_roster_alias` (caller_context.rs:59) is team-scoped; derived from Rand "using the alias for cross-team messaging has value independent of herdr" |
| D-04 | `atm send <name>` where name is a canonical member of the addressed team AND an alias elsewhere | canonical in the addressed team wins | covered `canonical_name_wins_over_historical_alias_collision` (send/recipient.rs:127) |
| D-05 | unknown alias | existing canonical error unchanged | covered `unknown_roster_alias_preserves_the_canonical_parse_result` (send/recipient.rs:113) |
| D-06 | `.atm.toml` alias and roster alias both define the token | `.atm.toml` table first | **GAP** |
| D-07 | `ATM_IDENTITY=<alias>` | canonical sender; persisted `from` canonical; observation dropped | covered `canonicalize_caller_context_replaces_an_ingress_alias_and_drops_alias_attestation` (caller_context.rs:389) |
| D-08 | `--as <alias>` | same as D-07 | covered `send_sender_identity_applies_alias_to_hook_identity` (identity/mod.rs:195) — CLI `--as` end-to-end **GAP** |
| D-09 | `atm read --as <alias>`, `--from <alias>`, peek | canonicalised before mailbox lookup | covered `read_ingress_canonicalizes_alias_caller_target_and_from_filter` (read/mod.rs:833), `resolve_target_canonicalizes_alias_before_mailbox_lookup` (mailbox/source.rs:252) |
| D-10 | `atm ack` as alias | canonical | **GAP** |
| D-11 | `set-member <alias>` / `remove-member <alias>` | canonical before persistence | covered `update_and_remove_member_canonicalize_alias_arguments_before_persistence` (member_mutation.rs:1334) |
| D-12 | self-send via own alias | rejected as self-send after replacement | **GAP** |
| D-13 | alias@team.remote-host (cross-host) | forwarded unresolved; receiver ingress replaces | **GAP** — behaviour to confirm with Rand before test |
| D-14 | alias used as `--chat-id`/qualified identity forms | canonical | **GAP** |
| D-15 | inventory: every clap argument/option/env var in `crates/atm/src` that names a member, listed here by command and flag | each entry is exercised by D-16 | **GAP** — arch-ctm produces the inventory in this row's sub-table (REQ-ROSTER-NAME-010) |
| D-16 | for every D-15 entry: run once with canonical name, once with alias (bare and `@team`) | identical outbound request/wire payload and identical persisted rows; observation attested to alias dropped | **GAP** (REQ-ROSTER-NAME-010; Rand: "if all prompts are written for either member name or alias, it will work the same") |
| D-17 | `--from <alias>` and `--to <alias>` filters on read/peek/inbox | same result set as canonical | **GAP** (D-09 covers `--from` on read only) |

## E. Persistence (REQ-ROSTER-NAME-007, "never in the database")

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
| F-07 | db holds duplicate effective names involving the caller's team | doctor lists each conflicting `(team, member)` pair and the `--alias` remedy | **GAP** (REQ-ROSTER-NAME-009) |

## G. Operator-facing errors (REQ-ROSTER-NAME-003)

| ID | Case | Expected | Status |
|----|------|----------|--------|
| G-01 | A-03 error text | names conflicting team and member, states `--alias` remedy | covered (member_mutation.rs:1215) |
| G-02 | A-06 error text | names the member that owns the alias and its team | **GAP** |
| G-03 | error is identical from CLI and daemon paths | same code and message | **GAP** |
