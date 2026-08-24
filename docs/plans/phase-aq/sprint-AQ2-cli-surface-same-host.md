# Sprint AQ2 — CLI Surface and Same-Host Delivery

Status: draft · Branch: `feature/aq-2-cli-surface` off `integrate/phase-aq` ·
PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Makes the pipeline real on one host:
`atm teams --json --members | <picker> | atm send --attach "$@" --from-json`.

## Deliverables

1. **Picker projection for `atm teams --json --members`** (build, not audit — verified
   baseline: today's output is `{name, member_count}` per team with no member
   entries). Add the `--members` projection to emit the PRD
   §4.2 nested projection `{id, name, host, cwd, status}` per member, sourced
   from the members surface (`MemberSummary`: `agent_id`, `home_dir`,
   `live_cwd`, pane) and runtime state. Includes the normative status
   mapping from `RuntimeMemberState`: `Active → active`, `Idle → idle`,
   `Offline | Unknown | IdentityConflict → dead`. `host` is sourced only
   from AQ1 decision (h)'s durable roster binding; AQ2 adds the `--host`
   admin metadata plumbing needed to write/read that field, but does not
   change heartbeat or daemon runtime code. Missing host is emitted as `null`
   and the picker marks the member unroutable.
2. **`atm send --attach <path>...`** (repeatable): computes sha256 + size,
   copies same-host content into `attachment_dir()` (AQ1), populates
   `attachments` on the envelope. Missing/unreadable path → hard error before
   any send.
3. **`atm send --from-json`**: reads `{recipients: [...], note?: string}`
   from stdin; fan-out is one envelope per recipient (new — send is
   single-recipient today, positional `to` required); `note` becomes message
   text with `note_source` per AQ1. Requires making positional `to` and
   `message` optional at the clap level; mutually exclusive with positional
   `to`, `--stdin`, `--file`, and `--template`; conflicts rejected by clap,
   not runtime checks. Fan-out delivery goes through the existing daemon
   HTTP write path per recipient — no direct storage writes.
4. **Cancel semantics (R5/R13)**: empty stdin or malformed JSON → nonzero
   exit, zero sends, zero files created. Attachment staging happens only
   after recipient JSON validates, with msg-id allocation/staging order per
   AQ1 ADR decision (g).
5. **Cross-host refusal stub**: recipient on another host → explicit
   "cross-host attachments land in AQ3" error, never a reference-only send.

## Normative CLI contracts

The new picker projection is exposed as `atm teams --json --members`; the
existing `atm teams --json` team-count shape remains backwards-compatible.
Each member object is exactly:

```json
{"id":"agent-id","name":"display-name","host":"host-or-null",
 "cwd":"absolute-or-null","status":"active|idle|dead"}
```

The source record is explicit: `RosterEntry.metadata_json["host"]` stores an
optional validated `HostName`, `MemberSummary` projects it as
`host: Option<HostName>`, and `teams add-member/update-member --host` is the
only Phase-1 writer. The implementation must add/update that metadata through
the existing roster mutation requests; no heartbeat or live socket observation
may populate it.

`atm send --from-json` accepts exactly one JSON object on stdin:

```json
{"recipients":["member-id", "member-id-2"], "note":"optional text"}
```

The parser rejects unknown keys, empty recipients, duplicate recipients,
malformed JSON, and trailing non-whitespace. It validates all recipients and
all attachment paths before allocating any message ID or creating a staging
directory. It then calls the existing canonical daemon write client once per
recipient; no CLI storage adapter is permitted.

Recipient resolution is explicit and testable:

```rust
fn resolve_picker_recipient(
    member_id: &str,
    roster: &dyn RosterStore,
    peers: &dyn PeerConfigStore,
) -> Result<AgentAddress, AddressResolutionError>;
```

The resolver maps a null-host member to the local canonical team address,
maps a registered host only when it matches an enabled `TrustedPeer`, and
rejects an unresolved/disabled remote member before staging or writing. It
never derives host from heartbeat, DNS, socket family, or the picker UI.

## Acceptance criteria

1. Truth-table tests for `--from-json`: valid multi-recipient → N envelopes
   each with identical `attachments` refs; empty/malformed/cancel → exit ≠ 0
   and `attachment_dir()` for the would-be msg-ids absent.
2. Same-host end-to-end test: file lands under `<known-temp>/atm/<msg-id>/`,
   sha256 verified, recipient reads envelope with populated `local_path`.
3. Duplicate content to two recipients produces two envelopes whose
   attachments share `sha256` (dedupe observable at the reference level).
4. `atm teams --json --members` output validates against the picker input schema in the
   PRD (§4.2) via a fixture test.
5. `atm send --from-json` resolves every recipient through the AQ1 host binding
   and proves local-vs-remote routing; null/unknown/disabled host bindings
   fail closed before staging.
6. Legacy single-recipient `atm send <to> <message>` still requires both
   positional arguments and preserves its existing malformed/missing-input
   exit codes/messages after the parser makes them optional for `--from-json`.
7. `just test` all three CI lanes (ubuntu, macOS, Windows); no clippy
   warnings in touched crates.

## Paths to delete

None. AQ2 extends the CLI and adds staging code; it must not delete the
existing single-recipient `atm send` mode or alter legacy team-count output.

## Required validation

- `just test` workspace, ubuntu + macOS + Windows CI lanes.
- One recorded same-host demo transcript (command + resulting envelope JSON)
  committed as evidence on the sprint branch.
- Focused command tests for `atm teams --json --members` and
  `atm send --from-json` are named in the PR and run independently of Wyvern.
- The legacy `atm send` positional compatibility test runs in the same command
  suite; its expected diagnostics are captured from the pre-AQ2 baseline.

## Non-closure / out of scope

- Cross-host fetch (AQ3). Sweeper (AQ4). Any UI (AQ5).

## Dependencies

- must_follow: AQ1 (contract) — merge-forward before every dev/fix round so
  the CLI never stages against a stale envelope/layout or pending-delivery
  rule.
- Dispatch precondition: `integrate/phase-aq` created from `develop`.
- parallel_safe: none before AQ2 merges; AQ3/AQ4/AQ5 may fan out only after
  they consume this exact projection and canonical send contract.
