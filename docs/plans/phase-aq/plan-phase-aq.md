# Plan — Phase AQ: ATM Send-To Shell Integration

Status: **re-hardened — whole-tree critical review PASS (round 3, 2026-08-26, `ea990a8dd`); quality-mgr gate pending.** The original critical review (fenix, 2026-08-26: 10 blocking / 22 important / 17 minor) and its closure are recorded below. The prior "hardened — plan-QA PASS (2026-08-24, queue-first 6-sprint structure)" header is **retracted**: the AQ2.6/AQ2.7 insertion (2026-08-25, `47a26c90f`…`e2d886c91`) was never reviewed and rewrote already-PASSed docs (AQ1, AQ2.5, AQ3, AQ4, AQ5), so those PASS rows no longer describe the text on disk (see "Critical review 2026-08-26" and "Insertion QA history (AQ2.6–AQ2.7)" below). Reordered 2026-08-26 per Rand: trait foundation first, Herdr second. ·
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
- **`ATM_TEMP` is a system-level contract.** One environment variable names
  the ATM scratch root for all features. **Resolved 2026-08-26 (critical
  review B10, ADR-055 / AQ4 decision (a)):** it is *not* mandatory — when
  unset, daemon and CLI fall back to a per-user `<temp_dir()>/atm-<uid>`
  (Windows `<temp_dir()>\atm`) created `0700`, with exactly one startup
  warning; when set-but-invalid, or when the resolved directory is owned by
  another uid or has group/world bits (`AtmTempInsecure`), resolution fails
  closed. Validation runs at daemon boot and at the CLI's first scratch use.
  One TTL-only sweeper (30 days) covers everything under it;
  `<known-temp>/atm/` per-feature layouts are a non-issue.
- **R13 chaining invariant.** Every pipeline stage is side-effect-free
  except the final `atm send`; any staging/transfer failure aborts the whole
  invocation with zero sends and the reason on stderr.
- **No new protocol verb, no `MessageEnvelope` change in Phase 1.** Landed
  paths ride in message text via the AQ4 decision-(d) template.

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

Delivery-trigger insertion (per Rand, 2026-08-24, sprint AQ2.5): the plan
wired queue through the CLI/daemon but never answered *when* a queued
message is delivered — no in-tree client produces the
`TeamMemberHeartbeatRequest` idle signal AQ3's drain consumes, and members
without a tmux `pane_id` in the roster had no injection path at all. AQ2.5
adds the hook-driven heartbeat producer (one hook shape for Claude and
Codex, with or without tmux), the single-code-owner `DeliveryChannel`
classifier (tmux steer / graft / bare-CLI — mechanism-positive names per
Rand's naming rule; future harness channels extend it), and the bare-CLI
path: a RAM-only bounded FIFO drained by a simple Stop-pull get
(simplicity mandate: staleness beats loss, no claim/requeue machinery).
The trigger-policy matrix is normative in sprint-AQ2-5 and lands as an
ADR-054 addendum; AQ3's sweep pre-check consumes the classifier seam.

Herdr alternate-backend insertion (per Rand, 2026-08-25, sprints AQ2.6–AQ2.7),
**re-positioned 2026-08-26 per Rand as the phase's most urgent deliverable**:
the existing Tokio tmux received-message emitter is retained. AQ2.6 adds the
Herdr implementation of the local message-received backend behind the sealed
`AsyncMessageReceivedHookEmitter` boundary as the **first implementer** of
the trait foundation AQ1 lands; it does not remove, emulate, or fall back to
tmux. AQ2.7 supplies the Herdr-aware deferred-wake pump for `atm queue`, with
its own Herdr-only claim guard (it no longer waits on AQ3). Herdr has no
server-side queue: ATM's durable mailbox and AQ1 pending marker remain the
queue, and every Herdr invocation is only a wake-up prompt directing the agent
to `atm read`. The wrapper's wait→prompt sequence has an acknowledged race and
must never be represented as an atomic "deliver when idle" primitive.
**Dispatch precondition (critical-review B9)**: ADR-058 (PR #1039) — the
Herdr 0.8.2 contract derived from source. Key decision (Rand, 2026-08-26):
the daemon never launches Herdr sessions — the external team launcher
does, exactly as with tmux — so the session an agent lives in is per-member
roster data (`LocalMessageReceivedBackend::Herdr { session:
Option<HerdrSession> }`, set at `add-member --backend herdr [--session]`),
and the emitter sets `HERDR_SESSION` on the child env per invocation; the
daemon's own environment is never consulted.

Trait-foundation-first reorder (per Rand, 2026-08-26; closes critical-review
B2/B5/B6/B7 by construction instead of re-litigating them across three
insertions): AQ1 becomes the trait-change sprint that lands every contract
the queue, Herdr, and graft sprints build on — `PendingNudgeStore` (incl.
`release_pending`, `clear_pending_on_handoff`, and the previously missing
`list_pending_members` + dispatch-from-message-id), the canonical `MemberKey`
resolved to **one** crate (no atm-core↔atm-storage cycle),
`LocalMessageReceivedBackend` + the `DeliveryChannel` classifier seam owned
**once** (AQ2.5 and AQ2.6 both consume it; neither defines it), and the
sealed `AsyncMessageReceivedHookEmitter` extension point with the boundary
manifest brought current. Herdr (AQ2.6/AQ2.7) then lands immediately as the
first implementer; graft registration (AQ1.5–AQ1.9) runs parallel_safe with
the Herdr pair (disjoint files); AQ2/AQ2.5/AQ3 follow; AQ4–AQ6 unchanged.

| Sprint | Title | Depends |
|---|---|---|
| AQ1 | **Trait foundation** + `atm queue`: CLI verb, taxonomy (ADR-054), kind-aware dispatch + renames, `PendingNudgeStore` (+`release_pending`, `clear_pending_on_handoff`, `list_pending_members`, dispatch-from-message-id), canonical `MemberKey` in one crate, `LocalMessageReceivedBackend` + `DeliveryChannel` classifier seam, sealed emitter extension point + manifest | — |
| AQ2.6 | Local steer backends: retained tmux + Herdr message-received emitter (first implementer of AQ1's seam) | must_follow AQ1 · **precondition: Herdr contract in-repo (lane A)** · parallel_safe AQ1.5–AQ1.9 |
| AQ2.7 | Queue: Herdr lifecycle-gated mailbox wake-up (own Herdr-only claim guard) | must_follow AQ1, AQ2.6 · parallel_safe AQ1.5–AQ1.9 |
| AQ1.5 | Graft registration: daemon API + durable SQLite store (ADR-056) | must_follow AQ1 · parallel_safe AQ2.6, AQ2.7 |
| AQ1.6 | Graft registration: receiver announce-at-init + lease refresh (dual-write) | must_follow AQ1.5 · parallel_safe AQ2.6, AQ2.7 |
| AQ1.7 | Graft endpoint consumer cutover (delivery, `_internal-nudge`, doctor) | must_follow AQ1.6 · parallel_safe AQ2.6, AQ2.7 |
| AQ1.8 | Graft file-record retirement + AI3133 closure | must_follow AQ1.7 · parallel_safe AQ1.9, AQ2.6, AQ2.7 |
| AQ1.9 | hermes-atm wheel bump + live restart-matrix verification on m5 | must_follow AQ1.7 · parallel_safe AQ1.8, AQ2.6, AQ2.7 |
| AQ2 | Queue: atm-graft dual-channel | must_follow AQ1, AQ1.7 (AQ2 owns the graft channel's send-and-report; retry state lives only in AQ1's store) |
| AQ2.5 | Queue delivery triggers: heartbeat producer (harness idle hooks), bare-CLI arm of AQ1's classifier, RAM FIFO + Stop-pull get (ADR-054 addendum) | must_follow AQ1, AQ1.7, AQ2 (shared `received_hook_selector.rs` — AQ2 lands first), AQ2.6 (Herdr arm already present in the seam) |
| AQ3 | Queue: tmux idle-drain + kind-agnostic recovery sweep (skip-Herdr pre-check on **both** drain and sweep) | must_follow AQ1, AQ2.5, AQ2.6 — transitively after AQ2; **no parallel_safe claim** (the former AQ2↔AQ3 claim was dead text, critical-review I1) |
| AQ4 | Send-To core: ATM_TEMP (ADR-055), CLI surface, transfer scripts, sweeper | must_follow AQ1–AQ3 (the former AQ2.6–AQ2.7 edge was unjustified — M12; AQ5's Herdr evidence still follows AQ2.7; lane C groundwork may start earlier — see Parallel lanes) |
| AQ5 | Send-To surface + phase evidence | must_follow AQ4; Herdr/tmux evidence consumes AQ2.6–AQ2.7 |
| AQ6 | SC-ecosystem dependency preflight (pin-latest + integration tests) + Wyvern contract issue | must_follow AQ5 |

Landing order (serial spine, with the only genuine parallelism marked):
AQ1 → AQ2.6 → AQ2.7 ∥ (AQ1.5 → AQ1.6 → AQ1.7 → {AQ1.8 ∥ AQ1.9}) → AQ2 →
AQ2.5 → AQ3 → AQ4 → AQ5 → AQ6. Fourteen sprints.

The table is an ownership map, not a second requirements list; each sprint
doc's own Dependencies section is authoritative on any mismatch, and no
later sprint may redefine an earlier contract.

ADR numbering: ADR-054 = nudge taxonomy + queue mechanism (AQ1); ADR-055 =
ATM_TEMP + transfer seam (AQ4); ADR-056 = graft receiver registration and
lease semantics (AQ1.5); ADR-058 = Herdr local steer backend contract (lane
A, PR #1039 → `integrate/phase-aq`; ADR-057 is already taken on `develop`). ADR-047 (phase-AO AO.1) and ADR-053 are on
`integrate/phase-ao2`, which merges to `develop` before `integrate/phase-aq`
is cut — verified mechanically on the cut head:
`test -f docs/adr/ADR-047-*.md && test -f docs/adr/ADR-053-*.md`.

Branch pattern: `feature/aq-N-<slug>` off `integrate/phase-aq`, PR target
`integrate/phase-aq`.

## Parallel lanes (per Rand, 2026-08-26)

Work that can be dispatched now, alongside the AQ1 trait-change sprint,
because it touches none of AQ1's files. Each lane is a separate dispatch to
arch-ctm; lane B is this plan-repair pass.

| Lane | Work | Why independent | Unblocks |
|---|---|---|---|
| A | **Herdr contract** — ADR + pinned Herdr version, CLI argv incl. workspace/team selection, stderr codes `agent_blocked`/`agent_not_found`, fixture transcript committed in-repo | Docs/fixtures only | AQ2.6 dispatch (B9) |
| B | **Plan repair** — this pass: header retraction, AQ2.6/2.7 QA table, reorder, decision-letter refs, §48 sprint count, phase-close predicate, dead parallel_safe | Doc-only | re-entry into scope/critical/quality-mgr hardening on the whole tree |
| C | **Send-To groundwork (AQ4 minus `send.rs`)** — ADR-055, `ATM_TEMP` rollout/compat story (B10), transfer-script contract (exec-bit/ownership check, `.ps1` via `pwsh -File`, `<host>`-as-filename guard), `docs/cross-host-file-transfer.md`, sweeper crate placement | No overlap with queue/emitter/store files; only `--attach` in `send.rs` waits on AQ1's `NudgeMode` seam | AQ4 |
| D | **Graft registration prep (AQ1.5)** after fixing B3/B4 — `GraftReceiverEndpointStore` in the correct crate, schema, register/unregister routes; registration client placed in `atm-graft` (never atm-core) | Disjoint from `PendingNudgeStore`; `protocol.rs`/router overlap is merge-forward sequencing only | AQ1.6–AQ1.9 |

Fillers: bring `boundaries/atm-core/message-received-hook-emitter.toml`
current (I13); AQ6 dependency-currency script hardening. **Cannot**
parallelize: AQ2.6/AQ2.7 implementation, AQ2/AQ2.5/AQ3, `--attach` in
`send.rs`.

## Critical review 2026-08-26 (fenix, whole-tree, @ `e2d886c91`)

Verdict **FAIL** — 10 blocking / 22 important / 17 minor; full text in ATM
message `01M1000DCNKE5837QH39HYHEFJ` + addenda `01M1008SRS7MENQ4SKZ8XYEZ22`,
`01M1009RNXWV1M869KANVH2ED0`, `01M100BMKBBC213EK6Y2YHMY6B`. Blockers and
disposition:

| id | Finding | Disposition (this pass = plan-doc only) |
|---|---|---|
| B1 | False closure: PASS header vs unreviewed AQ2.6/2.7 + rewritten PASSed docs | **closed here** — header retracted, stale rows marked, AQ2.6/2.7 table opened |
| B2 | `PendingNudgeStore` "owned by atm-storage" keyed on atm-core `MemberKey` = crate cycle | **structurally closed** — AQ1 trait-foundation scope requires one-crate resolution (AQ1 AC 8); crate decision is AQ1's ADR-054 deliverable |
| B3 | AQ1.6 registration inside atm-core `graft.rs` needs atm-http-runtime/atm-daemon-client (cycle) | **plan-level closed (finalization 2026-08-26)** — registration client, refresh, and unregister-on-drop moved to atm-graft (`RegisteredGraftReceiver`); atm-core exposes lease inputs only |
| B4 | AQ1.6/AQ1.7 grep gate false against real code (`graft_receiver_ownership.rs`, `runtime.rs:892`, `internal_nudge.rs`) | **plan-level closed** — `bind(graft_root, team, agent, owner_chat_id)` signature change, `endpoint_path`→`graft_root`, all five real call sites inventoried in AQ1.6; AQ1.7 AC 2 rewritten |
| B5 | AQ2.5↔AQ2.6 circular ownership of `LocalMessageReceivedBackend`/`HerdrSteer` | **structurally closed** — AQ1 owns the enum + classifier; AQ2.5/AQ2.6 consume |
| B6 | No dispatch-from-message-id path for drain/sweep/pump | **structurally closed** — AQ1 trait-foundation deliverable; contract text still to be authored in AQ1 |
| B7 | No `list_pending_members` — sweep/pump cannot enumerate | **structurally closed** — added to AQ1's `PendingNudgeStore` contract |
| B8 | AQ3 idle drain (not just sweep) can claim Herdr messages | **plan-level closed** — AQ2.7 owns its own Herdr-only guard; AQ3 pre-check applies to both drain and sweep (AQ3 doc updated); AC text still to be hardened |
| B9 | Herdr has no contract anywhere in repo | **closed at doc level** — ADR-058 + fixture authored on PR #1039 (open, doc-only, targets integrate/phase-aq); AQ2.6/AQ2.7 rewritten against it; **PR #1039 merge is AQ2.6's dispatch precondition** |
| B10 | `ATM_TEMP` eager daemon boot-fail is fleet-breaking with no rollout story | **plan-level closed** — unset falls back to `<temp_dir>/atm` with a startup warning; set-but-invalid fails closed; PRD §4.5 aligned |

Finalization pass (2026-08-26, five Sonnet doc agents + coordinator): every
Important/minor finding I1–I22, M1–M17 has been addressed in the sprint docs
(I2/I5/I6/I7/I13/I14/I15/M8/M10 in AQ2/AQ2.5/AQ3; I8–I12/M5–M7 in
AQ1.5–AQ1.9; I16–I21/M11 in AQ2.6/AQ2.7; I22/M12–M14 in AQ4–AQ6/PRD;
I3/I4/M1/M2 in AQ1; M9 in this doc's AQ2.5 history table). AQ1 is being
implemented concurrently against `aq1-blueprint.md`; the sprint doc was
aligned to the blueprint (D1 crate placement, `mark_pending`,
`GraftLeaseState`, `HerdrSession`, deferred renames). Re-entry rule: scope +
critical + quality-mgr run on the **whole tree**, not per-insertion.

Decisions surfaced by the finalization agents — **all eight approved by Rand
2026-08-26** (decision 1 explicitly discussed; each sprint doc now carries an inline
"approved by Rand 2026-08-26" marker at the decision — critical review F3): (1) AQ1.5
`register` displaces unconditionally on generation mismatch (flock proves
same-host exclusivity) — revises a 4-round-hardened contract element; (2)
AQ1.7 dials present-but-expired leases rather than refusing; (3) AQ2.6
mixed-backend doctor finding is Warning, not Error; (4) AQ2.6
`normalize_tmux_pane_id` non-`%N` targets accepted-with-warning; (5) AQ2.7
`HerdrQueueWakePumpConfig` defaults `max_concurrent_waits = 8`,
`target_recheck_interval = 10 min`; (6) AQ4 lands as one PR with tagged
sequential commits and a single QA gate (not AQ4a/AQ4b); (7) AQ4
`AtmConfig.local_host` as the sender's own host identity; (8) AQ6 Wyvern
issue target repo `randlee/wyvern`.

## Re-entry critical review (whole tree, 2026-08-26)

| Round | Reviewer | Commit | Verdict | Notes |
|---|---|---|---|---|
| 1 | critical-plan-reviewer (sonnet, whole tree) | `9827dfb1e` | FAIL — 3 Blocking (F1 AQ2.6 `Herdr.session` typed `Option<String>` vs AQ1's `HerdrSession`; F2 AQ2.5 classifier signature still `Option<&GraftReceiverLease>`; F3 "Rand to confirm" markers absent inline in 6/8 docs), 4 Important (F4 AQ3 attributed the classifier to AQ2.5; F5 no owner for the lease→`GraftLeaseState` mapping; F6 B9 "closed" while PR #1039 unmerged; F7 fallback `ATM_TEMP` had no shared-host permission story), 1 minor | Fixed `ab74477b6`/`cb0747be7`: types aligned to landed code; inline approval markers; AQ1.7 owns `graft_lease_state`; B9 reworded with PR #1039 as AQ2.6 precondition; per-uid `0700` fallback dir with `AtmTempInsecure`. |
| 2 | same | `cb0747be7` | FAIL — 0 Blocking, 2 Important (AQ1.7 AC 8 referenced but missing; AQ4 `AtmTempInsecure` not in error table/AC 1), 1 minor (AQ3 pronoun) | Fixed `ea990a8dd`. |
| 3 | same | `ea990a8dd` | **PASS** — zero Blocking/Important; no regressions | Ready for quality-mgr gate. |

## Insertion QA history (AQ2.6–AQ2.7)

| Round | Reviewer(s) | Commit | Verdict | Notes |
|---|---|---|---|---|
| 0 | — (initial draft, fenix per Rand) | `47a26c90f`…`e2d886c91` | DRAFT — **never reviewed** | Seven authored commits 2026-08-25; also modified AQ1 (+`release_pending`), AQ2.5 (+31/+41/+7), AQ3 (+23 skip-Herdr), AQ4 (+7), AQ5 (+8) after their PASS rows. |
| 1 | critical-plan-reviewer (fenix, whole-tree) | `e2d886c91` | FAIL | B5 circular ownership; B8 drain double-claim; B9 no Herdr contract; I16 argv lacks workspace; I17 pump concurrency/head-of-line; I18 "process adapter" undelivered + wrong crate direction; I19 two mapping owners; I20 mixed-backend doctor Error unjustified; I21 `recipient_pane_id` blast radius; M11 normalize flip. |
| 1 | plan repair (fenix, this pass) | this commit | — | Reordered to trait-foundation-first; AQ2.6 `must_follow AQ1` only + lane-A precondition; AQ2.7 drops AQ2.5/AQ3 deps and owns its guard; AQ1 owns enum/classifier. B5/B8 closed at plan level; B9 + I16–I21 + M11 open for the AQ2.6/AQ2.7 rewrite round. |
| 2 | finalization rewrite (sonnet agent B + fenix) | this commit | — | Both docs rewritten against ADR-058 + AQ1 blueprint: per-member `HerdrSession`, exact argv/env, full error-code table, `HerdrProcessAdapter`/`HerdrProcessInvoker` in atm-http-runtime (reachable by bootstrap; atm-core stays tokio-free), single classifier owner + named audit gate, doctor Warning for mixed backends, pane-id warning change explicit, `herdr agent get` presence probe; AQ2.7 pump: per-member Tokio task + bounded semaphore, blocked=exit 0 parsed, always `--timeout`, dispatch via `rebuild_received_hook_dispatch(.., Queue)`. B9/I16–I21/M11 closed at plan level; ready for whole-tree critical review. |

## Non-closure

- PRD Phase 2 (atm draft, chat sessions, "Open with agent", structured
  `attachments` envelope metadata, `note_source`).
- `atm spawn` shell entries (`atm queue` is in scope: AQ1-AQ3 and AQ2.7; its
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

> **Stale (2026-08-26):** every PASS row below predates `47a26c90f` (Herdr
> insertion), which rewrote AQ1, AQ2.5, AQ3, AQ4 and AQ5 — those rows no
> longer describe the text on disk. Retained as history only.

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

> **Open (2026-08-26):** ends on a round-5 quality-mgr FAIL with fixes but no
> re-gate PASS; critical review B3/B4 found the round-5 fix itself wrong.

| Round | Reviewer | Commit | Result | Disposition |
|-------|----------|--------|--------|-------------|
| 1 | plan-scope-reviewer (sonnet) | `03e28b8cc` | FAIL — 3 Important (no `GraftEndpointStoreError` enum; no `GraftReceiverLease` struct; bare "AI3133" ambiguous across 13+ same-prefix findings), 2 minor | Fixed in round-1 fix commit: error enum with per-variant caller obligations; full lease struct; exact finding id `AI3133-HERMES-GRAFT-RECEIVER-SINGLETON-UNSAFE` everywhere; envelope field sketches; Required-validation sections matched to sibling docs. |
| 1 | critical-plan-reviewer (sonnet) | `03e28b8cc` | FAIL — 3 Blocking (`mark_unreachable` had no backing column; `sealed::Sealed` bound dropped from the trait sample; refresh interval undefined + real idle-gated timer starvation could displace a live busy receiver), 3 Important (loopback validation unowned; undisclosed post-AQ1.8 cold-start delivery window; missing version-skew posture), 1 minor | Fixed in round-1 fix commit: `unreachable_at` column with refresh/register clearing rule; sealed bound + impl deliverable; `GRAFT_LEASE_REFRESH_INTERVAL`=1s / `ACTIVE_LEASE_WINDOW`=15s constants with every-iteration elapsed-checked refresh and a sustained-load AC; handler rejects non-loopback endpoints (+AC); cold-start window disclosed as accepted residual risk in AQ1.8/ADR-056; matched-release-pair version-skew statement in AQ1.7; AQ2 "published endpoint" glossed. |
| 2 | both reviewers (sonnet) | `c6ff611db` | **PASS** — all round-1 closures verified (critical reviewer additionally confirmed against real code that delivery is daemon-mediated, validating the cold-start disclosure); zero findings | Post-PASS addition (Rand): read-side consumers / liveness-derivation guardrails in AQ1.5 (natural-key join, no FK, derived-not-stored aliveness, two-writers rule) — narrowly re-verified in round 3. |
| 3 | critical-plan-reviewer (sonnet) | `d83cc8f3a` | FAIL — 1 Important (liveness block falsely claimed the schema has no FK precedent; `mail_message_states`→`mail_messages` FK exists), 1 minor (displacement vs read-time predicates not cross-referenced) | Fixed in round-3 commit: rationale rewritten acknowledging the FK precedent and distinguishing it (1:1 lifecycle vs save_roster's delete+reinsert pattern, verified at roster_store.rs); intentional two-predicate note added. |
| 4 | critical-plan-reviewer (sonnet) | `9dee3e807` | **PASS** — FK-rationale correction and two-predicates note verified; zero findings; insertion hardening complete | Ready for quality-mgr gate. |
| 5 | quality-mgr gate (PR #1011 insertion) | `4052d35b2` | FAIL — 1 Blocking (AQ1.8 deleted `graft_receiver_record_path_*` while atm-graft/src/lib.rs:390/:784 still call it for the retained flock's lock path — all 4 hardening rounds missed it by never grepping that file), 1 Important (AQ1.7 AC#2 grep-gate exemption clause false as written), 1 minor (finding-prefix count 13+ vs actual 11) | Fixed in round-5 commit: AQ1.6 deliverable 5 introduces `graft_receiver_lock_path_from_root` (independent derivation, same on-disk `.lock` location) and migrates both lib.rs call sites pre-deletion; AQ1.7 AC#2 rewritten to reflect the migration with a whole-workspace grep; AQ1.8 test-migration scope + grep gate extended to atm-graft/src/lib.rs, count corrected. |

## Insertion QA history (AQ2.5)

> **Stale (2026-08-26):** the round-6 PASS at `0b9b5bd91` predates the
> Herdr insertion's rewrites of this doc (+31/+41/+7 lines); round 7 has no
> verdict. Classifier/enum ownership moved to AQ1 on 2026-08-26 (B5).

| Round | Reviewer(s) | Commit | Verdict | Notes |
|---|---|---|---|---|
| 0 | — (initial draft, fenix) | `fd2d05abc` | DRAFT | Delivery-trigger gap identified by Rand 2026-08-24 (no in-tree `TeamMemberHeartbeatRequest` producer; no non-tmux injection policy). Grounded in a verified production Codex Stop-hook baseline (randlee/schook#168). |
| 1 | plan-scope-reviewer (sonnet) | `fd2d05abc` | FAIL — 3 Blocking (machine-global schook migration: scope sourced from external `~/.scripts` policy file; zero AC coverage; non-reproducible out-of-repo change), 3 Important (no CLI signatures/output contracts; ADR addendum un-gated; tmux live-evidence transcript double-owned with AQ3) | Fixed in round-1 fix commit: committed deliverables scoped to in-repo `scripts/hooks/` only, host migration moved to Non-closure as an explicit ops follow-up; clap signatures, stdout/exit-code contracts, and the literal Stop-hook block JSON added; AC 7 (now AC 9 after renumbering) gates the ADR addendum; Required Validation cedes the tmux transcript to AQ3. |
| 1 | critical-plan-reviewer (sonnet) | `fd2d05abc` | FAIL — 4 Blocking (no wire contract for the claim capability; "identical hook signal" contradicted roster-conditional Stop-pull, racing AQ3; `stop_hook_active` guard unspecified and contradicted AC 3's multi-pull; AQ3's sweep would burn attempt budget / false-stuck no-channel members), 2 Important (AC 5 identity gating unenforceable as worded; Windows exclusion rationale inapplicable to the Claude-specific pull path), 1 minor | Fixed in round-1 fix commit: full envelope + route + handler contract (daemon-mediated); trigger table enforced server-side at the get/claim point — hooks stay roster-blind and a tmux member's pull is denied, not raced; normative loop policy (pull allowed under `stop_hook_active`, never block on empty = structural terminator) reconciled with AC 3 (now AC 4); AQ3 sweep gains the channel pre-check; AC 5 rewritten to the honest caller-context bound (no target-member parameter); Claude hook-script unit tests added to the Windows lane, live evidence disclosed as macOS/ubuntu. |
| 2 | plan-scope-reviewer (sonnet) | `297915481` | **PASS** — all six round-1 closures verified; no regressions in the new wire-contract block; deliverable-to-AC mapping complete; 1 minor wording (Dependencies said "Coordinated with AQ3" instead of guideline vocabulary) | Wording fixed in round-2 fix commit (standard must_follow/parallel_safe vocabulary). |
| 2 | critical-plan-reviewer (sonnet) | `297915481` | FAIL — 2 Blocking (PLAN-CRIT-020: sweep pre-check double-owned by AQ2.5 and AQ3 with no code owner/merge order and no AQ3-side AC — false-closure risk; PLAN-CRIT-021: "graft root" is not roster data — it is AQ1.5's `GraftReceiverEndpointStore`, and AQ2.5 lacked the must_follow AQ1.7 dependency AQ2 declares for the same reason), 1 Important (PLAN-CRIT-022: requeue claim-ownership validation unspecified — cross-member attempt-budget griefing surface) | Fixed in round-2 fix commit, which also folds in Rand's 2026-08-24 directives (simplicity mandate; mechanism-positive naming — no "non-tmux"; bare-CLI = RAM-only bounded FIFO + simple get, one queue item per get / all steer items at once, no persistence, staleness-over-loss): PLAN-CRIT-020 — AQ2.5 owns classifier/target/emitter/FIFO, AQ3 owns the sweep pre-check code over the classifier seam with its own AC 6 and `must_follow AQ2.5`, exactly one author per file; PLAN-CRIT-021 — classifier's graft input named as `GraftReceiverEndpointStore::lookup`, `must_follow AQ1.7` added in AQ2.5 + ownership table; PLAN-CRIT-022 — resolved by construction: the simple-get design removes the requeue route and claim tokens from the wire entirely (nothing to validate), with the nudge-loss bound disclosed in deliverable 3 and ADR-054. New selector deliverable per Rand: `DeliveryChannel` classifier + `PostSendBuiltInTarget::QueuePull` + `PullPendingReceivedHook` (third `AsyncMessageReceivedHookEmitter` impl; FIFO append = AQ2-style handoff, marker cleared). |
| 3 | plan-scope-reviewer (sonnet) | `01b1c567c` | FAIL — 1 Blocking (PLAN-SCOPE-001: `PullPendingReceivedHook` + selector arm land in the same `received_hook_selector.rs` AQ2's queue-channel edits touch — `parallel_safe: AQ2` was false; the PLAN-CRIT-020 single-owner fix was never applied to the AQ2/AQ2.5 boundary), 1 Important (PLAN-SCOPE-002: deliverable 4's classifier/target/emitter contract was prose-only, unlike deliverable 3 and AQ3's `MemberStateTransitionSink` precedent) | Fixed in round-3 fix commit: AQ2.5 takes `must_follow AQ2` (AQ2 lands the selector-file diff first; merge-forward trigger AQ2 dev push; chain AQ2 → AQ2.5 → AQ3), recorded in AQ2.5 Dependencies, AQ2's Downstream note, and the ownership table; deliverable 4 gains the explicit Rust block (`DeliveryChannel` enum, pure `classify_delivery_channel` signature over pre-fetched inputs, `QueuePull` variant in enum context, sealed `PullPendingReceivedHook` impl + selector arm). Critical round 3 was terminated mid-run by a session usage limit — re-dispatched against the round-3 fix commit. |
| 4 | plan-scope-reviewer (sonnet) | `6fff8cf5e` | **PASS** — both round-3 closures verified across all four docs (must_follow AQ2 chain consistent in AQ2.5/AQ2/ownership table; deliverable-4 Rust block symmetric with deliverable 3 and AQ3's `MemberStateTransitionSink` precedent); no regressions | — |
| 4 | critical-plan-reviewer (sonnet) | `6fff8cf5e` | FAIL — 2 Blocking (PLAN-CRIT-023: FIFO placed "beside RuntimeHealth" in atm-http-runtime while its writer `PullPendingReceivedHook` lives in atm-daemon-bootstrap, with no composition path in the real selector-factory/router signatures; PLAN-CRIT-024: AQ2 cited a nonexistent `claim_pending` and both AQ2/AQ2.5 handoffs actually need a specific-message clear — `claim_next_pending` selects the OLDEST pending and would clear the wrong marker under a backlog), 1 Important (PLAN-CRIT-025: `send/hook.rs::build_built_in_dispatch` is an unacknowledged AQ1/AQ2.5 shared-file seam), 1 minor (PLAN-CRIT-M4: emitter's `PostSendEmissionPath` variant unspecified) | Fixed in round-4 fix commit: FIFO is composition-root state — `BareCliFifo` constructed once in `run_replacement_daemon_with_selector` (lib.rs ~:217), cloned into `StorageAndNudgeRouter::with_bare_cli_fifo` and a widened `active_received_hook_selector(service_runtime, bare_cli_fifo)` / `selector_factory` signature (explicitly NOT in `LocalServiceRuntime` or `RuntimeHealth`); AQ1's store contract gains `clear_pending_on_handoff(member, msg)` (unconditional, idempotent, specific-message) and AQ2's wrong method name is corrected to it, AQ2.5's emitter clears via it; AQ2.5 owns the `QueuePull` branch of `build_built_in_dispatch` with the AQ1 seam recorded as sequenced single ownership; `PostSendEmissionPath::QueuePull` added to the deliverable-4 sketch and returned by the emitter. |
| 5 | critical-plan-reviewer (sonnet) | `86e4ecac7` | FAIL — 1 Blocking (PLAN-CRIT-026: the 023-closure keyed `BareCliFifo` on `MemberKey`, which is private to `atm-http-runtime::runtime_health` — the sketch didn't compile for the crate it was placed in), 1 Important (PLAN-CRIT-027: the widened `selector_factory` signature breaks the Justfile-wired benchmark harness call site `benchmark_received_hook_selector` → `active_received_hook_selector(service_runtime)`, never mentioned in the doc). Closures 024/025/M4 verified clean against real code. | Fixed in round-5 commit: new `pub struct BareCliMemberKey { team, member }` defined in atm-core beside `QueuedNudgeMessage` (option (a); no visibility widening of the private type) and used in the alias + both code blocks; benchmark harness's Active arm explicitly passes `Arc::default()` (empty FIFO — bare-CLI delivery intentionally outside benchmark semantics), named in the deliverable-4 block so both callers of the widened signature are covered. |
| 6 | critical-plan-reviewer (sonnet) | `0b9b5bd91` | **PASS — closing round** — both round-4 closures verified against real code (`runtime_health.rs:43` private type untouched; `received_hook_selector.rs:55` / `lib.rs:171` benchmark caller covered); round-3 closures re-verified intact; zero findings | AQ2.5 hardening complete (scope PASS round 4, critical PASS round 5); ready for quality-mgr gate on PR #1019. |
| 6 | quality-mgr gate (PR #1019, AQ2.5) | pre-fix head | FAIL — 2 Important (AQ1's PendingNudgeStore keyed on an undefined MemberKey — only real one is private in runtime_health; sealed hook-emitter boundary manifest would go stale on the third implementer), 1 minor (steer=immediate terminology) | Fixed in this commit: AQ1 defines the canonical public atm-core `MemberKey { team, agent }` (one-canonical-type per ruthless-boundary-qa; private runtime_health key untouched, non-blocking consolidation noted); AQ2.5's BareCliMemberKey marked superseded-same-shape with the envelope-field derivation shown at PullPendingReceivedHook::emit; new AQ2.5 AC #11 (numbered #10 at the time) gates the boundary-manifest update; ADR-054 addendum gains the kind-vs-mechanism clarifying sentence. |
| 7 | consistency fix (fenix) | `1dfa0af9f` + this commit | — | Completed the MemberKey migration the round-6 fix started: AQ2.5's code block and `BareCliFifo` alias still *defined* the superseded `BareCliMemberKey` — now keyed directly on AQ1's canonical `MemberKey` (module-qualification note added for the private `runtime_health::MemberKey` name collision); `QueuePullTarget` field comment aligned to `agent`. No design change — same shape, one type, derivation at the emitter unchanged. Ready for quality-mgr re-gate on PR #1019. |
