# agent-team-mail (`atm`)

`agent-team-mail` is the retained `1.0` CLI and core library for local ATM
mailbox workflows.

This repository is now the source of truth for publishing:
- `agent-team-mail`
- `agent-team-mail-core`

The installed command remains `atm`.

## What `1.0` Includes

The retained `1.0` release scope is the daemon-free CLI/core pair:
- `agent-team-mail` — the `atm` CLI
- `agent-team-mail-core` — the core Rust library used by the CLI

This release line continues to consume the published `sc-observability` family
for retained logging and health reporting:
- `sc-observability`
- `sc-observability-types`
- `sc-observability-otlp`

This repo does not publish the retired legacy daemon, MCP, TUI, or CI-monitor
artifacts as part of the retained `1.0` surface.

## Installation

### GitHub Releases

Download the latest release from
[GitHub Releases](https://github.com/randlee/atm-core/releases).

Published archives:

| Platform | Archive |
| --- | --- |
| Linux (x86_64) | `atm_<version>_x86_64-unknown-linux-gnu.tar.gz` |
| macOS (Intel) | `atm_<version>_x86_64-apple-darwin.tar.gz` |
| macOS (Apple Silicon) | `atm_<version>_aarch64-apple-darwin.tar.gz` |
| Windows (x86_64) | `atm_<version>_x86_64-pc-windows-msvc.zip` |

Extract the archive and place `atm` or `atm.exe` somewhere on your `PATH`.

### Homebrew

```bash
brew tap randlee/tap
brew install randlee/tap/agent-team-mail
```

### crates.io

```bash
cargo install agent-team-mail
```

The library crate is also published as:

```bash
cargo add agent-team-mail-core
```

### winget

```powershell
winget install randlee.agent-team-mail
```

`winget` is a new required `1.0` Windows channel rather than a historical
parity channel from the old repo. Public `winget` installability may lag by
1-2 days after release because Microsoft reviews new submissions and updates
before they become broadly visible.

### Build From Source

```bash
git clone https://github.com/randlee/atm-core.git
cd atm-core
cargo install --path crates/atm --bin atm
```

## Quick Start

ATM works against the local Claude team mailbox layout under `~/.claude/teams`.
Typical flows:

### Send a message

```bash
atm send teammate "Hello from ATM"
atm send teammate@other-team "Cross-team message"
atm send teammate "Please confirm" --requires-ack
```

### Read your mailbox

```bash
atm read
atm read --all --no-mark
atm read --pending-ack-only
```

### Acknowledge or clear messages

```bash
atm ack <message-id> "Acknowledged"
atm clear
```

### Inspect health and retained logs

```bash
atm doctor
atm log snapshot --level warn
```

### Manage teams

```bash
atm teams
atm members my-team
atm teams add-member my-team teammate
atm teams backup my-team
atm teams restore my-team --from backup.tar.gz --dry-run
```

Run `atm --help` or `atm <command> --help` for the full command surface.

## CLI Surface

The retained CLI includes:
- `send`
- `read`
- `ack`
- `clear`
- `log`
- `doctor`
- `teams`
- `members`

The `teams` command also contains retained team-administration subcommands:
- `add-member`
- `backup`
- `restore`

## Configuration Notes

ATM resolves runtime identity and team context from the current CLI/config
surface and uses the local Claude team directory layout for mailbox storage.

## Post-Send Hook

`atm send` can run an optional post-send hook configured in `.atm.toml`:

> Design note: the current implementation below still reflects the shipped
> `post_send_hook_members` behavior. The pending redesign documented in
> `docs/requirements.md`, `docs/architecture.md`, and `docs/project-plan.md`
> replaces that key with `post_send_hook_senders` /
> `post_send_hook_recipients`, adds `hook_match` payload metadata, and allows
> an optional structured stdout result from the hook.

```toml
[atm]
post_send_hook = ["scripts/tmux-nudge.sh", "--team", "atm-dev"]
post_send_hook_members = ["team-lead", "arch-ctm"]
```

Behavior:
- `post_send_hook` is a command argv array. If the first entry is a relative path, ATM resolves it relative to the directory containing `.atm.toml`.
- `post_send_hook_members` is a sender allowlist. The hook runs only when the sending identity is in this list. It is not a recipient filter.
- ATM sets `ATM_POST_SEND` to a JSON payload with `{from, to, message_id, requires_ack, task_id}`.
- The hook gets 5 seconds to complete.
- Hook stdout and stderr are suppressed.
- If the hook exits non-zero, fails to start, or times out, `atm send` still succeeds and prints a warning.

Example tmux auto-nudge hook for a Codex pane:

```bash
#!/usr/bin/env bash
set -euo pipefail

readarray -t fields < <(python3 - <<'PY'
import json, os
payload = json.loads(os.environ["ATM_POST_SEND"])
print(payload["to"].split("@", 1)[0])
print(payload["to"].split("@", 1)[1])
PY
)

recipient="${fields[0]}"
team="${fields[1]}"
tmux send-keys -t "$recipient" "You have unread ATM messages. Run: atm read --team $team" Enter
```

Useful docs in this repo:
- [requirements.md](docs/requirements.md)
- [architecture.md](docs/architecture.md)
- [project-plan.md](docs/project-plan.md)
- [WINGET_SETUP.md](docs/WINGET_SETUP.md)

## Development

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
