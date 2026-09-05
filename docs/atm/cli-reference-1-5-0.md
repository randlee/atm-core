# ATM CLI Reference

This document is generated from the live `clap` command tree. Do not hand-edit it — regenerate with `cargo run -p agent-team-mail --features cli-surface-dump --example gen_cli_docs` (see `crates/atm/src/cli_surface.rs`).

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

### `atm compose`

Render a template through the core renderer port and print the exact body

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--template` |  | yes | Template file to validate and render |
| `--vars` |  | no | JSON object providing template variables; `-` reads stdin |
| `--var` |  | no | One template variable; may be repeated |
| `--env-prefix` |  | no | Capture environment variables with this prefix |
| `--dry-run` |  | no | Validate and render without any side effects (the default operation is already side-effect free; the flag makes scripts self-documenting) |
| `--json` |  | no | Emit a structured result instead of the byte-identical rendered body |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

**Notes:**

Composition is local and never reads or writes the mailbox. Use `atm send <agent> --template <path> --vars <file>` to deliver the same template after previewing it.

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
| `--source` |  | no | Select the retained-log source. JSONL is the byte-compatible default |
| `--level` |  | no | Restrict results to one or more severity levels |
| `--match` |  | no | Match one structured ATM field exactly, for example command=send |
| `--since` |  | no | Inclusive lower time bound as RFC3339 or a relative duration like 15m |
| `--component` |  | no | Restrict timeline records to a component prefix |
| `--limit` |  | no | Maximum number of returned records |
| `--json` |  | no | Emit machine-readable JSON output |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm log snapshot`

Query recent ATM log records

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--source` |  | no | Select the retained-log source. JSONL is the byte-compatible default |
| `--level` |  | no | Restrict results to one or more severity levels |
| `--match` |  | no | Match one structured ATM field exactly, for example command=send |
| `--since` |  | no | Inclusive lower time bound as RFC3339 or a relative duration like 15m |
| `--component` |  | no | Restrict timeline records to a component prefix |
| `--limit` |  | no | Maximum number of returned records |
| `--json` |  | no | Emit machine-readable JSON output |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm log tail`

Follow new ATM log records as they arrive

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--source` |  | no | Select the retained-log source. JSONL is the byte-compatible default |
| `--level` |  | no | Restrict results to one or more severity levels |
| `--match` |  | no | Match one structured ATM field exactly, for example command=send |
| `--since` |  | no | Inclusive lower time bound as RFC3339 or a relative duration like 15m |
| `--component` |  | no | Restrict timeline records to a component prefix |
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

#### `atm peer trust`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

##### `atm peer trust add`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--host` |  | yes |  |
| `--fingerprint` |  | yes |  |
| `--https-port` |  | no |  |
| `--yes` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

##### `atm peer trust list`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

##### `atm peer trust migrate`

Migrate or retire legacy literal-IP trusted-peer entries.

Without `--yes`, prints the migration plan (one line per legacy literal-IP row: enabled/disabled, fingerprint, port, and the action) and makes no changes. With `--yes`, applies the plan: rows named by `--map IP=HOSTNAME` convert to the durable hostname, preserving their fingerprint and port; every other legacy literal-IP row is revoked.

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--map` |  | no | Convert the legacy literal-IP entry `IP` to durable hostname `HOSTNAME`, keeping its fingerprint and port. May be repeated |
| `--yes` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

##### `atm peer trust replace`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--host` |  | yes |  |
| `--fingerprint` |  | yes |  |
| `--https-port` |  | no |  |
| `--yes` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

##### `atm peer trust revoke`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--host` |  | yes |  |
| `--yes` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

### `atm queue`

Queue one ATM mailbox message for deferred recipient notification

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<to>` |  | no |  |
| `<message>` |  | no |  |
| `--team` |  | no |  |
| `--host` |  | no | Route this send through the explicitly named host |
| `--chat-id` |  | no |  |
| `--as` |  | no |  |
| `--file` |  | no |  |
| `--stdin` |  | no |  |
| `--template` |  | no | Render and send a locally loaded template through the daemon-owned template admission path |
| `--vars` |  | no | JSON object providing template variables. `-` reads this object from stdin; it is distinct from `--stdin`, which is a plain message source |
| `--var` |  | no | One template variable. May be repeated; values parse as JSON when possible and otherwise remain strings |
| `--env-prefix` |  | no | Capture current environment variables with this prefix at CLI composition time |
| `--attach` |  | no | Attach a local file for Send-To delivery (ADR-055). May be repeated. A same-host recipient's files are staged under `$ATM_TEMP/send-to/<id>/`; a remote recipient's files are routed through that host's configured transfer script (see `docs/cross-host-file-transfer.md`). The landed path rides in the message text; there is no envelope change. Mutually exclusive with `--template` (structured template content and free-form attachment notes are not composed in this phase) |
| `--from-json` |  | no | Read one `PickerOutput` JSON document (`{"schema_version":1,"recipients":[...],"note":"..."}`) from stdin and send one immutable message per recipient (ADR-055). Mutually exclusive with the positional recipient/message text, `--stdin`, `--file`, `--template`, and `--env-prefix` |
| `--category` |  | no |  |
| `--tag` |  | no |  |
| `--content-format` |  | no |  |
| `--summary` |  | no |  |
| `--requires-ack` |  | no |  |
| `--task-id` |  | no |  |
| `--dry-run` |  | no |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

**Notes:**

Path-only bodies are admitted for compatibility but recorded as content_format=path-ref and warned on stderr; use `atm send --template <path> --vars <file>` to send rendered content, and `atm compose --template <path>` to preview it. Post-send hooks can be configured in .atm.toml via one or more [[atm.post_send_hooks]] rules with recipient = "name-or-*" and command = ["argv", ...]. Matching rules run after a successful non-dry-run send, in config order. Path-like command[0] values resolve relative to the declaring .atm.toml; bare executables like bash or python3 use normal PATH resolution. Recipient non-match is silent. For hook troubleshooting, combine --stderr-logs with ATM_LOG=debug to surface debug-level hook diagnostics on stderr.

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

### `atm search`

Search locally indexed ATM messages through the daemon's typed query API

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<text>` |  | no | Literal phrase by default, or an ATM advanced expression with --raw-match |
| `--raw-match` |  | no | Parse the positional text with ATM's documented bounded advanced grammar |
| `--template-meta` |  | no | Filter stored template frontmatter metadata; a trailing * is a prefix match |
| `--type` |  | no | Shorthand for --template-meta type=VALUE |
| `--template-sha` |  | no | Filter the exact immutable template revision |
| `--var` |  | no | Filter one stored template variable; may be repeated |
| `--tag` |  | no |  |
| `--effective-tag` |  | no | Filter ATM's immutable effective-tag projection; may be repeated |
| `--category` |  | no |  |
| `--from` |  | no |  |
| `--team` |  | no |  |
| `--agent` |  | no |  |
| `--workflow-scope-kind` |  | no |  |
| `--workflow-scope-id` |  | no |  |
| `--workflow-state` |  | no |  |
| `--workflow-stage` |  | no |  |
| `--workflow-transition` |  | no |  |
| `--workflow-iteration` |  | no |  |
| `--lifecycle-scope-kind` |  | no | Project generic lifecycle observations over the local search result set |
| `--lifecycle-scope-id` |  | no |  |
| `--lifecycle-start-state` |  | no |  |
| `--lifecycle-start-stage` |  | no |  |
| `--lifecycle-start-transition` |  | no |  |
| `--lifecycle-end-state` |  | no |  |
| `--lifecycle-end-stage` |  | no |  |
| `--lifecycle-end-transition` |  | no |  |
| `--since` |  | no |  |
| `--until` |  | no |  |
| `--limit` |  | no |  |
| `--cursor` |  | no |  |
| `--per-mailbox` |  | no | Preserve per-mailbox compound-key identities rather than default deduplication |
| `--count` |  | no |  |
| `--group-by` |  | no |  |
| `--min` |  | no |  |
| `--max` |  | no |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

**Notes:**

Plain positional text is always a literal phrase. --raw-match enables ATM's bounded advanced grammar (words, quoted phrases, NEAR(term term[, distance]), AND, OR, NOT); it never passes raw SQLite FTS syntax.

### `atm send`

Send one ATM mailbox message

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<to>` |  | no |  |
| `<message>` |  | no |  |
| `--team` |  | no |  |
| `--host` |  | no | Route this send through the explicitly named host |
| `--chat-id` |  | no |  |
| `--as` |  | no |  |
| `--file` |  | no |  |
| `--stdin` |  | no |  |
| `--template` |  | no | Render and send a locally loaded template through the daemon-owned template admission path |
| `--vars` |  | no | JSON object providing template variables. `-` reads this object from stdin; it is distinct from `--stdin`, which is a plain message source |
| `--var` |  | no | One template variable. May be repeated; values parse as JSON when possible and otherwise remain strings |
| `--env-prefix` |  | no | Capture current environment variables with this prefix at CLI composition time |
| `--attach` |  | no | Attach a local file for Send-To delivery (ADR-055). May be repeated. A same-host recipient's files are staged under `$ATM_TEMP/send-to/<id>/`; a remote recipient's files are routed through that host's configured transfer script (see `docs/cross-host-file-transfer.md`). The landed path rides in the message text; there is no envelope change. Mutually exclusive with `--template` (structured template content and free-form attachment notes are not composed in this phase) |
| `--from-json` |  | no | Read one `PickerOutput` JSON document (`{"schema_version":1,"recipients":[...],"note":"..."}`) from stdin and send one immutable message per recipient (ADR-055). Mutually exclusive with the positional recipient/message text, `--stdin`, `--file`, `--template`, and `--env-prefix` |
| `--category` |  | no |  |
| `--tag` |  | no |  |
| `--content-format` |  | no |  |
| `--summary` |  | no |  |
| `--requires-ack` |  | no |  |
| `--task-id` |  | no |  |
| `--dry-run` |  | no |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

**Notes:**

Path-only bodies are admitted for compatibility but recorded as content_format=path-ref and warned on stderr; use `atm send --template <path> --vars <file>` to send rendered content, and `atm compose --template <path>` to preview it. Post-send hooks can be configured in .atm.toml via one or more [[atm.post_send_hooks]] rules with recipient = "name-or-*" and command = ["argv", ...]. Matching rules run after a successful non-dry-run send, in config order. Path-like command[0] values resolve relative to the declaring .atm.toml; bare executables like bash or python3 use normal PATH resolution. Recipient non-match is silent. For hook troubleshooting, combine --stderr-logs with ATM_LOG=debug to surface debug-level hook diagnostics on stderr.

### `atm teams`

List teams or run one team-administration subcommand

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--json` |  | no |  |
| `--members` |  | no | Emit the picker member projection (ADR-055 decision (e), PRD §4.2/§5a) instead of the team-count list: per-member `{"id","name", "host","cwd","status"}`, consumed by `atm send --from-json`'s `recipients`. Only valid without a subcommand |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm teams add-member`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<team>` |  | yes |  |
| `<member>` |  | yes |  |
| `--agent-type` |  | no |  |
| `--model` |  | no |  |
| `--home-dir` |  | no |  |
| `--backend` |  | no | local receiver backend: tmux or herdr |
| `--target` |  | no | tmux pane target; required for --backend tmux |
| `--session` |  | no | Herdr session name; only valid with --backend herdr |
| `--pane-id` |  | no | deprecated compatibility spelling for --backend tmux --target |
| `--host` |  | no | this member's registered host (ADR-055 decision (e)); used for Send-To same-host/remote routing, never inferred |
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

#### `atm teams remove-member`

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<team>` |  | yes |  |
| `<member>` |  | yes |  |
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
| `--workspace-root` |  | no |  |
| `--harness` |  | no |  |
| `--agent-type` |  | no |  |
| `--model` |  | no |  |
| `--backend` |  | no | local receiver backend: tmux or herdr |
| `--target` |  | no | tmux pane target; required for --backend tmux |
| `--session` |  | no | Herdr session name; only valid with --backend herdr |
| `--pane-id` |  | no | deprecated compatibility spelling for --backend tmux --target |
| `--host` |  | no | this member's registered host (ADR-055 decision (e)); used for Send-To same-host/remote routing, never inferred |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

### `atm templates`

Inspect immutable templates registered by decomposed-message admission

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm templates list`

List every known immutable template revision, optionally by metadata type

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `--type` |  | no |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |

#### `atm templates schema`

Show the stored schema/frontmatter for one exact immutable SHA

| Flag | Short | Required | Description |
|------|-------|----------|-------------|
| `<sha>` |  | yes |  |
| `--json` |  | no |  |
| `--stderr-logs` |  | no | Route retained observability console logs to stderr |


