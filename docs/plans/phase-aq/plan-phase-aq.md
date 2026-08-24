# Plan — Phase AQ: ATM Send-To Shell Integration

Status: draft · Source PRD: [prd-atm-send-to.md](./prd-atm-send-to.md)
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
  **accepted** cross-host transport, ADR-034 (minimal cross-host HTTPS,
  port 43101) and ADR-035 (canonical write ingress + host routing) — not
  assumed to be sftp. ADR-028 and ADR-031 are superseded and must not be
  cited as authority.
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

Branch pattern: `feature/aq-N-<slug>` off `integrate/phase-aq`, PR target
`integrate/phase-aq`. Creating the `integrate/phase-aq` branch/worktree from
`develop` (carrying phase-ao2 merges) at phase start is a dispatch
precondition for AQ1 — every sprint PR, AQ1 included, targets
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
  live in `docs/adr/`; highest is ADR-053, so the AQ1 ADR is ADR-054.

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
- Message-id allocation vs staging order: `attachment_dir()` is keyed by
  `AtmMessageId`, so who allocates the ULID (CLI vs daemon) and when staging
  happens relative to the canonical write → AQ1 ADR decision (g).
- Post-fetch `local_path` population mutates a stored envelope; the storage
  update path for that is an AQ3 deliverable, decided in AQ1 decision (f).
- Wyvern cold-start latency measured in AQ5 before the Shortcuts prototype is
  replaced; the Shortcuts/Out-GridView fallback remains shippable.

## Non-closure

- PRD Phase 2 (atm draft, chat sessions, "Open with agent").
- `atm queue` / `atm spawn` shell entries.
- Team-level addressing (client-side fan-out stands for this phase).
