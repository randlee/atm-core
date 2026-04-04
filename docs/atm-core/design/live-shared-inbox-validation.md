# Live Shared-Inbox Validation

## Scope

This report closes Phase `J.5` by exercising the current shared-inbox design in
real and controlled operator flows before any ATM-native inbox redesign.

The validation target is the current architecture documented in:

- [`docs/claude-code-message-schema.md`](../../claude-code-message-schema.md)
- [`docs/legacy-atm-message-schema.md`](../../legacy-atm-message-schema.md)
- [`docs/atm-message-schema.md`](../../atm-message-schema.md)
- [`docs/atm-core/design/dedup-metadata-schema.md`](./dedup-metadata-schema.md)

## Evidence Sources

### 1. Live inbox files under `~/.claude`

Observed on April 4, 2026 in:

- `~/.claude/teams/atm-dev/inboxes/arch-ctm.json`
- `~/.claude/teams/atm-dev/inboxes/quality-mgr.json`

Current live messages still use legacy ATM top-level fields such as:

- `message_id`
- `source_team`
- `pendingAckAt`

The sampled live messages did not contain `metadata.atm`.

### 2. Prior live Claude-context projection test

`quality-mgr` verified a real ATM CLI send against Claude context injection and
found:

- on-disk ATM CLI messages persisted `message_id` and `source_team`
- Claude context injection stripped those ATM-specific fields
- the injected context reduced the message to teammate-visible text/summary only

This confirms that Claude context is a lossy projection of the shared inbox, not
the authoritative machine-readable ATM surface.

### 3. Controlled manual ATM CLI session

A disposable `ATM_HOME` was populated with realistic inbox files and team
config, then exercised with:

- `atm read --all --no-mark --json`
- `atm read --pending-ack-only --no-mark --json`
- `atm ack 11111111-1111-4111-8111-111111111111 "received and starting" --json`

The controlled inbox included:

- two unread Claude-native idle notifications from the same sender
- one read/history idle notification from the same sender
- one pending-ack ATM task message using legacy top-level `message_id`
- one message carrying forward-looking `metadata`

## Findings

### Shared inbox is usable for ATM CLI workflows today

The controlled `read --all --no-mark --json` run returned four visible messages:

- the newest unread idle notification from `team-lead`
- the read/history idle notification from `team-lead`
- the pending-ack task message with legacy top-level `message_id`
- the `metadata`-carrying message from `quality-mgr`

This shows that the current read path already supports the mixed shared-inbox
state needed for live use:

- Claude-native idle notices remain Claude-shaped in `text`
- unread idle dedup keeps only the latest unread idle notice per sender
- history/read idle notices remain visible
- legacy ATM top-level fields remain usable
- `metadata` is preserved in CLI JSON output when present

### Ack workflow works end-to-end on the shared inbox

The controlled `ack` run succeeded against the pending-ack task message.

Observed source-message mutation in `arch-ctm.json`:

- `pendingAckAt` was cleared
- `acknowledgedAt` was added
- the original legacy `message_id` remained stable
- `taskId` remained stable

Observed reply append in `team-lead.json`:

- ATM generated a new reply `message_id`
- ATM added `acknowledgesMessageId`
- the reply remained readable through the same shared-inbox storage surface

This is sufficient to support current operator ack workflows without an
ATM-native inbox.

### Claude context projection remains intentionally lossy

The live projection evidence matters more than the simulated fixture:

- ATM-specific machine fields are available on disk and through ATM CLI reads
- Claude context injection does not preserve those machine fields

Operational consequence:

- agents can rely on Claude-injected teammate context for human-readable
  awareness
- agents cannot rely on Claude-injected context for ATM machine workflows such
  as ack lookup, dedup diagnosis, or message-id-based automation

Machine workflows must continue to use ATM CLI surfaces such as `atm read`.

### `metadata.atm` is a viable forward target, but not yet the live write path

The live inbox sample showed no current `metadata.atm` usage.

The controlled `read` session showed that a `metadata` object remains visible in
ATM CLI JSON output when present, including:

- `metadata.atm.alertKind`
- `metadata.atm.sourceTeam`
- foreign/shared keys such as `priority` and `repo`

That is enough to support the current design direction:

- legacy ATM top-level fields stay read-compatible
- forward ATM-authored machine fields can move into `metadata.atm`
- shared and foreign metadata can coexist there without redefining Claude-native
  fields

However, `metadata.atm` is not yet the primary live operator path because the
current runtime still writes legacy top-level ATM fields.

## Gaps Found

No J.5 runtime blocker was reproduced.

No code change was required to complete the live validation report. The current
gaps are documented design boundaries, not immediate runtime failures:

- Claude context projection is lossy and must not be treated as the ATM machine
  contract
- `metadata.atm` remains a forward migration target rather than the current live
  write path

## Verdict

The current shared-inbox design is usable enough to defer ATM-native inbox work
to a later version.

That verdict is conditional on the following operational rule:

- ATM CLI remains the authoritative machine-readable workflow surface for
  message ids, ack state, and other ATM-specific metadata
- Claude context injection is treated as a human-context projection only

Under that rule, the current design is fit to run live while the later
ATM-native inbox redesign stays deferred as a separate versioned architecture
change.
