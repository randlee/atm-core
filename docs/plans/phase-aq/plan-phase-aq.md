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
  origin_path}`. Fetch mechanism is decided in AQ1's ADR against the existing
  cross-host transport (ADR-031/034/035), not assumed to be sftp.
- **No new MessageKind verb.** `attachments: []` is an optional envelope
  field.
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
`develop` (carrying phase-ao2 merges) is a dispatch precondition for AQ2.

## Open decisions routed to sprints

- Directory attachments: reference vs tar at origin → AQ1 ADR.
- Size limit and over-limit behavior → AQ1 ADR.
- Sweeper policy TTL vs on-ack → AQ1 ADR (decided), AQ4 (implemented).
- Known-temp root: named from existing daemon config in AQ1; if no such
  config key exists, AQ1 adds it. Do not hardcode a path in any sprint.
- Member `{host, cwd, status}` availability in `atm teams --json` output is
  verified in AQ2; registration gaps become AQ2 deliverables, not surprises.
- Wyvern cold-start latency measured in AQ5 before the Shortcuts prototype is
  replaced; the Shortcuts/Out-GridView fallback remains shippable.

## Non-closure

- PRD Phase 2 (atm draft, chat sessions, "Open with agent").
- `atm queue` / `atm spawn` shell entries.
- Team-level addressing (client-side fan-out stands for this phase).
