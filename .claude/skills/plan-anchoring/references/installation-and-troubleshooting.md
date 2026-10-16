---
name: installation-and-troubleshooting
version: 0.4.1
description: Installing and locating the CLIs and credentials plan-anchoring depends on (python3, beads, sc-compose, TypeSafe API key), minimum versions, PATH issues in Claude Code, Codex environment policy, validation, and known issues.
---

# Installation and troubleshooting

## Check first

```bash
which python3 && python3 --version
which bd && bd --version
which sc-compose && sc-compose --version
test -n "$TYPESAFE_API_KEY" && echo "key set" || echo "key missing"
```

If all three succeed, skip to Validation.

## Find an existing install

Claude Code's bash inherits a minimal PATH that can miss Homebrew, Go, pyenv, and `~/.local/bin`:

```bash
for p in /opt/homebrew/bin/bd "$HOME/go/bin/bd" "$HOME/.local/bin/bd" /usr/local/bin/bd; do [ -x "$p" ] && echo "bd at $p"; done
for p in /opt/homebrew/bin/python3 /usr/local/bin/python3 /usr/bin/python3 "$HOME/.pyenv/shims/python3"; do [ -x "$p" ] && echo "python3 at $p"; done
for p in /opt/homebrew/bin/sc-compose "$HOME/.local/bin/sc-compose" /usr/local/bin/sc-compose; do [ -x "$p" ] && echo "sc-compose at $p"; done
```

Use the full path, or `export PATH="/opt/homebrew/bin:$PATH"` for the session.

## Install

**python3** (3.9 or newer; the script is standard-library only)

- macOS: `brew install python` or the Xcode command line tools' `/usr/bin/python3`
- Linux: `apt install python3` / `dnf install python3`
- Windows: `winget install Python.Python.3.12`; use forward slashes in paths

**beads (`bd`)**

- macOS: `brew install steveyegge/beads/bd` (or your team's tap)
- Any platform with Go: `go install github.com/steveyegge/beads/cmd/bd@latest`, then add `$HOME/go/bin` to PATH
- Initialise once per repo: `bd init`. An error mentioning CGO or embedded Dolt means your binary needs `bd init --proxied-server` (or a Homebrew build). `bd init` also writes and commits `.claude/settings.json`, `CLAUDE.md`, `AGENTS.md` and git hooks; `--skip-agents --skip-hooks` avoids that. The issue prefix defaults to the directory name.

**sc-compose** (1.7.0 or newer)

- Install it from the Synaptic Canvas `sc-compose` release or team package
  source, then confirm `sc-compose --version` reports at least 1.7.0.
- The skill uses strict checked-in Jinja2 templates for every anchor-tuning
  request/result. Do not continue with hand-built JSON when this CLI is absent.

**TypeSafe API key**

- Create a key at typesafe.ai. Read it silently so it does not appear in the command line, shell history, or terminal output, then export it in the shell that launches Claude Code or Codex:
  ```bash
  mkdir -p ~/.config/typesafe && chmod 700 ~/.config/typesafe
  printf 'TypeSafe API key: ' >&2
  IFS= read -r -s TYPESAFE_API_KEY
  printf '\n' >&2
  export TYPESAFE_API_KEY
  (umask 077; printf 'export TYPESAFE_API_KEY=%q\n' "$TYPESAFE_API_KEY" > ~/.config/typesafe/env)
  echo '[ -r "$HOME/.config/typesafe/env" ] && . "$HOME/.config/typesafe/env"' >> ~/.zshenv   # and ~/.bash_profile for bash
  ```
  GUI-launched apps do not read shell files; use `launchctl setenv TYPESAFE_API_KEY "$TYPESAFE_API_KEY"` (macOS, until reboot). Never commit or echo the key.
- Under Codex, `TYPESAFE_API_KEY` matches the default `*KEY*` exclusion for command environments. If you restrict `[shell_environment_policy]` in `config.toml`, keep `ignore_default_excludes = true`, and with an allowlist under `[shell_environment_policy.filters]` add `"TYPESAFE_API_KEY" = "include"`.
- For the live rubric, tuning, fixture checks, and gates also `export PLAN_ANCHOR_ALLOW_REMOTE_JUDGE=1` for that session only (`jev-api.md`). The normal skill workflow requires Jev; offline commands are diagnostics and CI support.

## Minimum versions

| Tool | Minimum | Why |
| --- | --- | --- |
| python3 | 3.9 | tested floor; standard library only |
| bd | 1.x | `--parent`, `--labels`, `--silent`, `--body-file`, `show --json`, `children --json`, `comment --file`, `ready --exclude-label` |
| sc-compose | 1.7.0 | strict render, declared-variable validation, and `--check-render` |
| Jev | `jev-latest` | pin a version once thresholds are tuned |

## PATH troubleshooting

- A tool works in your terminal but not in the skill: Claude Code's bash did not source the shell init that added it to PATH. Export the directory in the session or use the full path.
- `sc-compose` is missing: install 1.7.0 or newer and rerun the dependency
  gate; do not hand-assemble the fenced tuning payload.
- `bd show --json` errors with "no database": run from the repo root that has `.beads/`, or pass `-C <repo root>` (or `--db <path>`).
- The key works in your terminal but the skill says it is missing: it was exported after Claude Code started, or only in an interactive rc file. Put it in `~/.zshenv`; restart Claude Code.

## Validation

```bash
S="<skill dir>/scripts/jev_judge.py"; X="<skill dir>/assets/examples"
python3 "$S" --help
python3 "$S" validate --anchors "$X/anchors.example.json"
python3 "$S" judge --anchors "$X/anchors.example.json" --gate plan --all --item-id smoke --item-file "<skill dir>/SKILL.md" \
  --answers-file "$X/answers.fixture.json" --out /tmp/anchor-smoke.json; echo "exit $?"
python3 "$S" report --run /tmp/anchor-smoke.json --format md
python3 -m unittest discover -s "<skill dir>/tests"
```

The smoke `judge` runs the full gating path offline: expect `not_addressed` for ANC-012, `not_applicable` for REQ-014, and **exit 1**, because the gate fails by design. The test suite reports `OK`.

## Known issues

- `POLICY.REMOTE_JUDGE_DISABLED` on a live run: set `PLAN_ANCHOR_ALLOW_REMOTE_JUDGE=1` only once sending this plan's text to TypeSafe is acceptable.
- `AUTH.CREDENTIALS` with the key set: it contains whitespace or a control character (usually a pasted trailing newline), or TypeSafe rejected it.
- `QUESTIONS.ROLES`: a question override renamed a question ID. Keep the default IDs.
- `ANCHOR.NOT_LOCKED`: `--anchor` named a proposed or retired anchor. Lock it, or use `--all`.
- `JEV.CONTEXT_EXCEEDED`: the item could not be shortened below 2,000 characters and still fit. Shorten the evidence account; at the development gate drop the test section first.
- `JEV.RESPONSE_INVALID` repeatedly: TypeSafe changed the response shape or a proxy is mangling it. Check `PLAN_ANCHOR_JEV_URL`.
- `VALIDATION.ANCHORS` from `judge`: run `validate` and fix the listed problems; the usual cause is a hand edit. Use `update`, `retire` and `promote`.
- `JEV.RATE_LIMITED` under parallel gates: cap concurrency at 3 beads; the script already retries three times with backoff.
- Dates in reports show as "undated": `stated_at` is not an ISO date. Use `YYYY-MM-DD`.
- `bd comment --body-file`: does not exist; use `--file` (or `--stdin`).
- Windows: run `bd` and the script from Git Bash or PowerShell with forward slashes; report short-date formatting falls back gracefully.
