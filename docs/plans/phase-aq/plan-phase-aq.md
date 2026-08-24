# Plan — Phase AQ: ATM Send-To Shell Integration

Status: redrafted — script-based transfer model (2026-08-23, per Rand) ·
Source PRD: [prd-atm-send-to.md](./prd-atm-send-to.md)
Reference code: `integrate/phase-ao2` (CLI at
`crates/atm/src/commands/{teams,send}.rs`, daemon maintenance-worker
precedent at `crates/atm-daemon/bin_support/daemon_observability.rs`).

## Scope

PRD Phase 1 only: one gesture from Finder/Explorer/Nautilus to a delivered
message whose text names files landed under the recipient host's `$ATM_TEMP`.
PRD Phase 2 (drafting agent, Wyvern chat sessions, structured `attachments`
metadata, `note_source`) is out of scope and planned after the Wyvern
chat-window integration exists.

## Binding decisions (from PRD, as redrafted per Rand 2026-08-23)

- **Transfer is an environment concern, not daemon machinery.** Cross-host
  bytes move via a specifically named, user-provided script per destination
  host (`~/.atm/transfer/<host>`), defaulting to sftp over the fleet's
  passwordless SSH, with Tailscale variants. Org-level SSH/Tailscale setup
  involves IT and cannot be planned around; the product ships modifiable
  examples and a setup doc. An unconfigured host fails the send closed with
  the canonical error: `File transfer to <host> not enabled. Read
  docs/cross-host-file-transfer.md to set up cross-host file transfer.` The
  daemon carries only ordinary messages — **no fetch/push endpoints, no
  transfer state machine, no envelope change, no new storage traits.**
- **`ATM_TEMP` is a system-level contract.** A mandatory environment
  variable naming the ATM scratch root for all features, validated at
  daemon/CLI startup. One TTL-only sweeper (30 days) covers everything under
  it; `<known-temp>/atm/` per-feature layouts are a non-issue.
- **R13 chaining invariant.** Every pipeline stage is side-effect-free
  except the final `atm send`; any staging/transfer failure aborts the whole
  invocation with zero sends and the reason on stderr.
- **No new protocol verb, no `MessageEnvelope` change in Phase 1.** Landed
  paths ride in message text via the AQ1 decision-(d) template.

## Sprints

| Sprint | Title | Depends |
|---|---|---|
| AQ1 | ATM_TEMP contract + transfer-script seam (ADR-054) | — |
| AQ2 | CLI surface: picker projection, `--attach`/`--from-json` fan-out, staging + transfer invocation | must_follow AQ1 |
| AQ3 | Transfer example scripts + setup doc | must_follow AQ2 · parallel_safe AQ4, AQ5 |
| AQ4 | ATM_TEMP sweeper (TTL-only, 30 d) | must_follow AQ1 · parallel_safe AQ2, AQ3, AQ5 |
| AQ5 | Wyvern picker + shell glue (macOS, Windows, Ubuntu) | must_follow AQ2 · parallel_safe AQ3, AQ4 |
| AQ6 | Validation evidence | must_follow all |
| AQ7 | `atm queue` verb: deferred-nudge send + pending-marker FIFO | must_follow AQ2 · parallel_safe AQ3, AQ4, AQ5 |
| AQ8 | Idle-drain for deferred nudges (heartbeat transition + recovery sweep) | must_follow AQ7 · parallel_safe AQ3, AQ5 |

Branch pattern: `feature/aq-N-<slug>` off `integrate/phase-aq`, PR target
`integrate/phase-aq`. Creating the `integrate/phase-aq` branch/worktree from
`develop` (carrying phase-ao2 merges) at phase start is a dispatch
precondition for AQ1, verified mechanically on the cut head:
`test -f docs/adr/ADR-047-*.md && test -f docs/adr/ADR-053-*.md`.

## Verified baseline facts (integrate/phase-ao2)

Verified against the reference tree; sprint docs cite these, reviewers should
not re-litigate them:

- `atm teams --json` emits `{name, member_count}` per team only. Per-member
  data lives in `atm members --json` (`MemberSummary`: `name`, `agent_id`,
  `harness`, `model`, `tmux_pane_id`, `home_dir`, `live_cwd`); runtime member
  state is `RuntimeMemberState` = `Unknown | IdentityConflict | Offline |
  Idle | Active`. The PRD §4.2 picker projection **does not exist and must
  be built** (AQ2), including the status mapping.
- No current roster or heartbeat record supplies a member host. AQ1 decision
  (e) makes host an explicit roster metadata binding via
  `teams add-member/update-member --host`; AQ2 owns the thin
  projection/resolution and touches no heartbeat or daemon runtime code.
- `atm send` is single-recipient (`to` positional, required) with existing
  flags `--team --host --chat-id --as --file --stdin --template --vars --var
  --tag --category --content-format --summary --requires-ack --task-id
  --dry-run --json`. No fan-out, no attach support. Sends go through the
  daemon HTTP transport only.
- Message delivery (local and cross-host) is the existing canonical path
  (ADR-035 ingress under ADR-047 peer-wire security) and is **unchanged by
  this phase** — Send-To adds no delivery semantics.
- `AtmConfig` (`crates/atm-core/src/config/types.rs`) has no temp/spool key;
  daemon directory conventions are `~/.atm/{daemon,db,logs}`
  (`crates/atm-core/src/home.rs`). `ATM_TEMP` is new (AQ1).
- Daemon background-task precedent: retained-log maintenance worker (60 s
  cadence, `crates/atm-daemon/bin_support/daemon_observability.rs`);
  observability is structured log events + health surface (e.g.
  `queue_full_drops_total`), no metrics registry.
- CI runs ubuntu + macOS + Windows lanes (`.github/workflows/ci.yml`).
  Python-driven shell-script test convention exists under `.just/tests/`
  (e.g. `test_release_gate.py`). ADRs live in `docs/adr/`; ADR-047 (created
  by phase-AO sprint AO.1) and ADR-053 exist on `integrate/phase-ao2`, so
  the AQ1 ADR is ADR-054.
- The fleet has passwordless SSH configured peer-to-peer from this machine
  to all destinations (Rand, 2026-08-23) — the sftp example script's
  baseline assumption.
- Nudges fire synchronously immediately post-persistence
  (`storage_and_nudge_router.rs:556-561` → `MessageReceivedHookSelector` →
  tmux send-keys or graft endpoint); **no deferral surface exists**. Member
  state arrives via `TeamMemberHeartbeatRequest`
  (`HeartbeatActivity: ActiveToolUse|Idle|SessionEnded`) into in-memory
  `RuntimeHealth` (polling-only; no transition events today).
  `mail_message_states` migrates via the `ensure_column` pattern
  (`shared_db.rs:888-935`). The queue channel is defined in **hermes-atm**
  (M5): Hermes exposes `/steer` (immediate) and `/queue` (deferred) as
  first-class input channels; atm-core's crates carry no queue surface on
  the reference tree, so AQ7 wires the atm-core side of that boundary (M5
  coordination for the exact contract).

## Open decisions routed to sprints

- `ATM_TEMP` strictness (fail-on-unset vs documented default), exact error
  texts, and the message path template → AQ1 ADR decisions (a)/(c)/(d).
- Remote-`ATM_TEMP` resolution approach inside transfer scripts → AQ3
  (script-owner's choice; examples show fixed-value and ssh-echo variants).
- Wyvern cold-start latency measured in AQ5 before the Shortcuts prototype
  is replaced; the Shortcuts/Out-GridView/zenity fallback remains shippable.

## Non-closure

- PRD Phase 2 (atm draft, chat sessions, "Open with agent", structured
  `attachments` envelope metadata, `note_source`).
- `atm spawn` shell entries (`atm queue` is now in scope: AQ7/AQ8; its
  dedicated shell entry, if any, is a follow-on).
- Durable heartbeat history / member-state subscription APIs beyond the
  internal transition sink (AQ8).
- Team-level addressing (client-side fan-out stands for this phase).
- Managed SSH/Tailscale enrollment (environment/IT concern; documented,
  not implemented).
- The prior pull-based transfer design (fetch endpoint, pending-delivery
  semantics, `AttachmentDeliveryStore`/`AttachmentSweepStore`, ADR-018 §3
  amendment): **rejected 2026-08-23 by Rand** as daemon state-machine
  complexity for a script-sized problem; retained only in git history.

## Plan-hardening QA history (2026-08-23)

Coordinator: fenix (plan edits by coordinator; reviews by background sonnet
agents per Rand's direction). Baseline verified against `integrate/phase-ao2`
by five explore agents before hardening began.

| Round | Step | Reviewer | reviewed_commit | status | blocking | important | minor | Note |
|---|---|---|---|---|---|---|---|---|
| 1 | 1 guidelines pass | Cipher-311d | 3db031a00 | PASS | – | – | – | contracts, paths-to-delete, --members |
| 1 | 2 plan-scope | plan-scope-reviewer | 3db031a00 | FAIL | 1 | 1 | 0 | host-sourcing gap; legacy-send compat gate |
| 1 | 3 fixes | Cipher-311d | 68f18d6af | PASS | – | – | – | decision (h) roster host binding |
| 1 | 4 critical | critical-plan-reviewer | 68f18d6af | FAIL | 1 | 3 | 1 | dup resolver; dedupe ADR; ADR-035 collision; R8 orphan |
| 2 | 5 fixes | fenix | 5d31d9823 | PASS | – | – | – | decisions (i), (f) rework, R8→AQ5 |
| 2 | 4 critical | critical-plan-reviewer | 5d31d9823 | FAIL | 0 | 1 | 0 | AQ3 stale vs decision (f) |
| 3 | 5 fixes | fenix | b865c00c4 | PASS | – | – | – | AQ3 implements (f) exactly |
| 3 | 4 critical | critical-plan-reviewer | b865c00c4 | PASS | 0 | 0 | 0 | closed on cycle 3/3 |
| 4 | 5 consistency | fenix | 6408ab8b0 | PASS | – | – | – | project-plan §48; ADR-054 in PRD |
| 5 | 6 plan-QA-1 | req-qa / arch-qa / ruthless-boundary-qa / rust-best-practices / rust-service-hardening | 6408ab8b0 | FAIL | 4 | 12 | 8 | 24 findings incl. ADR-047 supersession, ADR-018 §3 cap, fetch-endpoint hardening |
| 5 | fixes | fenix | 998009075 | PASS | – | – | – | all 24 folded in |
| 6 | plan-QA-2 | same five reviewers | 998009075 | FAIL | 1 | 2 | 2 | SweepStore signature; dispatch gate; http-runtime.toml collision; PRD wording |
| 6 | fixes | fenix | 931bc6294 / efc398dba | PASS | – | – | – | – |
| 7 | plan-QA-3 | req-qa; ruthless-boundary-qa | 931bc6294 / efc398dba | FAIL | 0 | 1 | 0 | RBQA-F007 sync-trait execution contract |
| 7 | fixes | fenix | 88b318837 | PASS | – | – | – | recorded sync exception + spawn_blocking rail |
| 8 | final | req-qa PASS (deliverables 16/16, 100%) · arch-qa PASS (merge-ready) · ruthless-boundary-qa PASS · rust-best-practices PASS · rust-service-hardening PASS | 88b318837 | **PASS** | 0 | 0 | 0 | quality-mgr gate: PASS (pull-based design) |
| 9 | redesign | Rand + fenix | (this commit) | REDRAFT | – | – | – | Rand rejected pull-based transfer as daemon state-machine over-engineering; plan redrafted to script-based transfer + system-level ATM_TEMP; re-review required |
