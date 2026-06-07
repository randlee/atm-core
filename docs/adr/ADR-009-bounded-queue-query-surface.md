# ADR-009 — Bounded Queue Query Surface

| Field | Value |
|---|---|
| ID | ADR-009 |
| Status | **Accepted** |
| Date | 2026-05-09 |
| Deciders | Rand Lee |
| Relates to | REQ-P-LIST-001, REQ-P-READ-001, REQ-CORE-LIST-001, REQ-CORE-WORKFLOW-001, ADR-010 |
| Supersedes | — |

---

## Context

ATM mail history is moving onto SQLite-backed durable storage. That means
mailboxes will grow without a practical fixed upper bound. The current `atm read`
shape mixes two different jobs:

- finding candidate messages
- opening full message detail

That coupling causes two product problems:

- queue inspection can flood operator context with long message bodies
- default reads still depend on full-surface materialization instead of a
  bounded query-first path

Phase S also exposed two concrete reliability failures in the current full-read
path:

- GitHub issue `#213`: full-read reliability failure (`failed to write daemon
  response frame` / JSON EOF)
- GitHub issue `#214`: default read path still materializes full history rather
  than a bounded unread/head query

ATM therefore needs a cleaner command split before the SQLite-backed mailbox
surface becomes the only normal source of truth.

## Decision Drivers

- queue inspection must not require rendering full message bodies
- default operational reads must remain bounded even when mailbox history is
  large
- `atm read` should become deterministic and single-message oriented
- shared search semantics should not diverge between metadata listing and full
  message retrieval
- malformed compatibility ingress records are tolerable at the Claude JSONL
  boundary, but malformed durable SQLite rows are not

## Decision

ATM splits queue inspection into two top-level commands:

- `atm list` finds messages
- `atm read` opens one message

### `atm list`

`atm list` is the bounded queue/index surface.

It returns compact metadata rows rather than full message bodies. The canonical
row fields are:

- `message_id`
- `summary`
- `from`
- `timestamp`
- `read`
- `pending_ack`
- `task_id` when present

In JSON output, `task_id` is always present and is `null` when the logical
message is not task-linked.

Default `atm list` behavior is a bounded actionable queue view. Full-history
listing is an explicit opt-in path rather than the implicit default.

### `atm read`

`atm read` is the single-message detail surface.

It returns exactly one full message. Message selection rules are:

- `--message-id <id>` selects that exact message when present
- shared match filters such as `--task`, `--from`, `--since`, `--contains`,
  `--unread`, and `--pending-ack` select a candidate set
- successor/update chains are one logical message; selection operates on the
  current terminal node for each chain rather than on superseded predecessors
- `--task <task-id>` first finds task-linked messages, then collapses each
  successor chain to its terminal node before choosing the most recent logical
  current message
- when multiple matches remain, `atm read` returns the most recent match
- the response must also expose:
  - `selected_message_id`
  - `match_count`
  - `additional_match_count`

`match_count` is the total number of logical current-message matches after all
filters and successor-chain collapse are applied. `additional_match_count` is
`match_count - 1` for a successful read.

Bare `atm read` with no explicit selector returns the most recent unread
actionable message, prioritizing pending-ack messages ahead of non-ack unread
messages.

### Shared Filter Contract

`atm list` and `atm read` share the same semantic search filters for matching
messages. The accepted S.5 baseline is:

- optional target inbox (`agent` or `agent@team`)
- `--team`
- `--from`
- `--since`
- `--task`
- `--contains`
- `--unread`
- `--pending-ack`
- `--all`

List-specific pagination and presentation controls such as `--limit` may remain
list-only, but the message-selection semantics themselves must stay aligned.

`--contains` searches both summary text and full message body text.

### Legacy Flag Migration

The old multi-message `atm read` surface exposed flags whose names no longer
fit the split command model. Phase S.5 adopts this migration:

- `--unread-only` is a deprecated alias for `--unread`
- `--pending-ack-only` is a deprecated alias for `--pending-ack`
- `--history` is a deprecated alias for `--all`
- `--since-last-seen` remains accepted as an explicit restatement of the
  default seen-state behavior
- `--no-since-last-seen` remains the opt-out for the default seen-state filter

Deprecation warnings must direct operators toward the new flag names. The
long-term surface keeps one naming system rather than carrying the legacy names
indefinitely.

### Bounded Query Rule

Default queue inspection must be bounded by query shape, not merely by final
render truncation.

That means:

- default `atm list` must not materialize full mailbox history just to throw
  most of it away
- default `atm read` must not behave like "list many, then print the first"
- summary/count queries must be able to execute without loading every message
  body into the operator-facing response path
- metadata search and full-message fetch must remain separate service paths
  rather than one broad mailbox materialization step followed by local
  filtering

### Durable Data Integrity Rule

Malformed shared-inbox JSON records remain a compatibility-ingress problem and
may degrade per record.

Malformed durable SQLite rows are different:

- ATM-owned SQLite message rows must contain valid serialized message state
- malformed durable rows are corruption/store-failure conditions, not a normal
  skip-and-continue read mode

## Consequences

### Positive

- queue inspection no longer floods context with message bodies
- `atm read` becomes deterministic and easier to script
- default operational queries remain bounded as mailbox history grows
- list/read command semantics become easier to explain and test

### Negative

- the existing `atm read` multi-message surface must be retired or migrated
- current `ReadQuery` / `ReadOutcome` shapes become transitional rather than
  the long-term command model
- implementation must separate metadata search from detail fetch rather than
  relying on one broad mailbox materialization path

## Follow-Up Work

- S.5 planning for `atm list` and single-message `atm read` semantics is
  recorded in `docs/plans/phase-S/sprint-S5.md` Required Work §7.
- update product and crate-local CLI documentation so `atm list` owns queue
  search and `atm read` owns detail fetch
- redesign the mailbox query service so default list/read flows are bounded by
  query behavior instead of full-surface materialization
