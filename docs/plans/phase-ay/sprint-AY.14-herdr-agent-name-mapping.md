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
  continue to use team-lead.  alias would be aceptable at all user/agent
  facing interfaces and would be immediately replaced" ... "and would
  immediately be replaced at the ingress interface."
- "additional requirements:  alias MUST be unique for atm database (meets
  herdr requirements)"

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
- AC5 Boundary TOMLs untouched unless the boundary guard requires a
  record update for the new newtype; if so, say which in the PR.

## Dispatch and PR topology

Not stacked. Branch from `integrate/phase-ay` head; plain `git push`; open
the PR (not draft) against `integrate/phase-ay` on the first push. Sized to
one context window; if D2/D6 will not fit, land D1/D3/D4/D5 first, push,
checkpoint to fenix, then continue on the same branch.
