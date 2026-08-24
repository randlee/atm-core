# Plan — Phase AQ: ATM Send-To Shell Integration

Status: ready_for_scope_review · Source PRD: [prd-atm-send-to.md](./prd-atm-send-to.md)
Reference code: `integrate/phase-ao2` (envelope at
`crates/atm-storage/src/schema/inbox_message.rs`, CLI at
`crates/atm/src/commands/{teams,send}.rs`).

## Scope

PRD Phase 1 only: one gesture from Finder/Explorer to a delivered message with
attachments, same-host and cross-host. PRD Phase 2 (drafting agent, Wyvern
chat sessions) is explicitly out of scope for this plan and will be planned
after the Wyvern chat-window integration exists.

## Binding decisions (from PRD)

- **Pull, not push.** Cross-host attachment bytes are fetched by the receiving
  daemon; the sender ships references `{sha256, size, origin_host,
  origin_path}`. Fetch mechanism is decided in AQ1's ADR against the
  **accepted** cross-host transport stack: ADR-035 (canonical write ingress +
  host routing, active) and ADR-047 (layered peer-wire security — `PeerWireMode`
  default mTLS — which supersedes ADR-034's transport wording and ADR-040;
  the ADR-047 file lives on `integrate/phase-ao2` and reaches `develop` with
  the AO2 merge, a phase-AQ dispatch precondition). ADR-034 remains the
  reference for the single-router HTTP shape it established. Not assumed to
  be sftp. ADR-028 and ADR-031 are superseded and must not be cited as
  authority.
- **No new protocol verb.** `attachments: []` is an optional envelope field
  on `MessageEnvelope` (`crates/atm-storage/src/schema/inbox_message.rs:137`);
  no new `WriteRequest` variant or protocol message kind.
- **R13 chaining invariant.** Every pipeline stage is side-effect-free except
  the final `atm send`.
- **Per-message temp subdirs** `<known-temp>/atm/<msg-id>/` with a
  daemon-owned sweeper.

## Sprints

| Sprint | Title | Depends |
|---|---|---|
| AQ1 | Attachment contract + ADR | — |
| AQ2 | CLI surface: teams projection, send --attach/--from-json, same-host delivery | must_follow AQ1 |
| AQ3 | Cross-host attachment pull | must_follow AQ2 |
| AQ4 | Temp lifecycle sweeper | must_follow AQ2 · parallel_safe AQ3 gated by AQ1 layout contract |
| AQ5 | Wyvern picker + shell glue (macOS Shortcuts, Windows SendTo) | must_follow AQ2 · parallel_safe AQ3, AQ4 |
| AQ6 | Validation evidence | must_follow all |

The table is an ownership map, not a second requirements list. The sprint
documents below are authoritative for each sprint's deliverables, acceptance
criteria, paths-to-delete (when any), and required validation. AQ1 owns the
envelope/layout/transport-policy contract; AQ2 owns the CLI projection,
staging, and same-host write path; AQ3 owns authenticated remote fetch and
pending-delivery state; AQ4 owns reclamation; AQ5 owns picker and platform
shell glue; AQ6 owns phase evidence and the merge gate. No later sprint may
redefine an earlier contract.

Dependency rationale:

- AQ1 must merge first because every later sprint consumes its attachment
  schema, `attachment_dir()` function, limits, and pending-delivery policy.
- AQ2 must merge before AQ3/AQ4/AQ5 because the remote and UI paths consume
  the validated CLI envelope and recipient projection.
- AQ3 and AQ4 are parallel-safe only after AQ1: AQ3 owns fetch/delivery and
  AQ4 owns the sweeper; both call AQ1's path function and neither may alter
  the other's runtime modules or storage state machine.
- AQ5 is parallel-safe with AQ3/AQ4 after AQ2 because it owns scripts/UI
  adapters and linked Wyvern artifacts, not daemon fetch or reclamation code.
- AQ6 must follow AQ1–AQ5 because it certifies the merged phase, not partial
  feature branches.

Branch pattern: `feature/aq-N-<slug>` off `integrate/phase-aq`, PR target
`integrate/phase-aq`. Creating the `integrate/phase-aq` branch/worktree from
`develop` (carrying phase-ao2 merges) at phase start is a dispatch
precondition for AQ1, verified mechanically on the cut head:
`test -f docs/adr/ADR-047-*.md && test -f docs/adr/ADR-053-*.md` — every sprint PR, AQ1 included, targets
`integrate/phase-aq` per the repo integration-branch policy.

## Verified baseline facts (integrate/phase-ao2)

Verified against the reference tree; sprint docs cite these, reviewers should
not re-litigate them:

- `MessageEnvelope` lives at
  `crates/atm-storage/src/schema/inbox_message.rs:137` with established
  back-compat patterns (`#[serde(default, skip_serializing_if =
  "Option::is_none")]`, `RawMessageEnvelope` custom deserialize, flattened
  `extra` map). Storage truth is SQLite (`mail_messages` /
  `mail_message_states`, `crates/atm-storage-rusqlite/src/shared_db.rs`);
  `mail_message_states.acknowledged_at` makes an on-ack sweeper queryable.
- There is **no** `Sha256Hex` type; the nearest precedent is `TemplateSha`
  (`crates/atm-storage/src/types.rs`), a validated lowercase 64-hex newtype.
  `AtmMessageId` (ULID) and `HostName` exist as named.
- `atm teams --json` emits `{name, member_count}` per team only. Per-member
  data lives in `atm members --json` (`MemberSummary`: `name`, `agent_id`,
  `harness`, `model`, `tmux_pane_id`, `home_dir`, `live_cwd`); runtime member
  state is `RuntimeMemberState` = `Unknown | IdentityConflict | Offline |
  Idle | Active`. The PRD §4.2 picker projection (nested members with
  `{id, host, cwd, status}`) **does not exist and must be built** (AQ2),
  including the status mapping.
- No current roster or heartbeat record supplies a member host. AQ1 decision
  (h) therefore makes host sourcing an explicit roster metadata binding,
  managed by `teams add-member/update-member --host` and checked against
  `TrustedPeer`; AQ2 owns the thin projection/resolution implementation and
  does not modify heartbeat or daemon runtime plumbing.
- `atm send` is single-recipient (`to` positional, required) with existing
  flags `--team --host --chat-id --as --file --stdin --template --vars --var
  --tag --category --content-format --summary --requires-ack --task-id
  --dry-run --json`. No fan-out, no attachment support anywhere. Sends go
  through the daemon HTTP transport only (no direct storage writes).
- Cross-host delivery is **stateless HTTPS push**: sender daemon persists
  locally, then POSTs the canonical write to the peer; receiver-side
  persistence *is* delivery (no separate delivery step), and ADR-034
  explicitly rejects outbox/retry/receipt state. There is **no** existing
  byte-fetch, parked-message, dead-letter, or retry machinery. AQ3's pull
  model therefore requires an explicit AQ1 ADR decision on
  pending-delivery semantics (see AQ1 decision (f)) that extends or
  supersedes those constraints — it cannot be assumed.
- Daemon runtime is Tokio+Axum (`atm-http-runtime`) as the only serving
  path; background-task precedent is the retained-log maintenance worker
  (60 s cadence, `crates/atm-daemon/bin_support/daemon_observability.rs`).
  Observability is structured log events + health surface (e.g.
  `queue_full_drops_total`), not a metrics registry.
- `AtmConfig` (`crates/atm-core/src/config/types.rs`) has **no** temp/spool
  directory key today; daemon directory conventions are `~/.atm/{daemon,db,
  logs}` (`crates/atm-core/src/home.rs`).
- CI runs ubuntu + macOS + Windows lanes (`.github/workflows/ci.yml` os
  matrix). A two-daemon peer-pair harness exists
  (`.just/tests/test_peer_pair_smoke.py`, `scripts/smoke/run_peer_pair.py`);
  grep-gate precedent exists (`scripts/check-legacy-mailbox-paths.py`). ADRs
  live in `docs/adr/`; ADR-047 and ADR-053 both exist on
  `integrate/phase-ao2` (merged to `develop` before `integrate/phase-aq` is
  cut — a dispatch precondition; this plan branch predates that merge), so
  the AQ1 ADR is ADR-054. Machine-readable boundary records live at
  repo-root `boundaries/<crate>/*.toml`, enforced by
  `cargo test -p atm-architecture`.
- ADR-018 §3 caps optional `atm-storage` capability traits; ADR-036 used the
  follow-up-ADR mechanism once and closed the door. The two new store traits
  (`AttachmentDeliveryStore`, `AttachmentSweepStore`) therefore require the
  ADR-018 §3 follow-up amendment + `docs/atm-storage/boundaries.md` +
  repo-root `boundaries/atm-storage/*.toml` records + `boundary-guard`
  review, all owned by AQ1 (deliverable 5). `PeerAttachmentSource` is a
  transport-adapter trait in `atm-http-runtime` (its record goes under
  `boundaries/atm-http-runtime/`), outside that cap.

## Open decisions routed to sprints

- Directory attachments: reference vs tar at origin → AQ1 ADR.
- Size limit and over-limit behavior → AQ1 ADR.
- Sweeper policy TTL vs on-ack → AQ1 ADR (decided), AQ4 (implemented).
- Known-temp root: named from existing daemon config in AQ1; if no such
  config key exists, AQ1 adds it. Do not hardcode a path in any sprint.
- Picker projection: the PRD §4.2 teams→members JSON does not exist (see
  baseline facts). AQ2 builds it, including the `RuntimeMemberState` →
  `active|idle|dead` mapping and sourcing of `host`/`cwd`; registration gaps
  become AQ2 deliverables, not surprises.
- Member host binding: AQ1 decides the durable roster field and authority;
  AQ2 must reject unresolved IDs and prove local-vs-remote routing from that
  field only. It may not invent a DNS/heartbeat inference.
- Dedupe storage mechanism (hardlink vs copy) and the sweeper's
  still-referenced check → AQ1 ADR decision (i); AQ3 (reuse) and AQ4
  (reclamation) implement that one mechanism, and both reuse AQ2's canonical
  `resolve_picker_recipient` rather than defining their own resolution.
- Message-id allocation vs staging order: `attachment_dir()` is keyed by
  `AtmMessageId`, so who allocates the ULID (CLI vs daemon) and when staging
  happens relative to the canonical write → AQ1 ADR decision (g).
- Post-fetch `local_path` population mutates a stored envelope; the storage
  update path for that is an AQ3 deliverable, decided in AQ1 decision (f).
- Wyvern cold-start latency measured in AQ5 before the Shortcuts prototype is
  replaced; the Shortcuts/Out-GridView fallback remains shippable.

## Phase-level closure contract

Phase AQ closes only when AQ1–AQ5 each satisfy their sprint acceptance
criteria and AQ6 publishes the requirement-to-evidence matrix. The phase
cannot claim success from a picker demo, schema-only change, or test-only
fixture. A missing cross-host fetch, unverified host projection, unbounded
temp residue, or absent platform evidence remains an open requirement.

The phase-level required validation is the union of the sprint validation
commands plus the two-daemon cross-host suite and the three CI lanes. AQ6 must
record the exact merged SHA, branch, commands, evidence paths, and any
deferred item with its owning follow-on; it may not close a gap by narrative
only.

## Non-closure

- PRD Phase 2 (atm draft, chat sessions, "Open with agent").
- `atm queue` / `atm spawn` shell entries.
- Team-level addressing (client-side fan-out stands for this phase).
