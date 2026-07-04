# Phase AD Readiness

## Goal

Record the accepted closure state for Phase AD:

- caller identity and caller team are resolved only from explicit CLI surfaces
  or invoking-shell environment when the command requires them
- `atm doctor` remains the identity-free diagnostic exception
- post-send behavior is reduced to explicit emitters with sender-visible
  warning fallback
- retained/dead compatibility surfaces are removed from the accepted line or
  explicitly marked obsolete

## Sprint Status

| Sprint | Status | Branch | Closure Gate |
| --- | --- | --- | --- |
| `AD.1` | `complete` | `feature/pAD-s1-caller-identity-ownership-restore` | shared caller-context resolution and required daemon request fields are explicit and validated at CLI entry |
| `AD.2` | `complete` | `feature/pAD-s2-config-identity-removal-and-doctor-repair` | obsolete config identity fallback is removed from accepted runtime ownership |
| `AD.3` | `complete` | `feature/pAD-s3-claude-backend-and-inbox-nudge-retirement` | retired Claude JSON mailbox append path is removed or marked obsolete-only |
| `AD.4` | `complete` | `feature/pAD-s4-reconcile-runtime-removal` | historical reconcile/watch runtime line is deleted from accepted behavior |
| `AD.5` | `complete` | `feature/pAD-s5-notification-runtime-removal-and-post-send-detachment` | post-send ownership is narrowed to explicit emitters and sender-visible warnings |
| `AD.6` | `complete` | `feature/pAD-s6-post-send-nudge-contract-simplification` | `PostSendHookEmitter` boundary is documented and enforced as the governing seam |
| `AD.7` | `complete` | `feature/pAD-s7-local-tmux-post-send-emitter` | local tmux nudges use authoritative roster pane metadata instead of repo-local pane truth |
| `AD.8` | `complete` | `feature/pAD-s8-graft-post-send-emitter` | graft-backed post-send emission is isolated behind the graft port and does not leak into core ownership |
| `AD.9` | `complete` | `feature/pAD-s9-update-member-cli-and-roster-repair-path` | `update-member` is the accepted roster-repair path and `.atm.toml` pane truth is obsolete |
| `AD.10` | `complete` | `feature/pAD-s10-directory-metadata-and-doctor-contract-cleanup` | durable `home_dir`, runtime `live_cwd`, and log-only `launch_cwd` terminology is enforced consistently |
| `AD.11` | `complete` | `feature/pAD-s11-smoke-and-readiness-closeout` | smoke artifacts, readiness gate, and promoted AD.9 findings close on one accepted evidence line |

## Evidence

| Sprint / Gate | Evidence | Meaning |
| --- | --- | --- |
| general smoke | `reports/smoke/smoke.md` | normal Phase AD smoke evidence for caller-context, doctor, and local post-send lanes |
| general smoke | `reports/smoke/smoke-thorough.md` | thorough Phase AD smoke evidence for cross-repo sender-home and graft emission lanes |
| `AD.6` governance | `boundaries/atm-core/post-send-hook-emitter.toml` | authoritative boundary record for `PostSendHookEmitter` |
| `AD.6` governance | `docs/atm-core/boundaries.md` | inventory entry for `PostSendHookEmitter` |
| `AD.8` governance | `boundaries/atm-core/graft-post-send-port.toml` | authoritative boundary record for `GraftPostSendPort` |
| `AD.8` governance | `docs/atm-core/boundaries.md` | inventory entry for `GraftPostSendPort` and its allowlisted daemon implementation |

## Phase Exit Criteria

Phase AD is not ready until all of the following are true:

- retained ATM commands that require caller identity fail locally when
  `ATM_IDENTITY` or an explicit override is unavailable
- retained ATM commands that require caller team fail locally when `ATM_TEAM`
  or an explicit override is unavailable
- commands with explicit caller-context override surfaces prefer those
  overrides over environment values
- `atm doctor` does not require caller identity or caller team and still
  honors optional `--team` scoping
- local tmux post-send emission uses authoritative roster pane metadata
- forced post-send emission failure becomes sender-visible warning output
- sender repository or sender `home_dir` differences do not change local
  post-send config lookup ownership
- graft-backed post-send behavior is mediated only by the graft port / emitter
  seam
- `GraftPostSendPort` governance exists in both the machine-readable boundary
  record and the boundary inventory
- no accepted Phase AD gate depends on daemon ambient identity, queued
  notification-runtime behavior, or historical Claude mailbox append logic

## Final Verdict

- candidate branch: `feature/pAD-s11-smoke-and-readiness-closeout`
- readiness gate: `PASS` when `just smoke normal`, `just smoke thorough`, and
  `python3 scripts/validate_release.py phase-ad-readiness` all pass on the
  same branch tip
- release verdict: `READY` on the branch candidate after the required AD.11
  validation commands pass
- notes: readiness remains fail-closed because the retained validation suite
  now checks the Phase AD readiness record, every AD sprint status, the
  `PostSendHookEmitter` / `GraftPostSendPort` boundary inventories, and both
  boundary TOML states
