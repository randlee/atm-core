---
id: AY.14
phase: AY
sprint: AY.14
title: Herdr agent-name mapping — many teams on one Herdr server
branch: feature/ay14-herdr-agent-name-mapping
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay14-herdr-agent-name-mapping
integration_branch: integrate/phase-ay
stack_parent: none
pr_target: integrate/phase-ay
target: integrate/phase-ay
status: dispatched
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: parallel
parallel_with: [AY.9, AY.13]
dependency_relations:
  - prerequisite: AY.14
    dependent: none
    relation: parallel_safe
    rationale: additive roster metadata key plus the three Herdr call sites that resolve a member to a Herdr target. AY.13 owns doctor scope files; AY.14 touches only the presence-probe target resolution inside doctor, not its scope. Added by fenix 2026-09-07 from Rand's constraint.
---

# AY.14 — Herdr agent-name mapping

Rand (2026-09-07, verbatim): "We currently run w/ most teams having
'team-lead' AND 'quality-mgr' agents/panes. Will this team configuration stop
working?" and, on the one-server-per-team workaround: "will not work".

## Facts (integrate/phase-ay @ 580154548, Herdr 0.8.2)

- Herdr: agent names "must be unique among live agents" per server;
  agent commands accept "a unique live agent name or the pane ID currently
  hosting that agent". A name follows the pane occupant and is cleared when
  the agent exits.
- ATM resolves a Herdr target from the roster `agent_name` alone:
  `HerdrProcessAdapter::{prompt,get,wait}` take `AgentName` +
  `Option<HerdrSession>` (`crates/atm-herdr/src/lib.rs:248-277`), and the
  wake loop matches `agent.list` snapshots by `name`
  (`crates/atm-http-runtime/src/herdr_queue_wake.rs:393-397`).
- Result: two teams on one Herdr server cannot both have a `team-lead`
  pane. The second is refused a name by Herdr, so its nudges log
  `held_target_not_present` forever and doctor presence shows NotVisible.

## Amendment (Rand, 2026-09-07, verbatim)

- "support for alias would mean it was saved to roster" ... "and was in
  sqlite."
- "this is a send and identity resolution issue."

So the Herdr-side name is a roster alias, not a Herdr-only key. The
metadata key is `alias` (not `herdrAgent`), persisted in the members table
`metadata_json` like `herdrSession`, and it is honoured in three places:

1. Herdr target resolution (D3/D4 below, unchanged in substance).
2. Send recipient resolution: `atm send <alias>` and `<alias>@<team>`
   resolve to the canonical member of that team before validation,
   self-send checks and mailbox lookup (same rules as the existing
   `.atm.toml` aliases in docs/requirements.md "Alias rules"; the
   `.atm.toml` table stays and is consulted first, roster alias second).
3. Sender identity resolution: `ATM_IDENTITY=<alias>` or `--as <alias>`
   resolves to the canonical member; canonical identity remains the
   routing, validation and audit identity; persisted `from` is canonical.

Alias validation: ATM name rules (`validate_path_segment`) and, when the
member backend is Herdr, also Herdr's `[a-z][a-z0-9_-]{0,31}`. An alias
must be unique across the whole ATM database (every team in the roster
store, not just the member's team; see Requirement below) and must not
equal any member's canonical name in any team. CLI flag is `--alias <name>` (D2), not
`--herdr-agent`. Convention for Herdr collisions: `<identity>_<team>`,
e.g. `team-lead_atm-dev`. Add D7: recipient and identity resolution tests
for alias, alias@team, and unknown alias (falls through to canonical parse
error unchanged).

## Required behaviour

- A roster member may carry `metadata_json["herdrAgent"]`, the Herdr-side
  live agent name, validated against Herdr's `[a-z][a-z0-9_-]{0,31}`.
- When absent, the Herdr target is the ATM `agent_name` (today's behaviour,
  byte-for-byte).
- Every Herdr call for a member (prompt, get, wait, list matching) uses the
  resolved Herdr target, never the ATM identity directly. Log lines and
  doctor findings show both (`member = team/agent`, `herdr_agent = ...`).
- Convention documented for operators: `<team>-<identity>` (e.g.
  `sc-lint-team-lead`); ATM never renames Herdr panes itself.

## Deliverables

- D1 `crates/atm-core/src/delivery_channel.rs`: `LocalMessageReceivedBackend::Herdr`
  gains `agent: Option<HerdrAgentName>` read from `herdrAgent` next to
  `herdrSession`; invalid values are logged and treated as absent (same
  policy as `herdrSession`). `crates/atm-core/src/team_admin.rs` roster
  entry gains `herdr_agent` (`rename = "herdrAgent"`, default, skip-if-none)
  so `atm members` JSON shows it.
- D2 `crates/atm/src/commands/teams.rs`: `--herdr-agent <name>` on the
  add-member and update-member commands, valid only with `--backend herdr`
  (mirror the `--herdr-session` validation); update-member with
  `--herdr-agent ''` or a `--clear-herdr-agent` flag removes it
  (`member_mutation.rs:595` shows the `herdrSession` removal pattern).
- D3 `crates/atm-herdr/src/lib.rs`: `HerdrAgentName` newtype (Herdr regex
  validation) alongside `AgentName`; the adapter trait's `agent` parameter
  becomes `&HerdrAgentName`. `From<&AgentName>` for the default mapping.
- D4 `crates/atm-http-runtime/src/herdr_queue_wake.rs`: `HerdrCandidate`
  carries `herdr_agent: HerdrAgentName`; `collect_idle_members` keys the
  snapshot map by that; `drain_eligible` prompts with it. The doctor
  presence probe (AYP-R6-002, `crates/atm-core/src/doctor/`) resolves the
  same way. No other doctor edits (AY.13 owns scope).
- D5 Tests: roster round-trip of `herdrAgent`; invalid value ignored with a
  warn; wake loop with two members `a/team-lead` (`herdrAgent = a-team-lead`)
  and `b/team-lead` (`herdrAgent = b-team-lead`) against one fake `list`
  returning both names resolves each to its own snapshot and prompts the
  right one; member without `herdrAgent` still matches by identity;
  CLI flag validation.
- D6 Docs: `docs/agent-conventions.md` (or the Herdr roster section that
  documents `--herdr-session`) gains the key, the CLI flag, and the naming
  convention.

## Requirement (Rand, 2026-09-07, verbatim)

- "adding a roster persisted alias makes a lot of sense.  For requirements,
  the alias should never be used in database."
- "i.e. if team-lead = atm-dev-lead (alias), all entries in database should
  continue to use team-lead.  alias would be acceptable at all user/agent
  facing interfaces and would be immediately replaced" [spelling normalized] ... "and would
  immediately be replaced at the ingress interface."
- "additional requirements:  alias MUST be unique for atm database (meets
  herdr requirements)"
- "if an alias is used, the alias would be the herdr agent name."

There is exactly one name field: when a member has an alias, that alias is
the Herdr agent name the adapter targets (prompt/wait/get, `agent.list`
snapshot matching, wake loop keys); when it has none, the canonical name is.
No separate Herdr-name key exists or is accepted.

- "what this really means is that 'alias' is always used by herdr IF it is
  present.  And all agents can use it in place of agent name."

So the alias is not Herdr-collision-only: any agent may address or identify
a member by alias wherever an agent name is accepted (send recipient,
`--as`/`ATM_IDENTITY`, `--team`-scoped member arguments, ack/read filters,
roster commands), and Herdr always uses it when present.

- "one more requirement: atm teams add_member must reject a duplicate name
  w/out an alias"

`atm teams add-member` rejects a member whose canonical name already exists
in any other team of the roster store unless `--alias` is given (and that
alias passes the database-wide uniqueness rule above). The error names the
conflicting team and the `--alias` remedy. Applies to every backend, as
written. Consequence for operators: a second team gaining `team-lead` or
`quality-mgr` must supply an alias at add time; the hmux spawn path that
creates teams outside ATM must pass one.
- Considered and withdrawn (Rand, 2026-09-07): rejecting `team-lead`,
  `quality-mgr`, `publisher` by name. Rand: "this is probably extreme for
  requirements, naming hardcoded commonly used names." No hardcoded name
  list; AC8's duplicate-name rule is the only gate.

Uniqueness is enforced where the alias is written (`add-member --alias`,
`set-member --alias`): the roster store rejects an alias already held by
any member of any team, and an alias equal to any canonical member name in
any team, with an error naming the conflicting team. Enforced under the
roster write lane so two concurrent writers cannot both succeed.

The alias is stored once, as the member's roster attribute (`alias` in
`metadata_json`). It is resolved to the canonical name at the CLI/runtime
edge and never written anywhere else: message rows (`from`, `to`,
recipients), queue and outbox rows, audit and delivery records, task-state
rows, graft and cross-host envelopes all carry the canonical name only. No
table gains an alias column and no query matches on the alias. A resolver
that leaks the alias past the edge is a blocking finding.

## Spawn-side alias source (Rand, 2026-09-07, verbatim)

- "My concern really is every team launches w/ duplicate members.  how do
  we manage this?"
- "i do not want every member to have team appended to their name"
- "ok, so this is an hmux issue for me.  other users (that don't use hmux)
  would get a hard fail on launch"
- "ok, let's add alias to .atm.toml on atm-core since this is the model
  repo others follow."

Decision as recorded: AC8 stays a reject (no automatic alias derivation).
The alias for a shared role name is declared in the repo's `.atm.toml`, and
atm-core's own `.atm.toml` is the model other repos copy. Rand's chosen
values for atm-core (2026-09-07, verbatim: "I would like alias's to be:
team-lead -> atm-lead, publisher -> atm-publisher, quality-mgr ->
atm-quality"). Members with unique names (`arch-ctm`, `cipher`, `fenix`)
get none. The `<identity>_<team>` convention above is a suggestion for
other repos, not a rule.

Rand, 2026-09-07, verbatim: ".atm.toml does not need to get processed by
daemon for alias"; "this sounds like vague requirements."; "It sounds like
you took an idea 'let's add alias to .atm.toml so hmux can use it' and
turned in into rust code".

D8, corrected (2026-09-07): `alias = "<name>"` is added to the
`team-lead`, `quality-mgr` and `publisher` entries under
`[[rmux.windows.panes]]` in atm-core's `.atm.toml`, as data for hmux (the
external spawner), which passes it to `atm teams add-member --alias`. No
atm crate reads this key: not the CLI, not the daemon, not
atm-http-runtime. There is no default-alias lookup, no precedence logic
and no Rust for this in AY.14. The only alias input to atm is the
`--alias` argument (and its daemon-path equivalent) from D7/AC8. The
earlier D8 draft that had `add-member` reading the pane alias is
withdrawn. Document the key in docs/requirements.md next to the existing
`[atm].aliases` rules as a spawner-consumed key that atm ignores.

## Acceptance criteria

- AC1 Members without `herdrAgent` behave exactly as before (existing
  tests unchanged and green).
- AC2 Two teams each with `team-lead` on one Herdr server, mapped to
  distinct Herdr names, each receive their own prompts and presence
  results (D5 test).
- AC3 `just validate`; `cargo test -p atm-core -p atm -p atm-herdr
  -p atm-http-runtime --all-features`; fmt and clippy clean before every
  push.
- AC4 Empty diff under `crates/atm-daemon/` (frozen legacy). The
  `HerdrProcessAdapter` trait signature change is the only public API
  change; record it as a minor bump under ADR-061 in the PR description
  (additive metadata, no wire or SQLite schema change).
- AC6 Alias never persisted outside the member's roster attribute: a test
  sends via alias and as an alias identity and asserts every stored
  message/audit row carries canonical names only; `rg alias` over
  crates/atm-storage and crates/atm-core/src/mailbox shows no write path
  other than the roster metadata.
- AC7 Alias uniqueness is database-wide: a test adds `alias` to a member of
  team A, then attempts the same alias on a member of team B and on a
  member whose canonical name equals it; both are rejected. Concurrent
  writers of the same alias: exactly one succeeds.
- AC8 `add-member` with a canonical name already present in another team
  and no `--alias` is rejected with an error naming that team; the same
  call with a unique `--alias` succeeds; the first member of that name in
  the database is accepted without an alias. Same check on the daemon
  member-add path so the CLI cannot be bypassed.
- AC9 atm-core `.atm.toml` declares `alias` on the team-lead, quality-mgr
  and publisher panes only (`atm-lead`, `atm-quality`, `atm-publisher`).
  `rg alias` over `crates/` shows no reader of the pane `alias` key; the
  existing `.atm.toml` parsing tests still pass with the key present
  (unknown-key tolerance, no new struct field).
- AC5 Boundary TOMLs untouched unless the boundary guard requires a
  record update for the new newtype; if so, say which in the PR.

## Dispatch and PR topology

Not stacked. Branch from `integrate/phase-ay` head; plain `git push`; open
the PR (not draft) against `integrate/phase-ay` on the first push. Sized to
one context window; if D2/D6 will not fit, land D1/D3/D4/D5 first, push,
checkpoint to fenix, then continue on the same branch.
