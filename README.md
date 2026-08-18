# agent-team-mail (`atm`)

`agent-team-mail` is the retained ATM CLI and daemon-backed runtime for local
ATM mailbox workflows.

This repository is now the source of truth for publishing:
- `agent-team-mail`
- `agent-team-mail-core`

The current Phase AI branch candidate is `1.4.0-beta-ai`; its CLI and
daemon must be switched and run as a matching pair for branch smoke evidence.
Run that evidence only through the canonical [`just smoke` command surface](docs/smoke-testing.md).

The installed command remains `atm`.

## What The Retained Line Includes

The retained release scope is the `atm` CLI plus the accepted same-host
daemon/SQLite runtime it bootstraps and talks to:
- `agent-team-mail` — the `atm` CLI entrypoint
- `agent-team-mail-core` — shared semantic and boundary code
- `atm-daemon` — the retained same-host daemon runtime used by `send`, `read`,
  `ack`, and `doctor`

This release line continues to consume the published `sc-observability` family
for retained logging and health reporting:
- `sc-observability`
- `sc-observability-types`
- `sc-observability-otlp`

This repo does not publish the retired Claude-compatibility runtime, MCP, TUI,
or CI-monitor artifacts as part of the retained ATM surface.

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

### PyPI

`hermes-atm` and `atm-graft` 1.4.2 are live on TestPyPI; publishing to
production PyPI is pending.

```bash
python -m pip install --upgrade \
  --index-url https://test.pypi.org/simple \
  --extra-index-url https://pypi.org/simple \
  "hermes-atm==1.4.2" "atm-graft==1.4.2"
```

For Hermes setup and verification, see
[the hermes-atm guide](docs/user-documents/hermes-atm.md).

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

ATM runs against the accepted ATM home/runtime layout and persists retained
mail state through the same-host daemon plus durable SQLite storage. Typical
flows:

### Send a message

```bash
atm send teammate "Hello from ATM"
atm send teammate@other-team "Cross-team message"
atm send teammate "Please confirm" --requires-ack
```

### Read your mailbox

```bash
atm read
atm peek --all
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
surface and uses the accepted daemon/SQLite runtime for retained mail state.

## Post-Send Hook

ATM ships one default post-send path for successful `atm send` and `atm ack`:
the built-in `atm internal-nudge` command. Most teams do not need any
`.atm.toml` hook configuration.

Use `[[atm.post_send_hooks]]` only for an explicit local override or
compatibility helper:

```toml
[[atm.post_send_hooks]]
recipient = "team-lead"
command = ["scripts/atm-nudge.sh", "team-lead"]

[[atm.post_send_hooks]]
recipient = "arch-ctm"
command = ["scripts/atm-nudge.sh", "arch-ctm"]
```

Behavior:
- If no matching external rule is configured, ATM falls back to the shipped
  built-in `atm internal-nudge` path.
- Each `[[atm.post_send_hooks]]` rule binds one `recipient` and one `command`.
- `recipient` matches either one exact member name or `*` for all recipients.
- Multiple matching rules all run, in config order.
- If `command[0]` is path-like, ATM resolves it relative to the directory containing `.atm.toml`.
- Bare executables like `bash`, `python3`, or `tmux` use normal `PATH` resolution.
- Recipient non-match is silent.
- ATM rejects retired `post_send_hook`, `post_send_hook_senders`, `post_send_hook_recipients`, and `post_send_hook_members` keys with migration guidance.
- ATM sets `ATM_POST_SEND` to a JSON payload with `{from, to, sender, recipient, team, message_id, requires_ack}` plus optional `task_id` when present.
- The hook gets 5 seconds to complete.
- Hook stderr is suppressed. Hook stdout may optionally return one JSON object with `level`, `message`, and optional `fields` for ATM to log.
- For troubleshooting hook diagnostics, combine `--stderr-logs` with `ATM_LOG=debug` to surface debug-level hook results on stderr.
- If the hook exits non-zero, fails to start, or times out, `atm send` still succeeds and prints a warning.

Repo-local `scripts/atm-nudge.sh` / `scripts/atm-nudge.py` remain
compatibility-only helpers. They are not the shipped default.

Example `ATM_POST_SEND` payload:

```json
{
  "from": "team-lead@atm-dev",
  "to": "arch-ctm@atm-dev",
  "sender": "team-lead",
  "recipient": "arch-ctm",
  "team": "atm-dev",
  "message_id": "01KWTMCQ418Q96BFRDTKGZMEAS",
  "requires_ack": true
}
```

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

Optional structured hook result on stdout:

```json
{"level":"debug","message":"arch-ctm nudged on pane %42","fields":{"pane_id":"%42"}}
```

Useful docs in this repo:
- [requirements.md](docs/requirements.md)
- [architecture.md](docs/architecture.md)
- [project-plan.md](docs/project-plan.md)
- [WINGET_SETUP.md](docs/WINGET_SETUP.md)

## Development

Windows first-run prerequisites on a new machine:
- install Rust `1.94.1` with the MSVC toolchain
- install Visual Studio C++ build tools
- install `just` (`winget install Casey.Just`)
- ensure `python` resolves on `PATH` for repo scripts and helpers

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Windows CI parity command:

```powershell
$env:ATM_TEST_RECV_TIMEOUT_SECS="60"
cargo test --workspace --verbose
```
