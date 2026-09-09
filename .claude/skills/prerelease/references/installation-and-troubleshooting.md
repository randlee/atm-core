# Installation and troubleshooting

## Check first

```bash
which python3 && python3 --version
python3 -c 'import sys; raise SystemExit("Python 3.11+ is required") if sys.version_info < (3, 11) else None'
which git && git --version
which gh && gh --version
gh auth status
```

## Find an existing installation

The agent environment may not inherit the interactive shell's `PATH`.

```bash
for command_path in \
  "$HOME/.local/bin/gh" \
  "$HOME/.local/bin/python3" \
  "$(python3 -m site --user-base 2>/dev/null)/bin/python3" \
  "/opt/homebrew/bin/gh" \
  "/opt/homebrew/bin/python3" \
  "/usr/local/bin/gh" \
  "/usr/local/bin/python3"; do
  [ -x "$command_path" ] && echo "Found at: $command_path"
done
```

Use a discovered absolute path or add its directory to `PATH` for the current
session. Do not edit shell startup files implicitly.

## Install

- GitHub CLI: follow <https://cli.github.com/> for macOS, Linux, or Windows.
- Python: install Python 3.11 or newer using the platform package manager or
  <https://www.python.org/downloads/>.
- Git: install it using the platform package manager or
  <https://git-scm.com/downloads>.

Authenticate GitHub CLI with `gh auth login`, then repeat the checks above.
Never place credentials in the manifest, command arguments, logs, or skill
output.

## Known issues

- A GUI-launched agent may have a narrower `PATH` than an interactive shell.
- `gh auth status` must succeed for the repository host before list, install,
  or create operations.
- The skill must run from a consumer root containing an enabled
  `[prerelease]` table in `release/publish-artifacts.toml`.
