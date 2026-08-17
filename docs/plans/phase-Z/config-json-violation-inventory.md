# Phase Z Config And Boundary Violation Inventory

## Purpose

Record every current production config/boundary touch-point that violates, or
risks violating, the accepted ownership model:

- ATM roster state in SQLite is canonical truth
- runtime membership decisions use ATM roster only
- `config.json` is limited to:
  - watcher-owned external ingress
  - ATM-owned projection target
  - `doctor` comparison surface
  - narrowly justified restore-shell preservation reads

This inventory is the authoritative delete / narrow / keep map for `Z.5`
through the current follow-on cleanup line.

## Reviewed Runtime Rule

Any production path not listed under `Allowed End-State Surfaces` below is a
candidate violation and must be deleted, narrowed, or rewritten before `Z.3`
canary begins.

## Allowed End-State Surfaces

These are the only approved surviving production `config.json` read/write
surfaces after `Z.10` closes:

| Surface | Final Role | Notes |
| --- | --- | --- |
| `crates/atm-core/src/config/mod.rs` | parser / serializer implementation | no generic runtime ownership; only approved callers may use it |
| `crates/atm-core/src/doctor/mod.rs` | comparison-only reader | compare Claude file state against ATM roster truth |
| watcher / reconcile import lane | external-ingress reader | imports externally changed Claude roster state into ATM roster truth |
| ATM-owned projection path | writer | writes projected Claude roster/config state from ATM roster truth |
| `crates/atm-core/src/team_admin/restore.rs` | narrow recreated-shell preservation read | may read the recreated shell's current `team-lead` / `leadSessionId`; must not read backup `config.json` as roster truth |

## Path Inventory

| Path | Current Behavior | Violation Status | Owning Sprint | Current Status | Required Resolution |
| --- | --- | --- | --- | --- | --- |
| `crates/atm-core/src/list.rs` | validates mailbox target membership through `runtime.load_team_config(...)` | delete | `Z.5` | `closed` | explicit target membership now resolves through ATM roster truth only |
| `crates/atm-core/src/read/mod.rs` | validates explicit target membership through `runtime.load_team_config(...)` | delete | `Z.5` | `closed` | explicit target membership now resolves through ATM roster truth only |
| `crates/atm-core/src/clear/mod.rs` | validates clear target membership through `runtime.load_team_config(...)` | delete | `Z.5` | `closed` | explicit target membership now resolves through ATM roster truth only |
| `crates/atm-core/src/ack/mod.rs` | validates actor/reply-team membership through `runtime.load_team_config(...)` | delete | `Z.5` | `closed` | actor and reply-target membership now resolve through ATM roster truth only |
| `crates/atm-core/src/doctor/mod.rs` | reads `config.json` and reports drift/baseline warnings | keep, narrow | `Z.5` | `closed` | compare-only reader; drift warnings now compare Claude file membership against ATM roster truth |
| `crates/atm-core/src/send/mod.rs` | performed pre-write config membership checks before Claude delivery path | rewrite | `Z.6` | `closed` | send now writes ATM durable state first, then uses store-backed `ClaudeCodeTeamRoster` for the Claude roster warning path; missing-config fallback remains post-write and only for the existing-inbox fallback case |
| `crates/atm-core/src/service_runtime.rs` | exposed generic `load_team_config(...)` runtime helper | delete / narrow | `Z.6` | `closed` | generic runtime helper removed from send-driven paths; retained runtime now exposes doctor-only compare loading plus store-backed `ClaudeCodeTeamRoster` access |
| `crates/atm-core/src/boundary/store.rs` | `ConfigIngress` behaved like a generic config/team lookup boundary | narrow | `Z.7` | `closed` | `ConfigIngress` now exposes workspace-config loading only; stale team-config DTOs were removed from the public boundary surface |
| `crates/atm-core/src/boundary_support.rs` | helper loaded team config through a generic boundary support seam | narrow | `Z.7` | `closed` | only workspace-config loading remains; generic runtime lookup support and the old startup-only roster helper are both gone |
| `crates/atm-core/src/direct_boundaries.rs` | forwarded generic `load_team_config(...)` boundary calls | narrow | `Z.7` | `closed` | generic runtime lookup forwarding was deleted and no team-config forwarder remains after `Z.8` removed the startup-only bridge |
| `crates/atm-core/src/boundary_support.rs` `hydrate_roster_from_team_config_once_at_startup_if_empty(...)` plus `crates/atm-core/src/direct_boundaries.rs` forwarding | temporary startup-only one-shot roster hydration when canonical ATM roster is empty before `Z.8` watcher ingress exists | keep, narrow then delete | `Z.8` | `closed` | helper and forwarding were deleted once watcher / reconcile became the sole external ingest path; the old allowlist entry is now historical only |
| `crates/atm-daemon/src/boundary_adapters.rs` | daemon-side `ConfigIngress` adapter exposed generic team-config load | narrow | `Z.7` | `closed` | daemon adapter now owns workspace-config loading only and no longer exposes generic team-config lookup |
| `crates/atm-daemon/src/direct_boundaries.rs` | forwarded generic daemon `load_team_config(...)` calls | narrow | `Z.7` | `closed` | generic runtime lookup forwarding was deleted from the daemon side |
| `crates/atm-daemon/src/watch_runtime.rs` | now watches Claude `config.json` alongside inbox sources for the reconcile lane | add / tighten | `Z.8` | `closed` | watch batches now include Claude team `config.json` when present so external edits reach the reconcile ingest lane |
| `crates/atm-daemon/src/reconcile_runtime.rs` | now owns external `config.json` ingest and daemon-write suppression | add / tighten | `Z.8` | `closed` | reconcile imports external Claude roster changes into canonical ATM roster truth, suppresses one matching daemon-authored projection event via a process-local journal, and clears suppression state on restart |
| `crates/atm-core/src/team_admin.rs` `list_teams` / `list_members` | reported raw `config.json` state as team/member truth | delete / rewrite | `Z.9` | `closed` | team/member listings now read canonical ATM roster truth only; Claude file drift is left to `doctor` |
| `crates/atm-core/src/team_admin.rs` `add_member` | read and mutated local `config.json` as the primary authority | rewrite | `Z.9` | `closed` | add-member now mutates ATM roster truth first, then projects the approved member set back into `config.json`; one narrow `load_team_projection_extra_for_member_add(...)` read remains allowlisted only to preserve non-roster Claude config extras until `Z.10` removes the last team-admin config read |
| `crates/atm-core/src/team_admin.rs` `.atm.toml` pane ownership assumptions | treated per-member pane routing as non-roster durable state | delete / rewrite | `Z.9` | `closed` | canonical pane metadata now lives on ATM roster-member rows and projects back to Claude `tmux_pane_id` |
| `crates/atm-core/src/schema/agent_member.rs` | modeled retained Claude metadata as copied file state without explicit ATM ownership contract | tighten | `Z.9` | `closed` | `AgentMember.tmux_pane_id` is now optional/serde-compatible and maps to canonical ATM roster `recipient_pane_id` metadata |
| `crates/atm-core/src/team_admin/restore.rs` | read current + backup `config.json` and replay backup-config roster state | delete / rewrite | `Z.10` | `closed` | restore now loads canonical ATM roster truth from the roster store, projects recreated `config.json` from ATM roster, and keeps only a narrow recreated-shell preservation read for current `team-lead` / `leadSessionId`; backup `config.json` is no longer consulted for membership |
| `crates/atm-core/src/config/mod.rs` | parser/serializer used broadly today | keep, allowlist | `Z.10` | `closed` | keep one parser implementation; approved-callers enforcement provided by `Z.12`–`Z.14` `SCB-RETAINED-001` / `SCB-WORKSPACE-001` / `SCB-SINGLETON-001` lint gates |

## Non-Violation References Reviewed

The following files mention `config.json` but are not themselves roster-truth
violations when kept in their current narrow role:

| Path | Reason |
| --- | --- |
| `crates/atm-core/src/send/missing_config_notice.rs` | path/warning construction for missing team config; not a roster-truth source |
| `crates/atm-core/src/send/alert_state.rs` | historical only — deleted in `CLAUDE-TEAMS-DIR-REMOVAL-1` / PR #554; no longer present in the codebase |
| `crates/atm-core/src/error_codes.rs` | error-code documentation |

## Post-Z.10 Boundary Violations

The following are real follow-on boundary violations even when they are not
Claude `config.json` roster-truth leaks:

| Path | Current Behavior | Violation Status | Owning Sprint | Current Status | Required Resolution |
| --- | --- | --- | --- | --- | --- |
| `crates/atm-core/src/delivery_policy.rs` and send-path error surfaces | returns opaque first-send empty-roster failure text | rewrite | `Z.11` | `closed` — Closed via Z.11 at 1bcf916e | replace the bad recovery contract with explicit `atm teams add-member` guidance and prove no hidden fallback was added |
| `crates/atm/src/commands/teams.rs` / `crates/atm/src/commands/members.rs` plus `crates/atm-core/src/team_admin.rs` | reaches `service_runtime_store::default_runtime()` through the wrong command-entry path for `teams`, `members`, and `add-member` | delete / rewrite | `Z.12` | `closed` — Closed via Z.12 at 602292e3 | route roster access through the approved `RosterStore` seam only; `teams --json`, `members --json`, and `teams add-member` must stop failing on the uninstalled default-runtime path |
| `crates/atm-core/src/team_admin.rs` `list_teams` / `list_members` | ambient `.atm.toml` / `load_config(...)` current-team reads outside the approved seam | delete / rewrite | `Z.13` | `closed` — Closed via Z.13 at 356af2dd | move current-team resolution behind the approved `ConfigIngress` / runtime seam and forbid new ambient command/team-admin reads |
| `crates/atm-core/src/lib.rs` | public crate-root re-export of `install_default_runtime_factory` leaks an ambient singleton/runtime-factory surface | delete / narrow | `Z.14` | `closed` — Closed via Z.14 at 75ebe60c | remove the broad re-export, keep only approved wrappers, and lint against new public singleton leaks |

## Static Gate Direction

Repository-local lint / `sc-lint`-candidate gates should be defined so the
follow-on implementation is mechanically checkable:

- reject production `load_claude_team_config_document(...)` or equivalent direct team
  `config.json` roster reads outside the explicit allowlist
- reject production generic team-config helper use from retained
  command/runtime paths
- reject Claude send paths that consult `config.json` before the durable ATM
  write has succeeded
- reject direct CLI-command `service_runtime_store::default_runtime()` use
  outside the approved retained-runtime install path, including `atm teams`,
  `atm members`, and `atm teams add-member`
- reject direct command/team-admin `.atm.toml` / `load_config(...)` reads
  outside the approved boundary seam
- reject new public ambient singleton/runtime-factory exposure such as broad
  crate-root re-exports of `install_default_runtime_factory`

These rule families are documented in the requirements / architecture updates
and become part of the `Z.7` implementation/validation contract.
