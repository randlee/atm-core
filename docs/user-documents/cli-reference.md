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
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm ack`

Acknowledge one pending-ack message and emit a reply when required

**Usage:**

```text
Usage: atm ack [OPTIONS] <MESSAGE_ID> <REPLY>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<message_id>` | `` | `MESSAGE_ID` | yes | `` |  | ID of the pending-ack message to acknowledge |
| `<reply>` | `` | `REPLY` | yes | `` |  | Reply body recorded with the acknowledgement |
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm api`

**Usage:**

```text
Usage: atm api [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm api spec`

Print the versioned daemon OpenAPI contract

**Usage:**

```text
Usage: atm api spec [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--format` | `` | `FORMAT` | no | `yaml` | json, yaml | Serialization format for the OpenAPI contract |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm clear`

Clear read or acknowledged messages from a mailbox

**Usage:**

```text
Usage: atm clear [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--older-than` | `` | `DURATION` | no | `` |  | Clear messages older than this duration (`s`, `m`, `h`, or `d`) |
| `--idle-only` | `` | `IDLE_ONLY` | no | `false` |  | Clear only idle mailbox messages |
| `--dry-run` | `` | `DRY_RUN` | no | `false` |  | Show the messages that would be cleared without clearing them |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm compose`

Render a template through the core renderer port and print the exact body

**Usage:**

```text
Usage: atm compose [OPTIONS] --template <PATH>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--template` | `` | `PATH` | yes | `` |  | Template file to validate and render |
| `--vars` | `` | `FILE\|-` | no | `` |  | JSON object providing template variables; `-` reads stdin |
| `--var` | `` | `KEY=VALUE` | no | `` |  | One template variable; may be repeated |
| `--env-prefix` | `` | `PREFIX` | no | `` |  | Capture environment variables with this prefix |
| `--dry-run` | `` | `DRY_RUN` | no | `false` |  | Validate and render without any side effects (the default operation is already side-effect free; the flag makes scripts self-documenting) |
| `--json` | `` | `JSON` | no | `false` |  | Emit a structured result instead of the byte-identical rendered body |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

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
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--all-teams` | `` | `ALL_TEAMS` | no | `false` |  | Inspect every team in the canonical roster. |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm escalation`

Manage daemon-wide and per-team escalation recipients

**Usage:**

```text
Usage: atm escalation [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm escalation add`

**Usage:**

```text
Usage: atm escalation add [OPTIONS] <ADDRESS>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<address>` | `` | `ADDRESS` | yes | `` |  | Recipient address to add or remove |
| `--team` | `` | `TEAM` | no | `` |  | Limit the change to this team; omit for daemon-wide recipients |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm escalation list`

**Usage:**

```text
Usage: atm escalation list [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--team` | `` | `TEAM` | no | `` |  | Limit the listing to this team; omit for daemon-wide recipients |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm escalation remove`

**Usage:**

```text
Usage: atm escalation remove [OPTIONS] <ADDRESS>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<address>` | `` | `ADDRESS` | yes | `` |  | Recipient address to add or remove |
| `--team` | `` | `TEAM` | no | `` |  | Limit the change to this team; omit for daemon-wide recipients |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm help`

Show ATM-owned conceptual help or delegated clap subcommand help

**Usage:**

```text
Usage: atm help [OPTIONS] [TARGET]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<target>` | `` | `TARGET` | no | `` |  | Conceptual-help topic or clap subcommand path to show |
| `--list` | `` | `LIST` | no | `false` |  | List available conceptual-help topics |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm list`

List one ATM mailbox surface as bounded metadata rows

**Usage:**

```text
Usage: atm list [OPTIONS] [TARGET]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<target>` | `` | `TARGET` | no | `` |  | Mailbox target address to inspect; defaults to the caller |
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--all` | `` | `ALL` | no | `false` |  | Include all messages instead of a filtered state |
| `--unread` | `` | `UNREAD` | no | `false` |  | Include unread messages |
| `--pending-ack` | `` | `PENDING_ACK` | no | `false` |  | Include messages awaiting acknowledgement |
| `--limit` | `` | `LIMIT` | no | `` |  | Maximum number of messages to return |
| `--since` | `` | `SINCE` | no | `` |  | Include messages at or after this timestamp |
| `--from` | `` | `FROM` | no | `` |  | Include messages sent by this address |
| `--task` | `` | `TASK` | no | `` |  | Include messages linked to this task ID |
| `--contains` | `` | `CONTAINS` | no | `` |  | Include messages whose body contains this text |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--as` | `` | `ACTOR` | no | `` |  | Act as this member instead of the environment identity |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm log`

Query or follow ATM retained observability records

**Usage:**

```text
Usage: atm log [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm log filter`

Query ATM log records using explicit field filters

**Usage:**

```text
Usage: atm log filter [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--source` | `` | `SOURCE` | no | `jsonl` | jsonl, timeline, merged | Select the retained-log source. JSONL is the byte-compatible default |
| `--level` | `` | `LEVELS` | no | `` |  | Restrict results to one or more severity levels |
| `--match` | `` | `KEY=VALUE` | no | `` |  | Match one structured ATM field exactly, for example command=send |
| `--since` | `` | `SINCE` | no | `` |  | Inclusive lower time bound as RFC3339 or a relative duration like 15m |
| `--until` | `` | `UNTIL` | no | `` |  | Inclusive upper time bound as RFC3339 or a relative duration |
| `--component` | `` | `COMPONENT` | no | `` |  | Restrict timeline records to a component prefix |
| `--limit` | `` | `LIMIT` | no | `` |  | Maximum number of returned records |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm log snapshot`

Query recent ATM log records

**Usage:**

```text
Usage: atm log snapshot [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--source` | `` | `SOURCE` | no | `jsonl` | jsonl, timeline, merged | Select the retained-log source. JSONL is the byte-compatible default |
| `--level` | `` | `LEVELS` | no | `` |  | Restrict results to one or more severity levels |
| `--match` | `` | `KEY=VALUE` | no | `` |  | Match one structured ATM field exactly, for example command=send |
| `--since` | `` | `SINCE` | no | `` |  | Inclusive lower time bound as RFC3339 or a relative duration like 15m |
| `--until` | `` | `UNTIL` | no | `` |  | Inclusive upper time bound as RFC3339 or a relative duration |
| `--component` | `` | `COMPONENT` | no | `` |  | Restrict timeline records to a component prefix |
| `--limit` | `` | `LIMIT` | no | `` |  | Maximum number of returned records |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm log tail`

Follow new ATM log records as they arrive

**Usage:**

```text
Usage: atm log tail [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--source` | `` | `SOURCE` | no | `jsonl` | jsonl, timeline, merged | Select the retained-log source. JSONL is the byte-compatible default |
| `--level` | `` | `LEVELS` | no | `` |  | Restrict results to one or more severity levels |
| `--match` | `` | `KEY=VALUE` | no | `` |  | Match one structured ATM field exactly, for example command=send |
| `--since` | `` | `SINCE` | no | `` |  | Inclusive lower time bound as RFC3339 or a relative duration like 15m |
| `--until` | `` | `UNTIL` | no | `` |  | Inclusive upper time bound as RFC3339 or a relative duration |
| `--component` | `` | `COMPONENT` | no | `` |  | Restrict timeline records to a component prefix |
| `--limit` | `` | `LIMIT` | no | `` |  | Maximum number of returned records |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--poll-interval-ms` | `` | `POLL_INTERVAL_MS` | no | `250` |  | Poll interval in milliseconds between follow polls |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm members`

List the current member roster for one ATM team

**Usage:**

```text
Usage: atm members [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm peek`

Inspect one ATM mailbox message without mutating mailbox state

**Usage:**

```text
Usage: atm peek [OPTIONS] [TARGET]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<target>` | `` | `TARGET` | no | `` |  | Mailbox target address to inspect; defaults to the caller |
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--all` | `` | `ALL` | no | `false` |  | Inspect all matching messages |
| `--unread` | `` | `UNREAD` | no | `false` |  | Inspect unread matching messages |
| `--unread-only` | `` | `UNREAD_ONLY` | no | `false` |  | Deprecated spelling for `--unread` |
| `--pending-ack` | `` | `PENDING_ACK` | no | `false` |  | Inspect messages awaiting acknowledgement |
| `--pending-ack-only` | `` | `PENDING_ACK_ONLY` | no | `false` |  | Deprecated spelling for `--pending-ack` |
| `--history` | `` | `HISTORY` | no | `false` |  | Deprecated spelling for `--all` |
| `--message-id` | `` | `MESSAGE_ID` | no | `` |  | Inspect one message by its ID |
| `--task` | `` | `TASK` | no | `` |  | Restrict messages to this task ID |
| `--contains` | `` | `CONTAINS` | no | `` |  | Restrict messages to bodies containing this text |
| `--since-last-seen` | `` | `SINCE_LAST_SEEN` | no | `false` |  | Explicitly retain the default seen-state filter |
| `--no-since-last-seen` | `` | `NO_SINCE_LAST_SEEN` | no | `false` |  | Disable the default seen-state filter |
| `--since` | `` | `SINCE` | no | `` |  | Restrict messages to those at or after this timestamp |
| `--from` | `` | `FROM` | no | `` |  | Restrict messages to this sender address |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--timeout` | `` | `TIMEOUT` | no | `` |  | Wait up to this many seconds for matching messages |
| `--as` | `` | `ACTOR` | no | `` |  | Act as this member instead of the environment identity |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm peer`

Manage durable cross-host HTTPS control-plane configuration

**Usage:**

```text
Usage: atm peer [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm peer certificate`

**Usage:**

```text
Usage: atm peer certificate [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

##### `atm peer certificate init`

**Usage:**

```text
Usage: atm peer certificate init [OPTIONS] --fingerprint <FINGERPRINT> --private-key-ref <PRIVATE_KEY_REF>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--fingerprint` | `` | `FINGERPRINT` | yes | `` |  | TLS certificate fingerprint for this host |
| `--private-key-ref` | `` | `PRIVATE_KEY_REF` | yes | `` |  | Reference to the local TLS private key; never the key material |
| `--yes` | `` | `YES` | no | `false` |  | Confirm the certificate initialization |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

##### `atm peer certificate show`

**Usage:**

```text
Usage: atm peer certificate show [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm peer interface`

**Usage:**

```text
Usage: atm peer interface [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

##### `atm peer interface list`

**Usage:**

```text
Usage: atm peer interface list [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

##### `atm peer interface remove`

**Usage:**

```text
Usage: atm peer interface remove [OPTIONS] --bind <BIND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--bind` | `` | `BIND` | yes | `` |  | Socket address of the HTTPS interface to remove |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

##### `atm peer interface set`

**Usage:**

```text
Usage: atm peer interface set [OPTIONS] --bind <BIND> --advertise-host <ADVERTISE_HOST>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--bind` | `` | `BIND` | yes | `` |  | Socket address on which the HTTPS interface listens |
| `--advertise-host` | `` | `ADVERTISE_HOST` | yes | `` |  | Durable hostname advertised for this interface |
| `--enabled` | `` | `ENABLED` | no | `true` |  | Enable or disable this HTTPS interface |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm peer trust`

**Usage:**

```text
Usage: atm peer trust [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

##### `atm peer trust add`

**Usage:**

```text
Usage: atm peer trust add [OPTIONS] --host <HOST> --fingerprint <FINGERPRINT>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--host` | `` | `HOST` | yes | `` |  | Durable hostname of the trusted peer |
| `--fingerprint` | `` | `FINGERPRINT` | yes | `` |  | TLS certificate fingerprint expected from the peer |
| `--https-port` | `` | `HTTPS_PORT` | no | `43101` |  | HTTPS port exposed by the trusted peer |
| `--yes` | `` | `YES` | no | `false` |  | Confirm adding the trusted peer |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

##### `atm peer trust list`

**Usage:**

```text
Usage: atm peer trust list [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

##### `atm peer trust migrate`

Migrate or retire legacy literal-IP trusted-peer entries

**Usage:**

```text
Usage: atm peer trust migrate [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--map` | `` | `IP=HOSTNAME` | no | `` |  | Convert the legacy literal-IP entry `IP` to durable hostname `HOSTNAME`, keeping its fingerprint and port. May be repeated |
| `--yes` | `` | `YES` | no | `false` |  | Apply the displayed migration plan |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

##### `atm peer trust replace`

**Usage:**

```text
Usage: atm peer trust replace [OPTIONS] --host <HOST> --fingerprint <FINGERPRINT>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--host` | `` | `HOST` | yes | `` |  | Durable hostname of the trusted peer |
| `--fingerprint` | `` | `FINGERPRINT` | yes | `` |  | Replacement TLS certificate fingerprint |
| `--https-port` | `` | `HTTPS_PORT` | no | `43101` |  | HTTPS port exposed by the trusted peer |
| `--yes` | `` | `YES` | no | `false` |  | Confirm replacing the trusted-peer configuration |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

##### `atm peer trust revoke`

**Usage:**

```text
Usage: atm peer trust revoke [OPTIONS] --host <HOST>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--host` | `` | `HOST` | yes | `` |  | Durable hostname of the trusted peer to revoke |
| `--yes` | `` | `YES` | no | `false` |  | Confirm revoking the trusted peer |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm queue`

Queue one ATM mailbox message for deferred recipient notification

**Usage:**

```text
Usage: atm queue [OPTIONS] [TO] [MESSAGE]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<to>` | `` | `TO` | no | `` |  | Recipient address for the message |
| `<message>` | `` | `MESSAGE` | no | `` |  | Message body to send |
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--host` | `` | `HOST` | no | `` |  | Route this send through the explicitly named host.  Supplying this flag allows a same-identity send to be an intentional physical delivery test (for example, `--host localhost` or a same-host IP). This is wire-equivalent to a host-qualified recipient address. When both forms are supplied, they must name the same host. |
| `--chat-id` | `` | `CHAT_ID` | no | `` |  | Restrict the send to this caller chat ID |
| `--as` | `` | `ACTOR` | no | `` |  | Act as this member instead of the environment identity |
| `--file` | `` | `FILE` | no | `` |  | Read the message body from this file |
| `--stdin` | `` | `STDIN` | no | `false` |  | Read the message body from standard input |
| `--template` | `` | `PATH` | no | `` |  | Render and send a locally loaded template through the daemon-owned template admission path |
| `--vars` | `` | `FILE\|-` | no | `` |  | JSON object providing template variables. `-` reads this object from stdin; it is distinct from `--stdin`, which is a plain message source |
| `--var` | `` | `KEY=VALUE` | no | `` |  | One template variable. May be repeated; values parse as JSON when possible and otherwise remain strings |
| `--env-prefix` | `` | `PREFIX` | no | `` |  | Capture current environment variables with this prefix at CLI composition time |
| `--attach` | `` | `PATH` | no | `` |  | Attach a local file for Send-To delivery (ADR-055). May be repeated. A same-host recipient's files are staged under `$ATM_TEMP/send-to/<id>/`; a remote recipient's files are routed through that host's configured transfer script (see `docs/cross-host-file-transfer.md`). The landed path rides in the message text; there is no envelope change. Mutually exclusive with `--template` (structured template content and free-form attachment notes are not composed in this phase) |
| `--from-json` | `` | `FROM_JSON` | no | `false` |  | Read one `PickerOutput` JSON document (`{"schema_version":1,"recipients":[...],"note":"..."}`) from stdin and send one immutable message per recipient (ADR-055). Mutually exclusive with the positional recipient/message text, `--stdin`, `--file`, `--template`, and `--env-prefix` |
| `--category` | `` | `CATEGORY` | no | `` |  | Classify the message under this category |
| `--tag` | `` | `TAG` | no | `` |  | Add a tag to the message; may be repeated |
| `--content-format` | `` | `FORMAT` | no | `` |  | Record this content format with the message |
| `--summary` | `` | `SUMMARY` | no | `` |  | Store this summary with the message |
| `--requires-ack` | `` | `REQUIRES_ACK` | no | `false` |  | Require the recipient to acknowledge the message |
| `--task-id` | `` | `TASK_ID` | no | `` |  | Link this message to the task with this ID |
| `--task-complete` | `` | `TASK_COMPLETE` | no | `false` |  | Close `--task-id` as completed after delivering this message as the report |
| `--dry-run` | `` | `DRY_RUN` | no | `false` |  | Validate and display the send without delivering it |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

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
| `<message_id_positional>` | `` | `MESSAGE_ID` | no | `` |  | Positional message IDs are rejected with a migration hint. Keeping this parser slot lets the CLI explain the supported option instead of returning clap's generic unexpected-argument error |
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--chat-id` | `` | `CHAT_ID` | no | `` |  | Restrict reads to this caller chat ID |
| `--as` | `` | `ACTOR` | no | `` |  | Act as this member instead of the environment identity |
| `--all` | `` | `ALL` | no | `false` |  | Read all matching messages |
| `--unread` | `` | `UNREAD` | no | `false` |  | Read unread matching messages |
| `--unread-only` | `` | `UNREAD_ONLY` | no | `false` |  | Deprecated spelling for `--unread` |
| `--pending-ack` | `` | `PENDING_ACK` | no | `false` |  | Read messages awaiting acknowledgement |
| `--pending-ack-only` | `` | `PENDING_ACK_ONLY` | no | `false` |  | Deprecated spelling for `--pending-ack` |
| `--history` | `` | `HISTORY` | no | `false` |  | Deprecated spelling for `--all` |
| `--message-id` | `` | `MESSAGE_ID` | no | `` |  | Read one message by its ID |
| `--task` | `` | `TASK` | no | `` |  | Restrict messages to this task ID |
| `--contains` | `` | `CONTAINS` | no | `` |  | Restrict messages to bodies containing this text |
| `--since-last-seen` | `` | `SINCE_LAST_SEEN` | no | `false` |  | Explicitly retain the default seen-state filter |
| `--no-since-last-seen` | `` | `NO_SINCE_LAST_SEEN` | no | `false` |  | Disable the default seen-state filter |
| `--since` | `` | `SINCE` | no | `` |  | Restrict messages to those at or after this timestamp |
| `--from` | `` | `FROM` | no | `` |  | Restrict messages to this sender address |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--timeout` | `` | `TIMEOUT` | no | `` |  | Wait up to this many seconds for matching messages |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm search`

Search locally indexed ATM messages through the daemon's typed query API

**Usage:**

```text
Usage: atm search [OPTIONS] [TEXT]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<text>` | `` | `TEXT` | no | `` |  | Literal phrase by default, or an ATM advanced expression with --raw-match |
| `--raw-match` | `` | `RAW_MATCH` | no | `false` |  | Parse the positional text with ATM's documented bounded advanced grammar |
| `--template-meta` | `` | `KEY=VALUE` | no | `` |  | Filter stored template frontmatter metadata; a trailing * is a prefix match |
| `--type` | `` | `VALUE` | no | `` |  | Shorthand for --template-meta type=VALUE |
| `--template-sha` | `` | `SHA` | no | `` |  | Filter the exact immutable template revision |
| `--var` | `` | `KEY=VALUE` | no | `` |  | Filter one stored template variable; may be repeated |
| `--tag` | `` | `VALUE` | no | `` |  | Filter messages by a stored tag; may be repeated |
| `--effective-tag` | `` | `VALUE` | no | `` |  | Filter ATM's immutable effective-tag projection; may be repeated |
| `--category` | `` | `CATEGORY` | no | `` |  | Filter messages by their category |
| `--from` | `` | `FROM` | no | `` |  | Filter messages by sender address |
| `--team` | `` | `TEAM` | no | `` |  | Filter messages by team |
| `--agent` | `` | `AGENT` | no | `` |  | Filter messages by agent identity |
| `--workflow-scope-kind` | `` | `VALUE` | no | `` |  | Filter workflow projections by scope kind |
| `--workflow-scope-id` | `` | `VALUE` | no | `` |  | Filter workflow projections by scope ID |
| `--workflow-state` | `` | `VALUE` | no | `` |  | Filter workflow projections by state |
| `--workflow-stage` | `` | `VALUE` | no | `` |  | Filter workflow projections by stage |
| `--workflow-transition` | `` | `VALUE` | no | `` |  | Filter workflow projections by transition |
| `--workflow-iteration` | `` | `VALUE` | no | `` |  | Filter workflow projections by iteration |
| `--lifecycle-scope-kind` | `` | `VALUE` | no | `` |  | Project generic lifecycle observations over the local search result set |
| `--lifecycle-scope-id` | `` | `VALUE` | no | `` |  | Lifecycle projection scope ID |
| `--lifecycle-start-state` | `` | `VALUE` | no | `` |  | Filter lifecycle projection start events by state |
| `--lifecycle-start-stage` | `` | `VALUE` | no | `` |  | Filter lifecycle projection start events by stage |
| `--lifecycle-start-transition` | `` | `VALUE` | no | `` |  | Filter lifecycle projection start events by transition |
| `--lifecycle-end-state` | `` | `VALUE` | no | `` |  | Filter lifecycle projection end events by state |
| `--lifecycle-end-stage` | `` | `VALUE` | no | `` |  | Filter lifecycle projection end events by stage |
| `--lifecycle-end-transition` | `` | `VALUE` | no | `` |  | Filter lifecycle projection end events by transition |
| `--since` | `` | `SINCE` | no | `` |  | Include messages at or after this timestamp |
| `--until` | `` | `UNTIL` | no | `` |  | Include messages at or before this timestamp |
| `--limit` | `` | `LIMIT` | no | `` |  | Maximum number of search results to return |
| `--cursor` | `` | `CURSOR` | no | `` |  | Continue from this search-results cursor |
| `--per-mailbox` | `` | `PER_MAILBOX` | no | `false` |  | Preserve per-mailbox compound-key identities rather than default deduplication |
| `--count` | `` | `COUNT` | no | `false` |  | Return only the number of matching messages |
| `--group-by` | `` | `FIELD` | no | `` |  | Group matching messages by this field |
| `--min` | `` | `MIN` | no | `` | message_at | Return the earliest matching message timestamp |
| `--max` | `` | `MAX` | no | `` | message_at | Return the latest matching message timestamp |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

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
| `<to>` | `` | `TO` | no | `` |  | Recipient address for the message |
| `<message>` | `` | `MESSAGE` | no | `` |  | Message body to send |
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--host` | `` | `HOST` | no | `` |  | Route this send through the explicitly named host.  Supplying this flag allows a same-identity send to be an intentional physical delivery test (for example, `--host localhost` or a same-host IP). This is wire-equivalent to a host-qualified recipient address. When both forms are supplied, they must name the same host. |
| `--chat-id` | `` | `CHAT_ID` | no | `` |  | Restrict the send to this caller chat ID |
| `--as` | `` | `ACTOR` | no | `` |  | Act as this member instead of the environment identity |
| `--file` | `` | `FILE` | no | `` |  | Read the message body from this file |
| `--stdin` | `` | `STDIN` | no | `false` |  | Read the message body from standard input |
| `--template` | `` | `PATH` | no | `` |  | Render and send a locally loaded template through the daemon-owned template admission path |
| `--vars` | `` | `FILE\|-` | no | `` |  | JSON object providing template variables. `-` reads this object from stdin; it is distinct from `--stdin`, which is a plain message source |
| `--var` | `` | `KEY=VALUE` | no | `` |  | One template variable. May be repeated; values parse as JSON when possible and otherwise remain strings |
| `--env-prefix` | `` | `PREFIX` | no | `` |  | Capture current environment variables with this prefix at CLI composition time |
| `--attach` | `` | `PATH` | no | `` |  | Attach a local file for Send-To delivery (ADR-055). May be repeated. A same-host recipient's files are staged under `$ATM_TEMP/send-to/<id>/`; a remote recipient's files are routed through that host's configured transfer script (see `docs/cross-host-file-transfer.md`). The landed path rides in the message text; there is no envelope change. Mutually exclusive with `--template` (structured template content and free-form attachment notes are not composed in this phase) |
| `--from-json` | `` | `FROM_JSON` | no | `false` |  | Read one `PickerOutput` JSON document (`{"schema_version":1,"recipients":[...],"note":"..."}`) from stdin and send one immutable message per recipient (ADR-055). Mutually exclusive with the positional recipient/message text, `--stdin`, `--file`, `--template`, and `--env-prefix` |
| `--category` | `` | `CATEGORY` | no | `` |  | Classify the message under this category |
| `--tag` | `` | `TAG` | no | `` |  | Add a tag to the message; may be repeated |
| `--content-format` | `` | `FORMAT` | no | `` |  | Record this content format with the message |
| `--summary` | `` | `SUMMARY` | no | `` |  | Store this summary with the message |
| `--requires-ack` | `` | `REQUIRES_ACK` | no | `false` |  | Require the recipient to acknowledge the message |
| `--task-id` | `` | `TASK_ID` | no | `` |  | Link this message to the task with this ID |
| `--task-complete` | `` | `TASK_COMPLETE` | no | `false` |  | Close `--task-id` as completed after delivering this message as the report |
| `--dry-run` | `` | `DRY_RUN` | no | `false` |  | Validate and display the send without delivering it |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

**Notes:**

Path-only bodies are admitted for compatibility but recorded as content_format=path-ref and warned on stderr; use `atm send --template <path> --vars <file>` to send rendered content, and `atm compose --template <path>` to preview it. Post-send hooks can be configured in .atm.toml via one or more [[atm.post_send_hooks]] rules with recipient = "name-or-*" and command = ["argv", ...]. Matching rules run after a successful non-dry-run send, in config order. Path-like command[0] values resolve relative to the declaring .atm.toml; bare executables like bash or python3 use normal PATH resolution. Recipient non-match is silent. For hook troubleshooting, combine --stderr-logs with ATM_LOG=debug to surface debug-level hook diagnostics on stderr.

### `atm task`

**Usage:**

```text
Usage: atm task [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm task assign`

Assign a task with deferred notification

**Usage:**

```text
Usage: atm task assign [OPTIONS] <ASSIGNEE> [MESSAGE]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<assignee>` | `` | `ASSIGNEE` | yes | `` |  | Member address to assign the task to |
| `--task-id` | `` | `TASK_ID` | no | `` |  | Explicit ID for the new task |
| `--before` | `` | `OTHER_TASK_ID` | no | `` |  | Place the task before this task in the assignee's queue |
| `--head` | `` | `HEAD` | no | `false` |  | Place the task at the head of the assignee's queue |
| `<text>` | `` | `MESSAGE` | no | `` |  | Message or report body |
| `--file` | `` | `FILE` | no | `` |  | Read the message or report body from this file |
| `--stdin` | `` | `STDIN` | no | `false` |  | Read the message or report body from standard input |
| `--template` | `` | `TEMPLATE` | no | `` |  | Render the message or report body from this template file |
| `--vars` | `` | `VARS` | no | `` |  | JSON object providing variables for `--template`; `-` reads stdin |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--as` | `` | `ACTOR` | no | `` |  | Act as this member instead of the environment identity |
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm task close`

Deliver a report and close a task with a typed outcome

**Usage:**

```text
Usage: atm task close [OPTIONS] <TASK_ID> <OUTCOME> [REASON] [MESSAGE]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<task_id>` | `` | `TASK_ID` | yes | `` |  | ID of the task to close |
| `<outcome>` | `` | `OUTCOME` | yes | `` | completed, refused, cancelled | Terminal task outcome to record |
| `<reason>` | `` | `REASON` | no | `` |  | Recorded on the close event; also the report body when no source is given |
| `<text>` | `` | `MESSAGE` | no | `` |  | Message or report body |
| `--file` | `` | `FILE` | no | `` |  | Read the message or report body from this file |
| `--stdin` | `` | `STDIN` | no | `false` |  | Read the message or report body from standard input |
| `--template` | `` | `TEMPLATE` | no | `` |  | Render the message or report body from this template file |
| `--vars` | `` | `VARS` | no | `` |  | JSON object providing variables for `--template`; `-` reads stdin |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--as` | `` | `ACTOR` | no | `` |  | Act as this member instead of the environment identity |
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm task events`

Append-only event history of one task

**Usage:**

```text
Usage: atm task events [OPTIONS] <TASK_ID>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<task_id>` | `` | `TASK_ID` | yes | `` |  | ID of the task whose events to show |
| `--limit` | `` | `N` | no | `` |  | Maximum number of events to return |
| `--all` | `` | `ALL` | no | `false` |  | Include every event instead of applying the limit |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--as` | `` | `ACTOR` | no | `` |  | Act as this member instead of the environment identity |
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm task history`

Past N tasks for the team, open and completed, newest first

**Usage:**

```text
Usage: atm task history [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--member` | `` | `MEMBER` | no | `` |  | Restrict history to tasks assigned to this member |
| `--limit` | `` | `N` | no | `` |  | Maximum number of past tasks to return |
| `--events` | `` | `EVENTS` | no | `false` |  | Include ledger events for the returned tasks |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--as` | `` | `ACTOR` | no | `` |  | Act as this member instead of the environment identity |
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm task list`

Queue view: the caller's open tasks; --all includes every member

**Usage:**

```text
Usage: atm task list [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--all` | `` | `ALL` | no | `false` |  | Include every member's open tasks |
| `--limit` | `` | `N` | no | `` |  | Maximum number of tasks to return |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--as` | `` | `ACTOR` | no | `` |  | Act as this member instead of the environment identity |
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm task move`

Reorder one member's queue

**Usage:**

```text
Usage: atm task move [OPTIONS] <--head|--end|--before <OTHER_TASK_ID>> <TASK_ID>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<task_id>` | `` | `TASK_ID` | yes | `` |  | ID of the task to move |
| `--head` | `` | `HEAD` | no | `false` |  | Move the task to the head of its queue |
| `--end` | `` | `END` | no | `false` |  | Move the task to the end of its queue |
| `--before` | `` | `OTHER_TASK_ID` | no | `` |  | Move the task before this task in its queue |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--as` | `` | `ACTOR` | no | `` |  | Act as this member instead of the environment identity |
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm task start`

Start an assigned task: moves it to active and tells the assigner

**Usage:**

```text
Usage: atm task start [OPTIONS] <TASK_ID> [MESSAGE]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<task_id>` | `` | `TASK_ID` | yes | `` |  | ID of the assigned task to start |
| `<text>` | `` | `MESSAGE` | no | `` |  | Message or report body |
| `--file` | `` | `FILE` | no | `` |  | Read the message or report body from this file |
| `--stdin` | `` | `STDIN` | no | `false` |  | Read the message or report body from standard input |
| `--template` | `` | `TEMPLATE` | no | `` |  | Render the message or report body from this template file |
| `--vars` | `` | `VARS` | no | `` |  | JSON object providing variables for `--template`; `-` reads stdin |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--as` | `` | `ACTOR` | no | `` |  | Act as this member instead of the environment identity |
| `--team` | `` | `TEAM` | no | `` |  | Override the team resolved from caller context |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm teams`

List teams or run one team-administration subcommand

**Usage:**

```text
Usage: atm teams [OPTIONS] [COMMAND]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--members` | `` | `MEMBERS` | no | `false` |  | Emit the picker member projection (ADR-055 decision (e), PRD §4.2/§5a) instead of the team-count list: per-member `{"id","name", "host","cwd","status"}`, consumed by `atm send --from-json`'s `recipients`. Only valid without a subcommand |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm teams add-member`

**Usage:**

```text
Usage: atm teams add-member [OPTIONS] <TEAM> <MEMBER>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<team>` | `` | `TEAM` | yes | `` |  | Team that will contain the new member |
| `<member>` | `` | `MEMBER` | yes | `` |  | Name of the member to add |
| `--agent-type` | `` | `AGENT_TYPE` | no | `general-purpose` |  | Agent-type metadata to store for the member |
| `--model` | `` | `MODEL` | no | `unknown` |  | Model metadata to store for the member |
| `--home-dir` | `` | `HOME_DIR` | no | `` |  | ATM home directory to store for the member |
| `--backend` | `` | `BACKEND` | no | `` |  | local receiver backend: tmux or herdr |
| `--target` | `` | `TARGET` | no | `` |  | tmux pane target; required for --backend tmux |
| `--session` | `` | `SESSION` | no | `` |  | Herdr session name; only valid with --backend herdr |
| `--alias` | `` | `ALIAS` | no | `` |  | durable roster alias; Herdr members use it as their live-agent target |
| `--pane-id` | `` | `PANE_ID` | no | `` |  | deprecated compatibility spelling for --backend tmux --target |
| `--host` | `` | `HOST` | no | `` |  | this member's registered host (ADR-055 decision (e)); used for Send-To same-host/remote routing, never inferred |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm teams backup`

**Usage:**

```text
Usage: atm teams backup [OPTIONS] <TEAM>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<team>` | `` | `TEAM` | yes | `` |  | Team whose roster configuration to back up |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm teams clear-nudge-template`

**Usage:**

```text
Usage: atm teams clear-nudge-template [OPTIONS] --team <TEAM> --kind <KIND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--team` | `` | `TEAM` | yes | `` |  | Team whose nudge template override to clear |
| `--kind` | `` | `KIND` | yes | `` |  | template kind: delivery, delivery_ack, queue, queue_ack, acknowledge, task_queued, task_ready, task_reminder, task_started, task_complete, task_closed; retired task and acknowledge_task are accepted for deletion |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm teams disable-nudge-template`

**Usage:**

```text
Usage: atm teams disable-nudge-template [OPTIONS] --team <TEAM> --kind <KIND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--team` | `` | `TEAM` | yes | `` |  | Team whose nudge template override to disable |
| `--kind` | `` | `KIND` | yes | `` |  | template kind: delivery, delivery_ack, queue, queue_ack, acknowledge, task_queued, task_ready, task_reminder, task_started, task_complete, task_closed |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm teams remove-member`

**Usage:**

```text
Usage: atm teams remove-member [OPTIONS] <TEAM> <MEMBER>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<team>` | `` | `TEAM` | yes | `` |  | Team containing the member to remove |
| `<member>` | `` | `MEMBER` | yes | `` |  | Name of the member to remove |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm teams restore`

**Usage:**

```text
Usage: atm teams restore [OPTIONS] <TEAM>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<team>` | `` | `TEAM` | yes | `` |  | Team whose roster configuration to restore |
| `--from` | `` | `FROM` | no | `` |  | Backup file to restore instead of the default backup location |
| `--dry-run` | `` | `DRY_RUN` | no | `false` |  | Validate and display the restore without writing it |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm teams set-nudge-template`

**Usage:**

```text
Usage: atm teams set-nudge-template [OPTIONS] --team <TEAM> --kind <KIND> --template-body <TEMPLATE_BODY>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--team` | `` | `TEAM` | yes | `` |  | Team whose nudge template override to set |
| `--kind` | `` | `KIND` | yes | `` |  | template kind: delivery, delivery_ack, queue, queue_ack, acknowledge, task_queued, task_ready, task_reminder, task_started, task_complete, task_closed |
| `--template-body` | `` | `TEMPLATE_BODY` | yes | `` |  | Template body to use for the override |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm teams update-member`

**Usage:**

```text
Usage: atm teams update-member [OPTIONS] <TEAM> <MEMBER>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<team>` | `` | `TEAM` | yes | `` |  | Team containing the member to update |
| `<member>` | `` | `MEMBER` | yes | `` |  | Name of the member to update |
| `--home-dir` | `` | `HOME_DIR` | no | `` |  | Replace the member's stored ATM home directory |
| `--workspace-root` | `` | `WORKSPACE_ROOT` | no | `` |  | Replace the member's stored workspace root |
| `--harness` | `` | `HARNESS` | no | `` |  | Replace the member's harness metadata |
| `--agent-type` | `` | `AGENT_TYPE` | no | `` |  | Replace the member's agent-type metadata |
| `--model` | `` | `MODEL` | no | `` |  | Replace the member's model metadata |
| `--backend` | `` | `BACKEND` | no | `` |  | local receiver backend: tmux or herdr |
| `--target` | `` | `TARGET` | no | `` |  | tmux pane target; required for --backend tmux |
| `--session` | `` | `SESSION` | no | `` |  | Herdr session name; only valid with --backend herdr |
| `--alias` | `` | `ALIAS` | no | `` |  | durable roster alias; Herdr members use it as their live-agent target |
| `--clear-alias` | `` | `CLEAR_ALIAS` | no | `false` |  | remove the member's durable roster alias |
| `--pane-id` | `` | `PANE_ID` | no | `` |  | deprecated compatibility spelling for --backend tmux --target |
| `--host` | `` | `HOST` | no | `` |  | this member's registered host (ADR-055 decision (e)); used for Send-To same-host/remote routing, never inferred |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

### `atm templates`

Inspect immutable templates registered by decomposed-message admission

**Usage:**

```text
Usage: atm templates [OPTIONS] <COMMAND>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm templates list`

List every known immutable template revision, optionally by metadata type

**Usage:**

```text
Usage: atm templates list [OPTIONS]
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `--type` | `` | `TEMPLATE_TYPE` | no | `` |  | Restrict results to this template metadata type |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |

#### `atm templates schema`

Show the stored schema/frontmatter for one exact immutable SHA

**Usage:**

```text
Usage: atm templates schema [OPTIONS] <SHA>
```

| Flag | Short | Value | Required | Default | Allowed values | Description |
|------|-------|-------|----------|---------|----------------|-------------|
| `<sha>` | `` | `SHA` | yes | `` |  | SHA of the immutable template revision |
| `--json` | `` | `JSON` | no | `false` |  | Emit the result as JSON |
| `--stderr-logs` | `` | `STDERR_LOGS` | no | `false` |  | Route retained observability console logs to stderr.  ATM owns normal command stdout output; this flag opts the shared console sink into stderr so retained diagnostics do not pollute stdout. |


