---
name: installation-and-troubleshooting
version: 0.6.1
description: Installing and locating the CLIs and credentials plan-anchoring depends on (python3, PyYAML, br or bd, sc-compose, jq, TypeSafe API key), how the anchor tracker is selected, minimum versions, PATH issues in Claude Code, Codex and Cursor environment policy, validation, and known issues.
---

# Installation and troubleshooting

## Check first

```bash
which python3 && python3 --version
python3 -c 'import yaml; print("PyYAML", yaml.__version__)'
python3 "<skill dir>/scripts/anchor_tracker.py" resolve      # selected tracker, or a typed error
which br && br --version                                      # or bd, when resolve selects bd
which sc-compose && sc-compose --version
which jq && jq --version
test -n "$TYPESAFE_API_KEY" && echo "key set" || echo "key missing"
```

If these checks succeed, skip to Validation.

## Find an existing install

Claude Code's bash inherits a minimal PATH that can miss Homebrew, Go, pyenv, and `~/.local/bin`:

```bash
for t in br bd; do for p in "/opt/homebrew/bin/$t" "$HOME/.cargo/bin/$t" "$HOME/go/bin/$t" "$HOME/.local/bin/$t" "/usr/local/bin/$t"; do [ -x "$p" ] && echo "$t at $p"; done; done
for p in /opt/homebrew/bin/python3 /usr/local/bin/python3 /usr/bin/python3 "$HOME/.pyenv/shims/python3"; do [ -x "$p" ] && echo "python3 at $p"; done
for p in /opt/homebrew/bin/sc-compose "$HOME/.local/bin/sc-compose" /usr/local/bin/sc-compose; do [ -x "$p" ] && echo "sc-compose at $p"; done
for p in /opt/homebrew/bin/jq /usr/bin/jq /usr/local/bin/jq; do [ -x "$p" ] && echo "jq at $p"; done
```

Use the full path, or `export PATH="/opt/homebrew/bin:$PATH"` for the session.

## Install

**python3** (3.9 or newer; the scripts are standard-library only, except `anchor_tracker.py`, which needs PyYAML)

- macOS: `brew install python` or the Xcode command line tools' `/usr/bin/python3`
- Linux: `apt install python3` / `dnf install python3`
- Windows: `winget install Python.Python.3.12`; use forward slashes in paths

**PyYAML** (5.1 or newer; read only by `anchor_tracker.py` for `.sc/plan-anchor/settings.yaml`)

- `python3 -m pip install pyyaml` (into the interpreter the skill runs). Without it `anchor_tracker.py` returns `DEPENDENCY.PYYAML` and exits 2; it never falls back to defaults.

**br (beads_rust)**, the default anchor tracker

- macOS: `brew install dicklesworthstone/tap/br`; other platforms: the releases at github.com/Dicklesworthstone/beads_rust.
- Bootstrap once per repository with `br init` from the repository root, or let the skill do it: `python3 "<skill dir>/scripts/anchor_tracker.py" write --init`. `br init` is the only tracker bootstrap this skill performs.
- br keeps its SQLite database (`.beads/beads.db`) gitignored by design; the committed copy of the data is `.beads/issues.jsonl`. Run `br sync --flush-only` before committing so `issues.jsonl` carries every change, then commit `.beads/`.

**bd (beads)**, only for an existing Dolt-backed workspace

- bd is supported only where a Dolt-backed `.beads/` already exists (`.beads/metadata.json` has `"backend": "dolt"`). On these systems bd needs an upstream Dolt (DoltHub or a Dolt server); provisioning one is outside this skill, so never run `bd init` for plan anchoring. Without such a workspace, use br.
- Install: `brew install steveyegge/beads/bd` (or your team's tap), or with Go `go install github.com/steveyegge/beads/cmd/bd@latest` and `$HOME/go/bin` on PATH.

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
- Cursor's agent shell uses the environment of the Cursor app. A GUI launch does not read shell rc files, so set `TYPESAFE_API_KEY` with `launchctl setenv` (macOS, until reboot) before starting Cursor, and export `PLAN_ANCHOR_ALLOW_REMOTE_JUDGE=1` in that same app environment for a live session. A plugin variable does not export into `python3 jev_judge.py`.
- For the live rubric, tuning, fixture checks, and gates also `export PLAN_ANCHOR_ALLOW_REMOTE_JUDGE=1` for that session only (`jev-api.md`). The normal skill workflow requires Jev; offline commands are diagnostics and CI support.

## Anchor tracker selection

`anchor_tracker.py resolve` picks the CLI that stores anchors and question beads, independently of the plan source:

1. An explicit `anchor_tracker` in `.sc/plan-anchor/settings.yaml` wins, checked against disk: `bd` needs `.beads/metadata.json` with `"backend": "dolt"` (else `TRACKER.DOLT_REQUIRED`); `br` needs a `.beads/` that is not Dolt-backed, or none yet (else `TRACKER.MISMATCH`). A mismatch is an error, never a silent switch.
2. With no setting: a Dolt-backed `.beads/` selects `bd`; any other `.beads/` selects `br`; no `.beads/` selects `br`.
3. The selected CLI must be on PATH (`TRACKER.CLI_MISSING`).

`resolve` is read-only and never initializes. `write` records the result in the settings file (`--tracker br|bd` pins a choice, checked the same way; an existing setting is never rewritten, and a different pin is `SETTINGS.CONFLICT`). `write --init` also runs `br init` when br is selected and there is no `.beads/`. Commit what `write` reports in `commit_paths`.

Exit codes follow the plugin convention shared with `jev_judge.py` and `plan_source.py`: 0 selected; 2 any error, invalid arguments included (`VALIDATION.INPUT`), with the typed error object on stdout. Act on the error code (`SETTINGS.*`, `BEADS.*`, `TRACKER.*`, `DEPENDENCY.PYYAML`, `REPO.NOT_FOUND`, `SCRIPT.INTERNAL`), not the exit status. Output is always the JSON envelope `{success, data, error}`; the reported `cli_path` is absolute.

**Settings file.** Synaptic Canvas packages keep repository settings at `<repository root>/.sc/<package>/settings.yaml`: YAML, snake_case keys, committed to git, never secrets. The repository root is `git rev-parse --show-toplevel` (falling back to the nearest ancestor with `.git`), never the working directory. For this package:

```yaml
# .sc/plan-anchor/settings.yaml
# Repository settings for the sc-plan-anchor plugin; commit this file, never put secrets in it.
schema_version: 1
anchor_tracker: br
```

`schema_version` must be `1`. Unknown or duplicate keys are rejected (`SETTINGS.UNKNOWN_KEY`, `SETTINGS.MALFORMED`) so a typo never falls back silently. When the file or its `anchor_tracker` key is absent, detection (rule 2) applies. `.gitignore` may ignore `.sc/sessions/` and `.sc/*/cache/`, never this file; `write` reports `settings_ignored_by_git` when it is.

This version wires the selection into the planning gate only. The development gate (`anchor-sprint-review`) is unchanged and still uses `bd`.

## Minimum versions

| Tool | Minimum | Why |
| --- | --- | --- |
| python3 | 3.9 | tested floor; standard library only, except `anchor_tracker.py` |
| PyYAML | 5.1 | `SafeLoader`-based strict reading of `.sc/plan-anchor/settings.yaml`; CI tests 5.1 and the latest release |
| br | 0.7.x | `init`, `show --json`, `sync --flush-only`; tested with 0.7.4 |
| bd | 1.x, Dolt-backed `.beads/` only | `--parent`, `--labels`, `--silent`, `--body-file`, `show --json`, `children --json`, `comment --file`, `ready --exclude-label` |
| sc-compose | 1.7.0 | strict render, declared-variable validation, and `--check-render` |
| jq | any current release | gate agents read question-override beads |
| Jev | `jev-latest` | pin a version once thresholds are tuned |

## PATH troubleshooting

- A tool works in your terminal but not in the skill: Claude Code's bash did not source the shell init that added it to PATH. Export the directory in the session or use the full path.
- `sc-compose` is missing: install 1.7.0 or newer and rerun the dependency
  gate; do not hand-assemble the fenced tuning payload.
- `br show --json` or `bd show --json` errors with "no database": run from the repo root that has `.beads/`, or pass `--db <path>` (bd also accepts `-C <repo root>`). In a fresh clone br imports `issues.jsonl` into its gitignored database automatically; `br sync --import-only` does it explicitly.
- The key works in your terminal but the skill says it is missing: it was exported after Claude Code started, or only in an interactive rc file. Put it in `~/.zshenv`; restart Claude Code.

## Validation

```bash
S="<skill dir>/scripts/jev_judge.py"; X="<skill dir>/assets/examples"
python3 "$S" --help
python3 "$S" validate --anchors "$X/anchors.example.json"
python3 "$S" judge --anchors "$X/anchors.example.json" --gate plan --all --item-id smoke --item-file "<skill dir>/SKILL.md" \
  --answers-file "$X/answers.fixture.json" --out /tmp/anchor-smoke.json; echo "exit $?"
python3 "$S" report --run /tmp/anchor-smoke.json --format md
# Maintainer source checkout only; the marketplace plugin excludes this suite.
python3 -m unittest discover -s "<repository root>/tests/sc-plan-anchor"
```

The smoke `judge` runs the full gating path offline: expect `not_addressed` for ANC-012, `not_applicable` for REQ-014, and **exit 1**, because the gate fails by design. In a maintainer source checkout, the repository-level test suite reports `OK`; it is not included in marketplace installations.

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
- `TRACKER.MISMATCH`: the settings file says `br` but `.beads/` is Dolt-backed. Set `anchor_tracker: bd`, or remove the key to detect.
- `TRACKER.DOLT_REQUIRED`: `bd` was requested without a Dolt-backed `.beads/`. Dolt provisioning is outside this skill; use `anchor_tracker: br`.
- `TRACKER.CLI_MISSING`: the selected tracker is not on PATH; see Find an existing install.
- `TRACKER.INIT_FAILED`: `br init` exited non-zero (its exit code and stderr are in `data`) or created no workspace.
- `SETTINGS.UNKNOWN_KEY`, `SETTINGS.SCHEMA_VERSION`, `SETTINGS.INVALID_TRACKER`, `SETTINGS.MALFORMED`: fix `.sc/plan-anchor/settings.yaml` to the form above.
- `BEADS.METADATA_INVALID`: `.beads/metadata.json` is not a readable JSON object; repair it before selection can run.
- Windows: run `br`/`bd` and the script from Git Bash or PowerShell with forward slashes; report short-date formatting falls back gracefully.
