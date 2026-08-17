# ATM CLI Reference

This document is generated from the live `clap` command tree. Do not hand-edit it — regenerate with `cargo run -p agent-team-mail --example gen_cli_docs` (see `crates/atm/src/cli_surface.rs`).

## `atm`

ATM CLI

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

### `atm ack`

Acknowledge one pending-ack message and emit a reply when required

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<message_id>` |  | yes |  |
| `<reply>` |  | yes |  |
| `--team` |  | no |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

### `atm api`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm api spec`

Print the versioned daemon OpenAPI contract

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--format` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

### `atm clear`

Clear read or acknowledged messages from a mailbox

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--team` |  | no |  |
| `--older-than` |  | no |  |
| `--idle-only` |  | no |  |
| `--dry-run` |  | no |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

### `atm doctor`

Run ATM health and configuration diagnostics

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--team` |  | no | Override the resolved team for the doctor check. |
| `--json` |  | no | Emit the doctor report as JSON. |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

### `atm help`

Show ATM-owned conceptual help or delegated clap subcommand help

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<target>` |  | no |  |
| `--list` |  | no |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

### `atm list`

List one ATM mailbox surface as bounded metadata rows

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<target>` |  | no |  |
| `--team` |  | no |  |
| `--all` |  | no |  |
| `--unread` |  | no |  |
| `--pending-ack` |  | no |  |
| `--limit` |  | no |  |
| `--since` |  | no |  |
| `--from` |  | no |  |
| `--task` |  | no |  |
| `--contains` |  | no |  |
| `--json` |  | no |  |
| `--as` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

### `atm log`

Query or follow ATM retained observability records

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm log filter`

Query ATM log records using explicit field filters

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--level` |  | no | Restrict results to one or more severity levels |
| `--match` |  | no | Match one structured ATM field exactly, for example command=send |
| `--since` |  | no | Inclusive lower time bound as RFC3339 or a relative duration like 15m |
| `--limit` |  | no | Maximum number of returned records |
| `--json` |  | no | Emit machine-readable JSON output |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm log snapshot`

Query recent ATM log records

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--level` |  | no | Restrict results to one or more severity levels |
| `--match` |  | no | Match one structured ATM field exactly, for example command=send |
| `--since` |  | no | Inclusive lower time bound as RFC3339 or a relative duration like 15m |
| `--limit` |  | no | Maximum number of returned records |
| `--json` |  | no | Emit machine-readable JSON output |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm log tail`

Follow new ATM log records as they arrive

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--level` |  | no | Restrict results to one or more severity levels |
| `--match` |  | no | Match one structured ATM field exactly, for example command=send |
| `--since` |  | no | Inclusive lower time bound as RFC3339 or a relative duration like 15m |
| `--limit` |  | no | Maximum number of returned records |
| `--json` |  | no | Emit machine-readable JSON output |
| `--poll-interval-ms` |  | no | Poll interval in milliseconds between follow polls |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

### `atm members`

List the current member roster for one ATM team

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--team` |  | no |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

### `atm peek`

Inspect one ATM mailbox message without mutating mailbox state

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<target>` |  | no |  |
| `--team` |  | no |  |
| `--all` |  | no |  |
| `--unread` |  | no |  |
| `--unread-only` |  | no |  |
| `--pending-ack` |  | no |  |
| `--pending-ack-only` |  | no |  |
| `--history` |  | no |  |
| `--message-id` |  | no |  |
| `--task` |  | no |  |
| `--contains` |  | no |  |
| `--since-last-seen` |  | no |  |
| `--no-since-last-seen` |  | no |  |
| `--since` |  | no |  |
| `--from` |  | no |  |
| `--json` |  | no |  |
| `--timeout` |  | no |  |
| `--as` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

### `atm peer`

Manage durable cross-host HTTPS control-plane configuration

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm peer certificate`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

##### `atm peer certificate init`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--fingerprint` |  | yes |  |
| `--private-key-ref` |  | yes |  |
| `--yes` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

##### `atm peer certificate show`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm peer interface`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

##### `atm peer interface list`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

##### `atm peer interface remove`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--bind` |  | yes |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

##### `atm peer interface set`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--bind` |  | yes |  |
| `--advertise-host` |  | yes |  |
| `--enabled` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm peer sync`

Immediately re-send one bounded batch of recent immutable outbound records to
one configured trusted peer. The durable sync policy must be enabled; this
command creates no background job, queue, or retry state.

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--json` |  | no | Emit the peer and delivered-record count as JSON |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm peer sync-policy`

View or configure the maximum age of canonical outbound records eligible for
bounded peer reconciliation. `0s` disables automatic and explicit sync.

##### `atm peer sync-policy show <peer>`

##### `atm peer sync-policy set <peer> --max-message-age <whole-seconds>s`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--max-message-age` |  | yes | Positive whole-second age, or `0s` to disable sync |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm peer trust`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

##### `atm peer trust add`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--host` |  | yes |  |
| `--fingerprint` |  | yes |  |
| `--yes` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

##### `atm peer trust list`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

##### `atm peer trust replace`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--host` |  | yes |  |
| `--fingerprint` |  | yes |  |
| `--yes` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

##### `atm peer trust revoke`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--host` |  | yes |  |
| `--yes` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

### `atm read`

Read one ATM mailbox message and optionally update read state

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--team` |  | no |  |
| `--chat-id` |  | no |  |
| `--as` |  | no |  |
| `--all` |  | no |  |
| `--unread` |  | no |  |
| `--unread-only` |  | no |  |
| `--pending-ack` |  | no |  |
| `--pending-ack-only` |  | no |  |
| `--history` |  | no |  |
| `--message-id` |  | no |  |
| `--task` |  | no |  |
| `--contains` |  | no |  |
| `--since-last-seen` |  | no |  |
| `--no-since-last-seen` |  | no |  |
| `--since` |  | no |  |
| `--from` |  | no |  |
| `--json` |  | no |  |
| `--timeout` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

### `atm send`

Send one ATM mailbox message

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<to>` |  | yes |  |
| `<message>` |  | no |  |
| `--team` |  | no |  |
| `--chat-id` |  | no |  |
| `--as` |  | no |  |
| `--file` |  | no |  |
| `--stdin` |  | no |  |
| `--summary` |  | no |  |
| `--requires-ack` |  | no |  |
| `--task-id` |  | no |  |
| `--dry-run` |  | no |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

**Notes:**

Post-send hooks can be configured in .atm.toml via one or more [[atm.post_send_hooks]] rules with recipient = "name-or-*" and command = ["argv", ...]. Matching rules run after a successful non-dry-run send, in config order. Path-like command[0] values resolve relative to the declaring .atm.toml; bare executables like bash or python3 use normal PATH resolution. Recipient non-match is silent. For hook troubleshooting, combine --stderr-logs with ATM_LOG=debug to surface debug-level hook diagnostics on stderr.

### `atm teams`

List teams or run one team-administration subcommand

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm teams add-member`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<team>` |  | yes |  |
| `<member>` |  | yes |  |
| `--agent-type` |  | no |  |
| `--model` |  | no |  |
| `--home-dir` |  | no |  |
| `--pane-id` |  | no | tmux pane id in '%<number>' form or a bare numeric pane id |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm teams backup`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<team>` |  | yes |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm teams clear-nudge-template`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--team` |  | yes |  |
| `--kind` |  | yes |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm teams disable-nudge-template`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--team` |  | yes |  |
| `--kind` |  | yes |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm teams restore`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<team>` |  | yes |  |
| `--from` |  | no |  |
| `--dry-run` |  | no |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm teams set-nudge-template`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--team` |  | yes |  |
| `--kind` |  | yes |  |
| `--template-body` |  | yes |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm teams update-member`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<team>` |  | yes |  |
| `<member>` |  | yes |  |
| `--home-dir` |  | no |  |
| `--harness` |  | no |  |
| `--agent-type` |  | no |  |
| `--model` |  | no |  |
| `--pane-id` |  | no | tmux pane id in '%<number>' form or a bare numeric pane id |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

