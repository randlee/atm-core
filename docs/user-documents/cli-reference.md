# ATM CLI Reference

Version: `1.6.1`

This document is generated from the live `clap` command tree. Do not hand-edit it — regenerate with `cargo run -p agent-team-mail --features cli-surface-dump --example gen_cli_docs` (see `crates/atm/src/cli_surface.rs`).

## `atm`

ATM CLI

**Usage:**

```text
Usage: atm [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm ack`

Acknowledge one pending-ack message and emit a reply when required

**Usage:**

```text
Usage: atm ack [OPTIONS] <MESSAGE_ID> <REPLY>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<message_id>` |  | `MESSAGE_ID` | yes |  |  |  |
| `<reply>` |  | `REPLY` | yes |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm api`

**Usage:**

```text
Usage: atm api [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm api spec`

Print the versioned daemon OpenAPI contract

**Usage:**

```text
Usage: atm api spec [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--format` |  | `FORMAT` | no | `yaml` | `json`, `yaml` |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm clear`

Clear read or acknowledged messages from a mailbox

**Usage:**

```text
Usage: atm clear [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--team` |  | `TEAM` | no |  |  |  |
| `--older-than` |  | `DURATION` | no |  |  |  |
| `--idle-only` |  | `IDLE_ONLY` | no | `false` |  |  |
| `--dry-run` |  | `DRY_RUN` | no | `false` |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm compose`

Render a template through the core renderer port and print the exact body

**Usage:**

```text
Usage: atm compose [OPTIONS] --template <PATH>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--template` |  | `PATH` | yes |  |  | Template file to validate and render |
| `--vars` |  | `FILE|-` | no |  |  | JSON object providing template variables; `-` reads stdin |
| `--var` |  | `KEY=VALUE` | no |  |  | One template variable; may be repeated |
| `--env-prefix` |  | `PREFIX` | no |  |  | Capture environment variables with this prefix |
| `--dry-run` |  | `DRY_RUN` | no | `false` |  | Validate and render without any side effects (the default operation is already side-effect free; the flag makes scripts self-documenting) |
| `--json` |  | `JSON` | no | `false` |  | Emit a structured result instead of the byte-identical rendered body |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

**Notes:**

Composition is local and never reads or writes the mailbox. Use `atm send <agent> --template <path> --vars <file>` to deliver the same template after previewing it.

### `atm doctor`

Run ATM health and configuration diagnostics

**Usage:**

```text
Usage: atm doctor [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--team` |  | `TEAM` | no |  |  | Override the resolved team for the doctor check. |
| `--all-teams` |  | `ALL_TEAMS` | no | `false` |  | Inspect every team in the canonical roster. |
| `--json` |  | `JSON` | no | `false` |  | Emit the doctor report as JSON. |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm escalation`

Manage daemon-wide and per-team escalation recipients

**Usage:**

```text
Usage: atm escalation [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm escalation add`

**Usage:**

```text
Usage: atm escalation add [OPTIONS] <ADDRESS>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<address>` |  | `ADDRESS` | yes |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm escalation list`

**Usage:**

```text
Usage: atm escalation list [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--team` |  | `TEAM` | no |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm escalation remove`

**Usage:**

```text
Usage: atm escalation remove [OPTIONS] <ADDRESS>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<address>` |  | `ADDRESS` | yes |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm help`

Show ATM-owned conceptual help or delegated clap subcommand help

**Usage:**

```text
Usage: atm help [OPTIONS] [TARGET]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<target>` |  | `TARGET` | no |  |  |  |
| `--list` |  | `LIST` | no | `false` |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm list`

List one ATM mailbox surface as bounded metadata rows

**Usage:**

```text
Usage: atm list [OPTIONS] [TARGET]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<target>` |  | `TARGET` | no |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--all` |  | `ALL` | no | `false` |  |  |
| `--unread` |  | `UNREAD` | no | `false` |  |  |
| `--pending-ack` |  | `PENDING_ACK` | no | `false` |  |  |
| `--limit` |  | `LIMIT` | no |  |  |  |
| `--since` |  | `SINCE` | no |  |  |  |
| `--from` |  | `FROM` | no |  |  |  |
| `--task` |  | `TASK` | no |  |  |  |
| `--contains` |  | `CONTAINS` | no |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--as` |  | `ACTOR` | no |  |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm log`

Query or follow ATM retained observability records

**Usage:**

```text
Usage: atm log [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm log filter`

Query ATM log records using explicit field filters

**Usage:**

```text
Usage: atm log filter [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--source` |  | `SOURCE` | no | `jsonl` | `jsonl`, `timeline`, `merged` | Select the retained-log source. JSONL is the byte-compatible default |
| `--level` |  | `LEVELS` | no |  |  | Restrict results to one or more severity levels |
| `--match` |  | `KEY=VALUE` | no |  |  | Match one structured ATM field exactly, for example command=send |
| `--since` |  | `SINCE` | no |  |  | Inclusive lower time bound as RFC3339 or a relative duration like 15m |
| `--until` |  | `UNTIL` | no |  |  | Inclusive upper time bound as RFC3339 or a relative duration |
| `--component` |  | `COMPONENT` | no |  |  | Restrict timeline records to a component prefix |
| `--limit` |  | `LIMIT` | no |  |  | Maximum number of returned records |
| `--json` |  | `JSON` | no | `false` |  | Emit machine-readable JSON output |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm log snapshot`

Query recent ATM log records

**Usage:**

```text
Usage: atm log snapshot [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--source` |  | `SOURCE` | no | `jsonl` | `jsonl`, `timeline`, `merged` | Select the retained-log source. JSONL is the byte-compatible default |
| `--level` |  | `LEVELS` | no |  |  | Restrict results to one or more severity levels |
| `--match` |  | `KEY=VALUE` | no |  |  | Match one structured ATM field exactly, for example command=send |
| `--since` |  | `SINCE` | no |  |  | Inclusive lower time bound as RFC3339 or a relative duration like 15m |
| `--until` |  | `UNTIL` | no |  |  | Inclusive upper time bound as RFC3339 or a relative duration |
| `--component` |  | `COMPONENT` | no |  |  | Restrict timeline records to a component prefix |
| `--limit` |  | `LIMIT` | no |  |  | Maximum number of returned records |
| `--json` |  | `JSON` | no | `false` |  | Emit machine-readable JSON output |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm log tail`

Follow new ATM log records as they arrive

**Usage:**

```text
Usage: atm log tail [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--source` |  | `SOURCE` | no | `jsonl` | `jsonl`, `timeline`, `merged` | Select the retained-log source. JSONL is the byte-compatible default |
| `--level` |  | `LEVELS` | no |  |  | Restrict results to one or more severity levels |
| `--match` |  | `KEY=VALUE` | no |  |  | Match one structured ATM field exactly, for example command=send |
| `--since` |  | `SINCE` | no |  |  | Inclusive lower time bound as RFC3339 or a relative duration like 15m |
| `--until` |  | `UNTIL` | no |  |  | Inclusive upper time bound as RFC3339 or a relative duration |
| `--component` |  | `COMPONENT` | no |  |  | Restrict timeline records to a component prefix |
| `--limit` |  | `LIMIT` | no |  |  | Maximum number of returned records |
| `--json` |  | `JSON` | no | `false` |  | Emit machine-readable JSON output |
| `--poll-interval-ms` |  | `POLL_INTERVAL_MS` | no | `250` |  | Poll interval in milliseconds between follow polls |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm members`

List the current member roster for one ATM team

**Usage:**

```text
Usage: atm members [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--team` |  | `TEAM` | no |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm peek`

Inspect one ATM mailbox message without mutating mailbox state

**Usage:**

```text
Usage: atm peek [OPTIONS] [TARGET]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<target>` |  | `TARGET` | no |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--all` |  | `ALL` | no | `false` |  |  |
| `--unread` |  | `UNREAD` | no | `false` |  |  |
| `--unread-only` |  | `UNREAD_ONLY` | no | `false` |  |  |
| `--pending-ack` |  | `PENDING_ACK` | no | `false` |  |  |
| `--pending-ack-only` |  | `PENDING_ACK_ONLY` | no | `false` |  |  |
| `--history` |  | `HISTORY` | no | `false` |  |  |
| `--message-id` |  | `MESSAGE_ID` | no |  |  |  |
| `--task` |  | `TASK` | no |  |  |  |
| `--contains` |  | `CONTAINS` | no |  |  |  |
| `--since-last-seen` |  | `SINCE_LAST_SEEN` | no | `false` |  |  |
| `--no-since-last-seen` |  | `NO_SINCE_LAST_SEEN` | no | `false` |  |  |
| `--since` |  | `SINCE` | no |  |  |  |
| `--from` |  | `FROM` | no |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--timeout` |  | `TIMEOUT` | no |  |  |  |
| `--as` |  | `ACTOR` | no |  |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm peer`

Manage durable cross-host HTTPS control-plane configuration

**Usage:**

```text
Usage: atm peer [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm peer certificate`

**Usage:**

```text
Usage: atm peer certificate [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

##### `atm peer certificate init`

**Usage:**

```text
Usage: atm peer certificate init [OPTIONS] --fingerprint <FINGERPRINT> --private-key-ref <PRIVATE_KEY_REF>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--fingerprint` |  | `FINGERPRINT` | yes |  |  |  |
| `--private-key-ref` |  | `PRIVATE_KEY_REF` | yes |  |  |  |
| `--yes` |  | `YES` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

##### `atm peer certificate show`

**Usage:**

```text
Usage: atm peer certificate show [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm peer interface`

**Usage:**

```text
Usage: atm peer interface [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

##### `atm peer interface list`

**Usage:**

```text
Usage: atm peer interface list [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

##### `atm peer interface remove`

**Usage:**

```text
Usage: atm peer interface remove [OPTIONS] --bind <BIND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--bind` |  | `BIND` | yes |  |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

##### `atm peer interface set`

**Usage:**

```text
Usage: atm peer interface set [OPTIONS] --bind <BIND> --advertise-host <ADVERTISE_HOST>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--bind` |  | `BIND` | yes |  |  |  |
| `--advertise-host` |  | `ADVERTISE_HOST` | yes |  |  |  |
| `--enabled` |  | `ENABLED` | no | `true` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm peer trust`

**Usage:**

```text
Usage: atm peer trust [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

##### `atm peer trust add`

**Usage:**

```text
Usage: atm peer trust add [OPTIONS] --host <HOST> --fingerprint <FINGERPRINT>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--host` |  | `HOST` | yes |  |  |  |
| `--fingerprint` |  | `FINGERPRINT` | yes |  |  |  |
| `--https-port` |  | `HTTPS_PORT` | no | `43101` |  |  |
| `--yes` |  | `YES` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

##### `atm peer trust list`

**Usage:**

```text
Usage: atm peer trust list [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

##### `atm peer trust migrate`

Migrate or retire legacy literal-IP trusted-peer entries.

Without `--yes`, prints the migration plan (one line per legacy literal-IP row: enabled/disabled, fingerprint, port, and the action) and makes no changes. With `--yes`, applies the plan: rows named by `--map IP=HOSTNAME` convert to the durable hostname, preserving their fingerprint and port; every other legacy literal-IP row is revoked.

**Usage:**

```text
Usage: atm peer trust migrate [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--map` |  | `IP=HOSTNAME` | no |  |  | Convert the legacy literal-IP entry `IP` to durable hostname `HOSTNAME`, keeping its fingerprint and port. May be repeated |
| `--yes` |  | `YES` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

##### `atm peer trust replace`

**Usage:**

```text
Usage: atm peer trust replace [OPTIONS] --host <HOST> --fingerprint <FINGERPRINT>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--host` |  | `HOST` | yes |  |  |  |
| `--fingerprint` |  | `FINGERPRINT` | yes |  |  |  |
| `--https-port` |  | `HTTPS_PORT` | no | `43101` |  |  |
| `--yes` |  | `YES` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

##### `atm peer trust revoke`

**Usage:**

```text
Usage: atm peer trust revoke [OPTIONS] --host <HOST>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--host` |  | `HOST` | yes |  |  |  |
| `--yes` |  | `YES` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm queue`

Queue one ATM mailbox message for deferred recipient notification

**Usage:**

```text
Usage: atm queue [OPTIONS] [TO] [MESSAGE]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<to>` |  | `TO` | no |  |  |  |
| `<message>` |  | `MESSAGE` | no |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--host` |  | `HOST` | no |  |  | Route this send through the explicitly named host |
| `--chat-id` |  | `CHAT_ID` | no |  |  |  |
| `--as` |  | `ACTOR` | no |  |  |  |
| `--file` |  | `FILE` | no |  |  |  |
| `--stdin` |  | `STDIN` | no | `false` |  |  |
| `--template` |  | `PATH` | no |  |  | Render and send a locally loaded template through the daemon-owned template admission path |
| `--vars` |  | `FILE|-` | no |  |  | JSON object providing template variables. `-` reads this object from stdin; it is distinct from `--stdin`, which is a plain message source |
| `--var` |  | `KEY=VALUE` | no |  |  | One template variable. May be repeated; values parse as JSON when possible and otherwise remain strings |
| `--env-prefix` |  | `PREFIX` | no |  |  | Capture current environment variables with this prefix at CLI composition time |
| `--attach` |  | `PATH` | no |  |  | Attach a local file for Send-To delivery (ADR-055). May be repeated. A same-host recipient's files are staged under `$ATM_TEMP/send-to/<id>/`; a remote recipient's files are routed through that host's configured transfer script (see `docs/cross-host-file-transfer.md`). The landed path rides in the message text; there is no envelope change. Mutually exclusive with `--template` (structured template content and free-form attachment notes are not composed in this phase) |
| `--from-json` |  | `FROM_JSON` | no | `false` |  | Read one `PickerOutput` JSON document (`{"schema_version":1,"recipients":[...],"note":"..."}`) from stdin and send one immutable message per recipient (ADR-055). Mutually exclusive with the positional recipient/message text, `--stdin`, `--file`, `--template`, and `--env-prefix` |
| `--category` |  | `CATEGORY` | no |  |  |  |
| `--tag` |  | `TAG` | no |  |  |  |
| `--content-format` |  | `FORMAT` | no |  |  |  |
| `--summary` |  | `SUMMARY` | no |  |  |  |
| `--requires-ack` |  | `REQUIRES_ACK` | no | `false` |  |  |
| `--task-id` |  | `TASK_ID` | no |  |  |  |
| `--task-complete` |  | `TASK_COMPLETE` | no | `false` |  | Close `--task-id` as completed after delivering this message as the report |
| `--dry-run` |  | `DRY_RUN` | no | `false` |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

**Notes:**

Path-only bodies are admitted for compatibility but recorded as content_format=path-ref and warned on stderr; use `atm send --template <path> --vars <file>` to send rendered content, and `atm compose --template <path>` to preview it. Post-send hooks can be configured in .atm.toml via one or more [[atm.post_send_hooks]] rules with recipient = "name-or-*" and command = ["argv", ...]. Matching rules run after a successful non-dry-run send, in config order. Path-like command[0] values resolve relative to the declaring .atm.toml; bare executables like bash or python3 use normal PATH resolution. Recipient non-match is silent. For hook troubleshooting, combine --stderr-logs with ATM_LOG=debug to surface debug-level hook diagnostics on stderr.

### `atm read`

Read one ATM mailbox message and optionally update read state

**Usage:**

```text
Usage: atm read [OPTIONS] [MESSAGE_ID]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<message_id_positional>` |  | `MESSAGE_ID` | no |  |  | Positional message IDs are rejected with a migration hint. Keeping this parser slot lets the CLI explain the supported option instead of returning clap's generic unexpected-argument error |
| `--team` |  | `TEAM` | no |  |  |  |
| `--chat-id` |  | `CHAT_ID` | no |  |  |  |
| `--as` |  | `ACTOR` | no |  |  |  |
| `--all` |  | `ALL` | no | `false` |  |  |
| `--unread` |  | `UNREAD` | no | `false` |  |  |
| `--unread-only` |  | `UNREAD_ONLY` | no | `false` |  |  |
| `--pending-ack` |  | `PENDING_ACK` | no | `false` |  |  |
| `--pending-ack-only` |  | `PENDING_ACK_ONLY` | no | `false` |  |  |
| `--history` |  | `HISTORY` | no | `false` |  |  |
| `--message-id` |  | `MESSAGE_ID` | no |  |  |  |
| `--task` |  | `TASK` | no |  |  |  |
| `--contains` |  | `CONTAINS` | no |  |  |  |
| `--since-last-seen` |  | `SINCE_LAST_SEEN` | no | `false` |  |  |
| `--no-since-last-seen` |  | `NO_SINCE_LAST_SEEN` | no | `false` |  |  |
| `--since` |  | `SINCE` | no |  |  |  |
| `--from` |  | `FROM` | no |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--timeout` |  | `TIMEOUT` | no |  |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm search`

Search locally indexed ATM messages through the daemon's typed query API

**Usage:**

```text
Usage: atm search [OPTIONS] [TEXT]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<text>` |  | `TEXT` | no |  |  | Literal phrase by default, or an ATM advanced expression with --raw-match |
| `--raw-match` |  | `RAW_MATCH` | no | `false` |  | Parse the positional text with ATM's documented bounded advanced grammar |
| `--template-meta` |  | `KEY=VALUE` | no |  |  | Filter stored template frontmatter metadata; a trailing * is a prefix match |
| `--type` |  | `VALUE` | no |  |  | Shorthand for --template-meta type=VALUE |
| `--template-sha` |  | `SHA` | no |  |  | Filter the exact immutable template revision |
| `--var` |  | `KEY=VALUE` | no |  |  | Filter one stored template variable; may be repeated |
| `--tag` |  | `VALUE` | no |  |  |  |
| `--effective-tag` |  | `VALUE` | no |  |  | Filter ATM's immutable effective-tag projection; may be repeated |
| `--category` |  | `CATEGORY` | no |  |  |  |
| `--from` |  | `FROM` | no |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--agent` |  | `AGENT` | no |  |  |  |
| `--workflow-scope-kind` |  | `VALUE` | no |  |  |  |
| `--workflow-scope-id` |  | `VALUE` | no |  |  |  |
| `--workflow-state` |  | `VALUE` | no |  |  |  |
| `--workflow-stage` |  | `VALUE` | no |  |  |  |
| `--workflow-transition` |  | `VALUE` | no |  |  |  |
| `--workflow-iteration` |  | `VALUE` | no |  |  |  |
| `--lifecycle-scope-kind` |  | `VALUE` | no |  |  | Project generic lifecycle observations over the local search result set |
| `--lifecycle-scope-id` |  | `VALUE` | no |  |  |  |
| `--lifecycle-start-state` |  | `VALUE` | no |  |  |  |
| `--lifecycle-start-stage` |  | `VALUE` | no |  |  |  |
| `--lifecycle-start-transition` |  | `VALUE` | no |  |  |  |
| `--lifecycle-end-state` |  | `VALUE` | no |  |  |  |
| `--lifecycle-end-stage` |  | `VALUE` | no |  |  |  |
| `--lifecycle-end-transition` |  | `VALUE` | no |  |  |  |
| `--since` |  | `SINCE` | no |  |  |  |
| `--until` |  | `UNTIL` | no |  |  |  |
| `--limit` |  | `LIMIT` | no |  |  |  |
| `--cursor` |  | `CURSOR` | no |  |  |  |
| `--per-mailbox` |  | `PER_MAILBOX` | no | `false` |  | Preserve per-mailbox compound-key identities rather than default deduplication |
| `--count` |  | `COUNT` | no | `false` |  |  |
| `--group-by` |  | `FIELD` | no |  |  |  |
| `--min` |  | `MIN` | no |  | `message_at` |  |
| `--max` |  | `MAX` | no |  | `message_at` |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

**Notes:**

Plain positional text is always a literal phrase. --raw-match enables ATM's bounded advanced grammar (words, quoted phrases, NEAR(term term[, distance]), AND, OR, NOT); it never passes raw SQLite FTS syntax.

### `atm send`

Send one ATM mailbox message

**Usage:**

```text
Usage: atm send [OPTIONS] [TO] [MESSAGE]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<to>` |  | `TO` | no |  |  |  |
| `<message>` |  | `MESSAGE` | no |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--host` |  | `HOST` | no |  |  | Route this send through the explicitly named host |
| `--chat-id` |  | `CHAT_ID` | no |  |  |  |
| `--as` |  | `ACTOR` | no |  |  |  |
| `--file` |  | `FILE` | no |  |  |  |
| `--stdin` |  | `STDIN` | no | `false` |  |  |
| `--template` |  | `PATH` | no |  |  | Render and send a locally loaded template through the daemon-owned template admission path |
| `--vars` |  | `FILE|-` | no |  |  | JSON object providing template variables. `-` reads this object from stdin; it is distinct from `--stdin`, which is a plain message source |
| `--var` |  | `KEY=VALUE` | no |  |  | One template variable. May be repeated; values parse as JSON when possible and otherwise remain strings |
| `--env-prefix` |  | `PREFIX` | no |  |  | Capture current environment variables with this prefix at CLI composition time |
| `--attach` |  | `PATH` | no |  |  | Attach a local file for Send-To delivery (ADR-055). May be repeated. A same-host recipient's files are staged under `$ATM_TEMP/send-to/<id>/`; a remote recipient's files are routed through that host's configured transfer script (see `docs/cross-host-file-transfer.md`). The landed path rides in the message text; there is no envelope change. Mutually exclusive with `--template` (structured template content and free-form attachment notes are not composed in this phase) |
| `--from-json` |  | `FROM_JSON` | no | `false` |  | Read one `PickerOutput` JSON document (`{"schema_version":1,"recipients":[...],"note":"..."}`) from stdin and send one immutable message per recipient (ADR-055). Mutually exclusive with the positional recipient/message text, `--stdin`, `--file`, `--template`, and `--env-prefix` |
| `--category` |  | `CATEGORY` | no |  |  |  |
| `--tag` |  | `TAG` | no |  |  |  |
| `--content-format` |  | `FORMAT` | no |  |  |  |
| `--summary` |  | `SUMMARY` | no |  |  |  |
| `--requires-ack` |  | `REQUIRES_ACK` | no | `false` |  |  |
| `--task-id` |  | `TASK_ID` | no |  |  |  |
| `--task-complete` |  | `TASK_COMPLETE` | no | `false` |  | Close `--task-id` as completed after delivering this message as the report |
| `--dry-run` |  | `DRY_RUN` | no | `false` |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

**Notes:**

Path-only bodies are admitted for compatibility but recorded as content_format=path-ref and warned on stderr; use `atm send --template <path> --vars <file>` to send rendered content, and `atm compose --template <path>` to preview it. Post-send hooks can be configured in .atm.toml via one or more [[atm.post_send_hooks]] rules with recipient = "name-or-*" and command = ["argv", ...]. Matching rules run after a successful non-dry-run send, in config order. Path-like command[0] values resolve relative to the declaring .atm.toml; bare executables like bash or python3 use normal PATH resolution. Recipient non-match is silent. For hook troubleshooting, combine --stderr-logs with ATM_LOG=debug to surface debug-level hook diagnostics on stderr.

### `atm task`

**Usage:**

```text
Usage: atm task [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm task assign`

Assign a task with deferred notification

**Usage:**

```text
Usage: atm task assign [OPTIONS] <ASSIGNEE> [MESSAGE]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<assignee>` |  | `ASSIGNEE` | yes |  |  |  |
| `--task-id` |  | `TASK_ID` | no |  |  |  |
| `--before` |  | `OTHER_TASK_ID` | no |  |  |  |
| `--head` |  | `HEAD` | no | `false` |  |  |
| `<text>` |  | `MESSAGE` | no |  |  |  |
| `--file` |  | `FILE` | no |  |  |  |
| `--stdin` |  | `STDIN` | no | `false` |  |  |
| `--template` |  | `TEMPLATE` | no |  |  |  |
| `--vars` |  | `VARS` | no |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--as` |  | `ACTOR` | no |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm task close`

Deliver a report and close a task with a typed outcome

**Usage:**

```text
Usage: atm task close [OPTIONS] <TASK_ID> <OUTCOME> [REASON] [MESSAGE]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<task_id>` |  | `TASK_ID` | yes |  |  |  |
| `<outcome>` |  | `OUTCOME` | yes |  | `completed`, `refused`, `cancelled` |  |
| `<reason>` |  | `REASON` | no |  |  | Recorded on the close event; also the report body when no source is given |
| `<text>` |  | `MESSAGE` | no |  |  |  |
| `--file` |  | `FILE` | no |  |  |  |
| `--stdin` |  | `STDIN` | no | `false` |  |  |
| `--template` |  | `TEMPLATE` | no |  |  |  |
| `--vars` |  | `VARS` | no |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--as` |  | `ACTOR` | no |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm task events`

Append-only event history of one task

**Usage:**

```text
Usage: atm task events [OPTIONS] <TASK_ID>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<task_id>` |  | `TASK_ID` | yes |  |  |  |
| `--limit` |  | `N` | no |  |  |  |
| `--all` |  | `ALL` | no | `false` |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--as` |  | `ACTOR` | no |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm task history`

Past N tasks for the team, open and completed, newest first

**Usage:**

```text
Usage: atm task history [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--member` |  | `MEMBER` | no |  |  |  |
| `--limit` |  | `N` | no |  |  |  |
| `--events` |  | `EVENTS` | no | `false` |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--as` |  | `ACTOR` | no |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm task list`

Queue view: the caller's open tasks; --all includes every member

**Usage:**

```text
Usage: atm task list [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--all` |  | `ALL` | no | `false` |  |  |
| `--limit` |  | `N` | no |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--as` |  | `ACTOR` | no |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm task move`

Reorder one member's queue

**Usage:**

```text
Usage: atm task move [OPTIONS] <--head|--end|--before <OTHER_TASK_ID>> <TASK_ID>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<task_id>` |  | `TASK_ID` | yes |  |  |  |
| `--head` |  | `HEAD` | no | `false` |  |  |
| `--end` |  | `END` | no | `false` |  |  |
| `--before` |  | `OTHER_TASK_ID` | no |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--as` |  | `ACTOR` | no |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm task start`

Start an assigned task: moves it to active and tells the assigner

**Usage:**

```text
Usage: atm task start [OPTIONS] <TASK_ID> [MESSAGE]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<task_id>` |  | `TASK_ID` | yes |  |  |  |
| `<text>` |  | `MESSAGE` | no |  |  |  |
| `--file` |  | `FILE` | no |  |  |  |
| `--stdin` |  | `STDIN` | no | `false` |  |  |
| `--template` |  | `TEMPLATE` | no |  |  |  |
| `--vars` |  | `VARS` | no |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--as` |  | `ACTOR` | no |  |  |  |
| `--team` |  | `TEAM` | no |  |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm teams`

List teams or run one team-administration subcommand

**Usage:**

```text
Usage: atm teams [OPTIONS] [COMMAND]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--json` |  | `JSON` | no | `false` |  |  |
| `--members` |  | `MEMBERS` | no | `false` |  | Emit the picker member projection (ADR-055 decision (e), PRD §4.2/§5a) instead of the team-count list: per-member `{"id","name", "host","cwd","status"}`, consumed by `atm send --from-json`'s `recipients`. Only valid without a subcommand |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm teams add-member`

**Usage:**

```text
Usage: atm teams add-member [OPTIONS] <TEAM> <MEMBER>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<team>` |  | `TEAM` | yes |  |  |  |
| `<member>` |  | `MEMBER` | yes |  |  |  |
| `--agent-type` |  | `AGENT_TYPE` | no | `general-purpose` |  |  |
| `--model` |  | `MODEL` | no | `unknown` |  |  |
| `--home-dir` |  | `HOME_DIR` | no |  |  |  |
| `--backend` |  | `BACKEND` | no |  |  | local receiver backend: tmux or herdr |
| `--target` |  | `TARGET` | no |  |  | tmux pane target; required for --backend tmux |
| `--session` |  | `SESSION` | no |  |  | Herdr session name; only valid with --backend herdr |
| `--alias` |  | `ALIAS` | no |  |  | durable roster alias; Herdr members use it as their live-agent target |
| `--pane-id` |  | `PANE_ID` | no |  |  | deprecated compatibility spelling for --backend tmux --target |
| `--host` |  | `HOST` | no |  |  | this member's registered host (ADR-055 decision (e)); used for Send-To same-host/remote routing, never inferred |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm teams backup`

**Usage:**

```text
Usage: atm teams backup [OPTIONS] <TEAM>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<team>` |  | `TEAM` | yes |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm teams clear-nudge-template`

**Usage:**

```text
Usage: atm teams clear-nudge-template [OPTIONS] --team <TEAM> --kind <KIND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--team` |  | `TEAM` | yes |  |  |  |
| `--kind` |  | `KIND` | yes |  |  | template kind: delivery, delivery_ack, queue, queue_ack, acknowledge, task_queued, task_ready, task_reminder, task_started, task_complete, task_closed; retired task and acknowledge_task are accepted for deletion |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm teams disable-nudge-template`

**Usage:**

```text
Usage: atm teams disable-nudge-template [OPTIONS] --team <TEAM> --kind <KIND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--team` |  | `TEAM` | yes |  |  |  |
| `--kind` |  | `KIND` | yes |  |  | template kind: delivery, delivery_ack, queue, queue_ack, acknowledge, task_queued, task_ready, task_reminder, task_started, task_complete, task_closed |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm teams remove-member`

**Usage:**

```text
Usage: atm teams remove-member [OPTIONS] <TEAM> <MEMBER>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<team>` |  | `TEAM` | yes |  |  |  |
| `<member>` |  | `MEMBER` | yes |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm teams restore`

**Usage:**

```text
Usage: atm teams restore [OPTIONS] <TEAM>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<team>` |  | `TEAM` | yes |  |  |  |
| `--from` |  | `FROM` | no |  |  |  |
| `--dry-run` |  | `DRY_RUN` | no | `false` |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm teams set-nudge-template`

**Usage:**

```text
Usage: atm teams set-nudge-template [OPTIONS] --team <TEAM> --kind <KIND> --template-body <TEMPLATE_BODY>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--team` |  | `TEAM` | yes |  |  |  |
| `--kind` |  | `KIND` | yes |  |  | template kind: delivery, delivery_ack, queue, queue_ack, acknowledge, task_queued, task_ready, task_reminder, task_started, task_complete, task_closed |
| `--template-body` |  | `TEMPLATE_BODY` | yes |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm teams update-member`

**Usage:**

```text
Usage: atm teams update-member [OPTIONS] <TEAM> <MEMBER>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<team>` |  | `TEAM` | yes |  |  |  |
| `<member>` |  | `MEMBER` | yes |  |  |  |
| `--home-dir` |  | `HOME_DIR` | no |  |  |  |
| `--workspace-root` |  | `WORKSPACE_ROOT` | no |  |  |  |
| `--harness` |  | `HARNESS` | no |  |  |  |
| `--agent-type` |  | `AGENT_TYPE` | no |  |  |  |
| `--model` |  | `MODEL` | no |  |  |  |
| `--backend` |  | `BACKEND` | no |  |  | local receiver backend: tmux or herdr |
| `--target` |  | `TARGET` | no |  |  | tmux pane target; required for --backend tmux |
| `--session` |  | `SESSION` | no |  |  | Herdr session name; only valid with --backend herdr |
| `--alias` |  | `ALIAS` | no |  |  | durable roster alias; Herdr members use it as their live-agent target |
| `--clear-alias` |  | `CLEAR_ALIAS` | no | `false` |  | remove the member's durable roster alias |
| `--pane-id` |  | `PANE_ID` | no |  |  | deprecated compatibility spelling for --backend tmux --target |
| `--host` |  | `HOST` | no |  |  | this member's registered host (ADR-055 decision (e)); used for Send-To same-host/remote routing, never inferred |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

### `atm templates`

Inspect immutable templates registered by decomposed-message admission

**Usage:**

```text
Usage: atm templates [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm templates list`

List every known immutable template revision, optionally by metadata type

**Usage:**

```text
Usage: atm templates list [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--type` |  | `TEMPLATE_TYPE` | no |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |

#### `atm templates schema`

Show the stored schema/frontmatter for one exact immutable SHA

**Usage:**

```text
Usage: atm templates schema [OPTIONS] <SHA>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<sha>` |  | `SHA` | yes |  |  |  |
| `--json` |  | `JSON` | no | `false` |  |  |
| `--stderr-logs` |  | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr |


