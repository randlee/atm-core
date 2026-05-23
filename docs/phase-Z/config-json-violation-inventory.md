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
| `crates/atm-core/src/list.rs` | validates mailbox target membership through `runtime.load_team_config(...)` | delete | `Z.5` | `open` | use ATM roster truth only |
| `crates/atm-core/src/read/mod.rs` | validates explicit target membership through `runtime.load_team_config(...)` | delete | `Z.5` | `open` | use ATM roster truth only |
| `crates/atm-core/src/clear/mod.rs` | validates clear target membership through `runtime.load_team_config(...)` | delete | `Z.5` | `open` | use ATM roster truth only |
| `crates/atm-core/src/ack/mod.rs` | validates actor/reply-team membership through `runtime.load_team_config(...)` | delete | `Z.5` | `open` | use ATM roster truth only |
| `crates/atm-core/src/doctor/mod.rs` | reads `config.json` and reports drift/baseline warnings | keep, narrow | `Z.5` | `open` | preserve compare-only role; do not let `doctor` become a generic runtime lookup seam |
| `crates/atm-core/src/send/mod.rs` | performs pre-write config membership checks before Claude delivery path | rewrite | `Z.6` | `open` | durable ATM write first; if inbox exists, inbox write still attempted; only then return Claude roster warning when member is absent from `config.json` |
| `crates/atm-core/src/service_runtime.rs` | exposes generic `load_team_config(...)` runtime helper | delete / narrow | `Z.6` | `open` | remove generic runtime helper from normal command flows; replace with ATM roster / `ClaudeCodeTeamRoster` access |
| `crates/atm-core/src/boundary/store.rs` | `ConfigIngress` behaves like a generic config/team lookup boundary | narrow | `Z.7` | `open` | make the boundary watcher-ingress / approved comparison oriented rather than a general runtime roster lookup |
| `crates/atm-core/src/boundary_support.rs` | helper loads team config through generic boundary support seam | narrow | `Z.7` | `open` | keep only approved ingress / comparison operations |
| `crates/atm-core/src/direct_boundaries.rs` | forwards generic `load_team_config(...)` boundary call | narrow | `Z.7` | `open` | remove generic runtime lookup forwarding |
| `crates/atm-core/src/boundary_support.rs` `hydrate_roster_from_team_config_if_empty(...)` plus `crates/atm-core/src/direct_boundaries.rs` forwarding | temporary startup-only one-shot roster hydration when canonical ATM roster is empty before `Z.8` watcher ingress exists | keep, narrow then delete | `Z.7` | `open` | `Z.7` must rename this exact helper to `hydrate_roster_from_team_config_once_at_startup_if_empty(...)`, add that renamed symbol to `.just/allowlists/scb_config_allowlist.toml` with `sunset_sprint = "Z.8"`, prove it is a no-op when ATM roster is non-empty, and forbid any other generic caller; `Z.8` deletes the renamed helper and its direct-boundary forwarding once watcher / reconcile becomes the sole external ingest path before `Z.3` begins |
| `crates/atm-daemon/src/boundary_adapters.rs` | daemon-side `ConfigIngress` adapter exposes generic team-config load | narrow | `Z.7` | `open` | make daemon adapter watcher-ingress / approved comparison only |
| `crates/atm-daemon/src/direct_boundaries.rs` | forwards generic daemon `load_team_config(...)` call | narrow | `Z.7` | `open` | remove generic runtime lookup forwarding |
| `crates/atm-daemon/src/watch_runtime.rs` | does not yet own the only external `config.json` ingest path | add / tighten | `Z.8` | `open` | own external Claude config ingest and daemon-write suppression |
| `crates/atm-daemon/src/reconcile_runtime.rs` | does not yet own the only external `config.json` ingest reconciliation lane | add / tighten | `Z.8` | `open` | own external Claude config ingest reconciliation into ATM roster truth |
| `crates/atm-core/src/team_admin.rs` `list_teams` / `list_members` | reports raw `config.json` state as team/member truth | delete / rewrite | `Z.9` | `open` | report ATM roster truth; let `doctor` own drift warnings |
| `crates/atm-core/src/team_admin.rs` `add_member` | reads and mutates local `config.json` as the primary authority | rewrite | `Z.9` | `open` | mutate ATM roster truth first, then project approved member set back into `config.json` |
| `crates/atm-core/src/team_admin.rs` `.atm.toml` pane ownership assumptions | treats per-member pane routing as non-roster durable state | delete / rewrite | `Z.9` | `open` | move `tmux_pane_id` into canonical ATM roster-member state |
| `crates/atm-core/src/schema/agent_member.rs` | models retained Claude metadata as copied file state without explicit ATM ownership contract | tighten | `Z.9` | `open` | document and implement canonical ATM ownership for `tmux_pane_id` and any surviving Claude routing metadata |
| `crates/atm-core/src/team_admin/restore.rs` | reads current + backup `config.json` and replays backup-config roster state | delete / rewrite | `Z.10` | `open` | do not read backup `config.json` as roster truth; after Claude `TeamCreate`, project ATM roster truth into recreated `config.json`; preserve recreated `team-lead` / `leadSessionId` only |
| `crates/atm-core/src/config/mod.rs` | parser/serializer used broadly today | keep, allowlist | `Z.10` | `open` | keep one parser implementation, but only approved callers may reach it after follow-on cleanup closes |

## Non-Violation References Reviewed

The following files mention `config.json` but are not themselves roster-truth
violations when kept in their current narrow role:

| Path | Reason |
| --- | --- |
| `crates/atm-core/src/send/missing_config_notice.rs` | path/warning construction for missing team config; not a roster-truth source |
| `crates/atm-core/src/send/alert_state.rs` | path-based alert-state diagnostics; not a roster-truth source |
| `crates/atm-core/src/error_codes.rs` | error-code documentation |

## Post-Z.10 Boundary Violations

The following are real follow-on boundary violations even when they are not
Claude `config.json` roster-truth leaks:

| Path | Current Behavior | Violation Status | Owning Sprint | Current Status | Required Resolution |
| --- | --- | --- | --- | --- | --- |
| `crates/atm-core/src/delivery_policy.rs` and send-path error surfaces | returns opaque first-send empty-roster failure text | rewrite | `Z.11` | `planned` | replace the bad recovery contract with explicit `atm teams add-member` guidance and prove no hidden fallback was added |
| `crates/atm/src/commands/teams.rs` / `crates/atm/src/commands/members.rs` plus `crates/atm-core/src/team_admin.rs` | reaches `service_runtime_store::default_runtime()` through the wrong command-entry path for `teams`, `members`, and `add-member` | delete / rewrite | `Z.12` | `planned` | route roster access through the approved `RosterStore` seam only; `teams --json`, `members --json`, and `teams add-member` must stop failing on the uninstalled default-runtime path |
| `crates/atm-core/src/team_admin.rs` `list_teams` / `list_members` | ambient `.atm.toml` / `load_config(...)` current-team reads outside the approved seam | delete / rewrite | `Z.13` | `planned` | move current-team resolution behind the approved `ConfigIngress` / runtime seam and forbid new ambient command/team-admin reads |
| `crates/atm-core/src/lib.rs` | public crate-root re-export of `install_default_runtime_factory` leaks an ambient singleton/runtime-factory surface | delete / narrow | `Z.14` | `planned` | remove the broad re-export, keep only approved wrappers, and lint against new public singleton leaks |

## Static Gate Direction

Repository-local lint / `sc-lint`-candidate gates should be defined so the
follow-on implementation is mechanically checkable:

- reject production `config::load_team_config(...)` or equivalent direct team
  `config.json` roster reads outside the explicit allowlist
- reject production generic `load_team_config(...)` helper use from retained
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
