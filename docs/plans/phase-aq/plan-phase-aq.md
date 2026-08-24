# Plan — Phase AQ: ATM Send-To Shell Integration

Status: hardened — plan-QA PASS (2026-08-24, queue-first 6-sprint structure) ·
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

Queue ships first (per Rand, 2026-08-23): it is self-contained, unblocks
Hermes/graft consumers, and the Send-To CLI work then lands on the
kind-aware send surface.

Graft connection-model insertion (per Rand, 2026-08-24, sprints
AQ1.5–AQ1.9): the file-based receiver endpoint record is replaced with
push-registration to the daemon (SQLite-backed runtime as single source of
truth) before AQ2 puts the queue's graft channel on that foundation. Binding
lifecycle requirement: daemon and graft receivers restart independently and
delivery recovers with zero manual steps (no Hermes profile reset). See
sprint-AQ1-5 for the requirement text and ADR-056. Sub-numbered ids follow
the AO2.5.x precedent to avoid renumbering the hardened AQ2–AQ6 docs.

| Sprint | Title | Depends |
|---|---|---|
| AQ1 | `atm queue`: CLI verb, taxonomy (ADR-054), kind-aware dispatch + renames, `PendingNudgeStore` | — |
| AQ1.5 | Graft registration: daemon API + durable SQLite store (ADR-056) | must_follow AQ1 |
| AQ1.6 | Graft registration: receiver announce-at-init + lease refresh (dual-write) | must_follow AQ1.5 |
| AQ1.7 | Graft endpoint consumer cutover (delivery, `_internal-nudge`, doctor) | must_follow AQ1.6 |
| AQ1.8 | Graft file-record retirement + AI3133 closure | must_follow AQ1.7 · parallel_safe AQ1.9 |
| AQ1.9 | hermes-atm wheel bump + live restart-matrix verification on m5 | must_follow AQ1.7 · parallel_safe AQ1.8 |
| AQ2 | Queue: atm-graft dual-channel | must_follow AQ1, AQ1.7 · parallel_safe AQ3 (AQ2 owns the graft channel's send-and-report; AQ3 owns kind-agnostic claim/dispatch scheduling; retry state lives only in AQ1's store — neither calls the other's code) |
| AQ3 | Queue: tmux idle-drain | must_follow AQ1 · parallel_safe AQ2 |
| AQ4 | Send-To core: ATM_TEMP (ADR-055), CLI surface, transfer scripts, sweeper | must_follow AQ1–AQ3 |
| AQ5 | Send-To surface + phase evidence | must_follow AQ4 |
| AQ6 | SC-ecosystem dependency preflight (pin-latest + integration tests) + Wyvern contract issue | must_follow AQ5 |

The table is an ownership map, not a second requirements list; each sprint
doc's own Dependencies section is authoritative on any mismatch, and no
later sprint may redefine an earlier contract.

ADR numbering: ADR-054 = nudge taxonomy + queue mechanism (AQ1); ADR-055 =
ATM_TEMP + transfer seam (AQ4); ADR-056 = graft receiver registration and
lease semantics (AQ1.5). ADR-047 (phase-AO AO.1) and ADR-053 are on
`integrate/phase-ao2`, which merges to `develop` before `integrate/phase-aq`
is cut — verified mechanically on the cut head:
`test -f docs/adr/ADR-047-*.md && test -f docs/adr/ADR-053-*.md`.

Branch pattern: `feature/aq-N-<slug>` off `integrate/phase-aq`, PR target
`integrate/phase-aq`.

## Non-closure

- PRD Phase 2 (atm draft, chat sessions, "Open with agent", structured
  `attachments` envelope metadata, `note_source`).
- `atm spawn` shell entries (`atm queue` is in scope: AQ1-AQ3; its
  dedicated shell entry, if any, is a follow-on).
- Durable heartbeat history / member-state subscription APIs beyond the
  internal transition sink (AQ3).
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
| 9 | redesign | Rand + fenix | 6bfb9e609 | REDRAFT | – | – | – | Rand rejected pull-based transfer as daemon state-machine over-engineering; plan redrafted to script-based transfer + system-level ATM_TEMP; re-review required |
| 10 | restructure | Rand + fenix | 7c76603c6 | REDRAFT | – | – | – | queue-first 5-sprint structure (AQ1 queue-cli, AQ2 graft, AQ3 tmux, AQ4 send-to core, AQ5 surface+evidence); all prior QA-hardened ACs preserved inside the consolidated docs; nudge/steer/queue taxonomy + repo-doc sweep folded in; re-review required |
| 11 | plan-QA (restructure) | req-qa / arch-qa / ruthless-boundary-qa / rust-best-practices / rust-service-hardening | 0092db33e | FAIL | 0 | 6 | 1 | priors all verified closed; new: project-plan ADR self-contradiction, RSH-016 unbounded graft retry, RBP-F008 RosterStore undeclared, RBQA-F012 graft-retry interface vs parallel_safe, ATM-QA-011/012 |
| 11 | critical (restructure) | critical-plan-reviewer | 0092db33e | FAIL | 1 | 2 | 0 | PLAN-CRIT-014 ADR numbering; 015 AQ4 split-risk; 016 AQ1 governance-gate bundling |
| 11 | fixes | fenix | 5d66b7a2c…054600099 | PASS | – | – | – | incl. Wyvern dependency contract (Rand's gap: optional runtime dep, schema_version gate, CLI version floor, 6 degradation cases); retry state given one owner (`claim_next_pending`/`requeue_pending` + `nudge_attempts`), AQ3 sweep made kind-agnostic; AQ4 consolidation justified with staged landing order |
| 12 | verify | ruthless-boundary-qa | 054600099 → c825fdd55 | PASS | 0 | 0 | 0 | RBQA-F012 closed structurally; RBQA-F013 (unbounded `wyvern --version` probe) raised and fixed — zero open |
| 12 | verify | critical-plan-reviewer | 054600099 | FAIL | 0 | 1 | 0 | 014/016 closed; **015 mitigation accepted — AQ4 split NOT required**; PLAN-CRIT-017 PRD schema not updated for `schema_version` |
| 13 | fixes + verify | fenix; critical-plan-reviewer | 0a2730064 | **PASS** | 0 | 0 | 1 | PRD §4.2/§5a carry `schema_version` with fall-back semantics; AQ4 stdin contract aligned; PLAN-CRIT-M3 wording fixed |
| 14 | Wyvern policy | Rand + fenix | 6fa5c5a7b | UPDATE | – | – | – | pin-latest Wyvern policy (bump every atm release, no version ranges); new AQ6: sc-ecosystem preflight rules (sc-compose/sc-observability/wyvern bump-to-latest + integration tests, fix-forward) + Wyvern-repo contract-test GH issue |
| 15 | final verify | req-qa · critical-plan-reviewer · ruthless-boundary-qa | e97505ac1 / ea43d2d0b | **PASS** | 0 | 0 | 0 | ATM-QA-013/PLAN-CRIT-018 (phase close → AQ6 AC5) and PLAN-CRIT-019 (dependency-currency extension + per-dep integration targets) closed; req-qa 18/18 deliverables (100%); critical reviewer: zero Blocking/Important across all six sprints, PRD, and plan — full arc closed |

## Insertion QA history (AQ1.5–AQ1.9)

| Round | Reviewer | Commit | Result | Disposition |
|-------|----------|--------|--------|-------------|
| 1 | plan-scope-reviewer (sonnet) | `03e28b8cc` | FAIL — 3 Important (no `GraftEndpointStoreError` enum; no `GraftReceiverLease` struct; bare "AI3133" ambiguous across 13+ same-prefix findings), 2 minor | Fixed in round-1 fix commit: error enum with per-variant caller obligations; full lease struct; exact finding id `AI3133-HERMES-GRAFT-RECEIVER-SINGLETON-UNSAFE` everywhere; envelope field sketches; Required-validation sections matched to sibling docs. |
| 1 | critical-plan-reviewer (sonnet) | `03e28b8cc` | FAIL — 3 Blocking (`mark_unreachable` had no backing column; `sealed::Sealed` bound dropped from the trait sample; refresh interval undefined + real idle-gated timer starvation could displace a live busy receiver), 3 Important (loopback validation unowned; undisclosed post-AQ1.8 cold-start delivery window; missing version-skew posture), 1 minor | Fixed in round-1 fix commit: `unreachable_at` column with refresh/register clearing rule; sealed bound + impl deliverable; `GRAFT_LEASE_REFRESH_INTERVAL`=1s / `ACTIVE_LEASE_WINDOW`=15s constants with every-iteration elapsed-checked refresh and a sustained-load AC; handler rejects non-loopback endpoints (+AC); cold-start window disclosed as accepted residual risk in AQ1.8/ADR-056; matched-release-pair version-skew statement in AQ1.7; AQ2 "published endpoint" glossed. |
| 2 | both reviewers (sonnet) | `c6ff611db` | **PASS** — all round-1 closures verified (critical reviewer additionally confirmed against real code that delivery is daemon-mediated, validating the cold-start disclosure); zero findings | Post-PASS addition (Rand): read-side consumers / liveness-derivation guardrails in AQ1.5 (natural-key join, no FK, derived-not-stored aliveness, two-writers rule) — narrowly re-verified in round 3. |
| 3 | critical-plan-reviewer (sonnet) | `d83cc8f3a` | FAIL — 1 Important (liveness block falsely claimed the schema has no FK precedent; `mail_message_states`→`mail_messages` FK exists), 1 minor (displacement vs read-time predicates not cross-referenced) | Fixed in round-3 commit: rationale rewritten acknowledging the FK precedent and distinguishing it (1:1 lifecycle vs save_roster's delete+reinsert pattern, verified at roster_store.rs); intentional two-predicate note added. |
