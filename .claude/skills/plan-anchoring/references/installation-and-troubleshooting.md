---
name: installation-and-troubleshooting
version: 0.9.2
description: Installing plan-anchor into a repository or a parent folder with the plan-anchor-installer skill (install.py), per-repository overrides and per-folder upgrades, the anchor tracker this install is rendered for and how to switch it, the CLIs and credentials plan-anchoring depends on (python3, PyYAML, bd, sc-compose, jq, TypeSafe API key), minimum versions, PATH issues in Claude Code, Codex and Cursor environment policy, validation, and known issues.
---

# Installation and troubleshooting

## Check first

```bash
which python3 && python3 --version
python3 -c 'import yaml; print("PyYAML", yaml.__version__)'
python3 "<skill dir>/scripts/anchor_tracker.py" resolve      # selected tracker, or a typed error
which bd && bd --version
which sc-compose && sc-compose --version
which jq && jq --version
test -n "$TYPESAFE_API_KEY" && echo "environment key set (preferred)" || echo "environment key absent; check the documented platform credential store"
```

If these checks succeed, skip to Validation. The environment-key message is informational: a documented local platform credential can also satisfy live Jev authentication.

## Install plan-anchor into a folder

The marketplace plugin (`/plugin install sc-plan-anchor`, the Codex and Cursor plugin catalogs) ships one skill, `plan-anchor-installer`. It installs nothing globally: ask it to install plan-anchor and it runs

```bash
uv run "<installer plugin>/install.py" --dest <folder> --tracker br|bd --allow-remote-judge|--no-remote-judge [--target claude,codex,cursor] [--dry-run]
```

`uv` reads the script's inline dependencies (PyYAML and the `sc-compose` wheel) and needs no pip step; from a plan-anchor checkout, `uv run plugins/sc-plan-anchor/install.py ...` is the same command. The installer renders the skill and agent text for one tracker, so an installed copy names only that tracker, and writes `<folder>/.claude/skills/plan-anchoring/` and `<folder>/.claude/agents/anchor-*.md` (`.codex/` or `.cursor/` with `--target`; default `claude`). It records the version, tracker, targets and the sha256 of every file it wrote in `<folder>/.sc/plan-anchor/install-lock.json`; commit that file with the installed files. When no settings file says so yet, the installer skill asks once for the tracker and whether plan text may be sent to TypeSafe (Jev); it never infers that consent from earlier runs.

**Destination.** `<folder>` is a repository, or a parent folder that holds several repositories (it need not be a git repository). Global locations are refused with `INSTALL.DEST_GLOBAL` (exit 2): the home directory itself, `~/.claude`, `~/.codex`, `~/.cursor`, `~/.agents`, `/etc/codex`, and any plugin cache under them. A parent folder that is not a repository and has no `.beads/` needs `--tracker` (`TRACKER.REQUIRED` otherwise); `--init-br` is ignored there, so run `br init` in each repository. A subfolder of a repository is not a parent folder: without `--tracker` the installer takes the tracker from the nearest `.sc/plan-anchor/settings.yaml` above it, or from the enclosing repository's `.beads/`, and `--init-br` runs at the repository root.

**Parent-folder installs and overrides.** The gates trust the `jev_judge.py` in the nearest `<dir>/.claude|.codex|.cursor/skills/plan-anchoring/scripts/`, walking up from the working directory and stopping before the home directory, and only when `<dir>/.sc/plan-anchor/install-lock.json` lists that script: a copy the installer did not write (hand-copied, or with the lock removed), a copy under `.agents/`, or a copy in a global location fails with `VALIDATION.PATH`; rerun the installer to fix it. Settings follow the same walk, starting at the working directory even inside a repository: the nearest `.sc/plan-anchor/settings.yaml` wins whole (files are not merged), so a repository under a parent install can override the tracker or `allow_remote_judge` with its own file. The tracker check still runs in each repository: a `br` setting over a Dolt-backed `.beads/` is `TRACKER.MISMATCH`. `.beads/`, `anchors.json`, plans and run files stay in each repository. Whether a harness discovers a skill installed above the repository it was started in is the harness's own rule; Claude Code and Codex document skill discovery up to the repository root, so start the session in the folder that holds the install, or install into the repository.

**Upgrade.** An upgrade is a rerun of the same command for that folder; each install folder is upgraded on its own. `install.py --dest <folder> --status` reports, read-only, the installed and installer versions (for example `installed 0.7.3, installer 0.8.0`). A rerun replaces only files that are unchanged since the installer wrote them, removes files this version no longer ships (agents, scripts, references, tests left inside the skill directory) when unchanged, and refuses, naming the file, when one was modified; `--overwrite` moves such a file to `<folder>/.backup/<UTC time>/<path>` and installs over it. It never writes over a file it does not own. A folder vendored by hand before the lock file existed is migrated once: a file is owned when its bytes match what some earlier plan-anchor version shipped. Every check runs before anything is written; `--dry-run` prints the plan (writes, replacements, removals, settings) and writes nothing. Output is the JSON envelope `{success, data, error}`; exit 0 when the command ran, 2 on any error. The installer plugin's SessionStart hook prints one line when the nearest install is older than the plugin, and stays silent otherwise.

## Anchor tracker selection

The installer picks the CLI that stores anchors and question beads, independently of the plan source, and renders every installed file (prompts and scripts) for that tracker only:

1. An explicit `anchor_tracker` in `.sc/plan-anchor/settings.yaml` wins, checked against disk: `bd` needs `.beads/metadata.json` with `"backend": "dolt"` (else `TRACKER.DOLT_REQUIRED`); `br` needs a `.beads/` that is not Dolt-backed, or none yet (else `TRACKER.MISMATCH`). A mismatch is an error, never a silent switch.
2. With no setting: a Dolt-backed `.beads/` selects `bd`; any other `.beads/` selects `br`; no `.beads/` selects `br`.
3. The selected CLI must be on PATH (`TRACKER.CLI_MISSING`).

Without `--tracker` the installer uses the resolved tracker (the setting, else detection). It writes the chosen tracker to `anchor_tracker` in the settings file. With no `.beads/` it refuses (`TRACKER.NOT_INITIALIZED`) unless `--init-br` is given, which runs `br init`. Dolt provisioning is outside this skill: the installer and the skill never run or recommend `bd init`.

The installed `anchor_tracker.py resolve` knows only the tracker this install is rendered for, `bd`, and applies rules 1 and 3 to it: a setting that names the other tracker is `SETTINGS.INVALID_TRACKER`, and a `.beads/` that is not Dolt-backed, or none, is `TRACKER.DOLT_REQUIRED`, with or without a setting. Each error's suggested action is to rerun the installer with `--tracker` (see Switching trackers); there is never a silent switch.

`resolve` is read-only and never initializes. `write` records the result in the settings file (`--tracker bd` pins it, checked the same way; an existing setting is never rewritten, and a different pin is `SETTINGS.CONFLICT`). Commit what `write` reports in `commit_paths`.

Exit codes follow the plugin convention shared by every script: 0 the command ran (selected); 2 any error, invalid arguments included (`VALIDATION.INPUT`), with the typed error object on stdout. The only exit codes are 0, 2 and 130 (interrupt); a negative answer from a command that ran is exit 0 with the outcome in `data`. Act on the error code (`SETTINGS.*`, `BEADS.*`, `TRACKER.*`, `DEPENDENCY.PYYAML`, `REPO.NOT_FOUND`, `SCRIPT.INTERNAL`), not the exit status. Output is always the JSON envelope `{success, data, error}`; the reported `cli_path` is absolute.

**Settings file.** Synaptic Canvas packages keep settings at `<folder>/.sc/<package>/settings.yaml`: YAML, snake_case keys, committed to git, never secrets. The file in effect is the nearest one at or above the working directory, stopping before the home directory: the repository's own file, else a parent folder's (see Parent-folder installs). `plan_anchor_settings.py set` changes the file in effect (or creates one at the repository root, `git rev-parse --show-toplevel`); `anchor_tracker.py write` records the tracker in the repository's own file. For this package:

```yaml
# .sc/plan-anchor/settings.yaml
# Repository settings for the sc-plan-anchor plugin; commit this file, never put secrets in it.
schema_version: 1
anchor_tracker: bd
# plan_dir: the current phase's plan folder (anchors.json, intent.md, anchor-runs/). Set it again when the phase changes.
plan_dir: "docs/plans/phase-b"
```

`plan_dir` names the current phase folder, `docs/plans/phase-<current>` with the phase id written out: a relative path with forward slashes and no `..` or `<placeholder>` (`SETTINGS.INVALID_PLAN_DIR`). Set it with `plan_anchor_settings.py set plan_dir docs/plans/phase-<id>`; the installer and `anchor_tracker.py write` keep it. `schema_version` must be `1`. Unknown or duplicate keys are rejected (`SETTINGS.UNKNOWN_KEY`, `SETTINGS.MALFORMED`) so a typo never falls back silently. When the file or its `anchor_tracker` key is absent, the installer detects (rule 2) and the installed `anchor_tracker.py` selects `bd`, checked against disk. `.gitignore` may ignore `.sc/sessions/` and `.sc/*/cache/`, never this file; `write` reports `settings_ignored_by_git` when it is.

This install is rendered for `bd`; both gates read anchor and question beads with it. The sprint text comes from the caller, from wherever it wrote it (`item_file`).

## Switching trackers

Rerun the installer for the install folder with the other `--tracker` value. It checks the request against disk with the rules above, re-renders every installed copy (all targets in the lock file) for that tracker, and updates `anchor_tracker` in `.sc/plan-anchor/settings.yaml`. Switching does not migrate issue data: anchors, question beads and reports already in the old tracker stay there; recreate the anchor tree in the new one (`references/bead-layout.md`) and update the bead IDs in `anchors.json` with `set-beads`. The switched folder is byte-for-byte what a fresh install with that `--tracker` writes into an empty folder, the lock and settings files included, in either direction. Commit the settings file, the lock file and the re-rendered files together.

## Install the CLIs

**python3** (3.9 or newer; the scripts are standard-library only, except `anchor_tracker.py`, which needs PyYAML)

- macOS: `brew install python` or the Xcode command line tools' `/usr/bin/python3`
- Linux: `apt install python3` / `dnf install python3`
- Windows: `winget install Python.Python.3.12`; use forward slashes in paths

**PyYAML** (5.1 or newer; read only by `anchor_tracker.py` for `.sc/plan-anchor/settings.yaml`)

- `python3 -m pip install pyyaml` (into the interpreter the skill runs). Without it `anchor_tracker.py` returns `DEPENDENCY.PYYAML` and exits 2; it never falls back to defaults.

**bd (beads)**, only for an existing Dolt-backed workspace

- bd is supported only where a Dolt-backed `.beads/` already exists (`.beads/metadata.json` has `"backend": "dolt"`). On these systems bd needs an upstream Dolt (DoltHub or a Dolt server); provisioning one is outside this skill, so never run `bd init` for plan anchoring.

- Install: `brew install steveyegge/beads/bd` (or your team's tap), or with Go `go install github.com/steveyegge/beads/cmd/bd@latest` and `$HOME/go/bin` on PATH.

**sc-compose** (1.7.0 or newer)

- Install it from the Synaptic Canvas `sc-compose` release or team package
  source, then confirm `sc-compose --version` reports at least 1.7.0.
- The skill uses strict checked-in Jinja2 templates for every anchor-tuning
  request/result. Do not continue with hand-built JSON when this CLI is absent.

**TypeSafe API key**

The live judge looks for the key in this order: `TYPESAFE_API_KEY` in the process environment, then the platform credential store. An invalid environment value fails closed; it does not silently select another credential. The platform entry is local-interactive convenience only—cloud, CI, and remote agents should use their platform secret injection.

- macOS: Keychain generic-password item with service `plan-anchor` and account `typesafe-api-key`.
- Windows: Credential Manager generic credential with target `plan-anchor:typesafe-api-key`.
- Linux: Secret Service item returned by `secret-tool lookup service plan-anchor account typesafe-api-key`. Create it without putting the key on a command line with `secret-tool store --label="plan-anchor TypeSafe API key" service plan-anchor account typesafe-api-key` and enter the value on standard input.

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
- Cursor's agent shell uses the environment of the Cursor app. A GUI launch does not read shell rc files, so set `TYPESAFE_API_KEY` with `launchctl setenv` (macOS, until reboot) before starting Cursor. A plugin variable does not export into `python3 jev_judge.py`.
- The live rubric, tuning, fixture checks, and gates also need the user's committed answer `allow_remote_judge: true` in `.sc/plan-anchor/settings.yaml` (`jev-api.md`). Record it once, at install time with `--allow-remote-judge` (or `--no-remote-judge`), or later with `python3 <skill>/scripts/plan_anchor_settings.py set allow_remote_judge true|false`, and commit the file. The normal skill workflow requires Jev; offline commands are diagnostics and CI support.

## Find an existing install

Claude Code's bash inherits a minimal PATH that can miss Homebrew, Go, pyenv, and `~/.local/bin`:

```bash
for t in bd; do for p in "/opt/homebrew/bin/$t" "$HOME/.cargo/bin/$t" "$HOME/go/bin/$t" "$HOME/.local/bin/$t" "/usr/local/bin/$t"; do [ -x "$p" ] && echo "$t at $p"; done; done
for p in /opt/homebrew/bin/python3 /usr/local/bin/python3 /usr/bin/python3 "$HOME/.pyenv/shims/python3"; do [ -x "$p" ] && echo "python3 at $p"; done
for p in /opt/homebrew/bin/sc-compose "$HOME/.local/bin/sc-compose" /usr/local/bin/sc-compose; do [ -x "$p" ] && echo "sc-compose at $p"; done
for p in /opt/homebrew/bin/jq /usr/bin/jq /usr/local/bin/jq; do [ -x "$p" ] && echo "jq at $p"; done
```

Use the full path, or `export PATH="/opt/homebrew/bin:$PATH"` for the session.

## Minimum versions

| Tool | Minimum | Why |
| --- | --- | --- |
| python3 | 3.9 | tested floor; standard library only, except `anchor_tracker.py` |
| PyYAML | 5.1 | `SafeLoader`-based strict reading of `.sc/plan-anchor/settings.yaml`; CI tests 5.1 and the latest release |
| bd | 1.x, Dolt-backed `.beads/` only | `--parent`, `--labels`, `--silent`, `--body-file`, `show --json`, `children --json`, `comment --file`, `ready --exclude-label` |
| sc-compose | 1.7.0 | strict render, declared-variable validation, and `--check-render` |
| jq | any current release | gate agents read question-override beads |
| Jev | `jev-latest` | pin a version once thresholds are tuned |

## PATH troubleshooting

- A tool works in your terminal but not in the skill: Claude Code's bash did not source the shell init that added it to PATH. Export the directory in the session or use the full path.
- `sc-compose` is missing: install 1.7.0 or newer and rerun the dependency
  gate; do not hand-assemble the fenced tuning payload.
- `bd show --json` errors with "no database": run from the repo root that has `.beads/`, or pass `--db <path>` or `-C <repo root>`.
- The key works in your terminal but the skill says it is missing: it was exported after Claude Code started, or only in an interactive rc file. Put it in `~/.zshenv`; restart Claude Code.

## Validation

```bash
S="<skill dir>/scripts/jev_judge.py"; X="<skill dir>/assets/examples"
python3 "$S" --help
python3 "$S" validate --anchors "$X/anchors.example.json"
python3 "$S" judge --anchors "$X/anchors.example.json" --gate plan --all --item-id smoke --item-file "<skill dir>/SKILL.md" \
  --answers-file "$X/answers.fixture.json" --out /tmp/anchor-smoke.json | jq '{exit_ok: .success, pass: .data.summary.pass, failing: .data.summary.failing}'
python3 "$S" report --run /tmp/anchor-smoke.json --format md
# Maintainer source checkout only; the marketplace plugin excludes this suite.
python3 -m unittest discover -s "<repository root>/tests/sc-plan-anchor"
```

The smoke `judge` runs the full gating path offline: expect `not_addressed` for ANC-012, `not_applicable` for REQ-014, and **exit 0 with `data.summary.pass: false`**, because the gate fails by design; a failing gate is a completed run, not a tool failure. `validate` exits 0 with `data.valid: true`. In a maintainer source checkout, the repository-level test suite reports `OK`; it is not included in marketplace installations.

## Known issues

- `POLICY.REMOTE_JUDGE_DISABLED` on a live run: `allow_remote_judge` is `false` or absent in `.sc/plan-anchor/settings.yaml`. If absent, ask the user once, record the answer with `plan_anchor_settings.py set allow_remote_judge true|false`, and commit it. Never infer consent from earlier runs.
- `AUTH.CREDENTIALS` with the key set: it contains whitespace or a control character (usually a pasted trailing newline), or TypeSafe rejected it.
- `QUESTIONS.ROLES`: a question override renamed a question ID. Keep the default IDs.
- `ANCHOR.NOT_LOCKED`: `--anchor` named a proposed or retired anchor. Lock it, or use `--all`.
- `JEV.CONTEXT_EXCEEDED`: the item could not be shortened below 2,000 characters and still fit. Shorten the evidence account; at the development gate drop the test section first.
- `JEV.RESPONSE_INVALID` repeatedly: TypeSafe changed the response shape or a proxy is mangling it. Check for a proxy between you and `api.typesafe.ai`.
- `VALIDATION.ANCHORS` from `judge`: run `validate` and fix the listed problems; the usual cause is a hand edit. Use `update`, `retire` and `promote`.
- `JEV.RATE_LIMITED` under parallel gates: cap concurrency at 3 beads; the script already retries three times with backoff.
- Dates in reports show as "undated": `stated_at` is not an ISO date. Use `YYYY-MM-DD`.
- `bd comment --body-file`: does not exist; use `--file` (or `--stdin`).
- `TRACKER.MISMATCH`, `TRACKER.DOLT_REQUIRED`: the settings file in effect (possibly a parent folder's) and the repository's `.beads/` disagree. Dolt provisioning is outside this skill; give the repository its own `.sc/plan-anchor/settings.yaml`, or rerun the installer with the `--tracker` that matches it.
- `TRACKER.CLI_MISSING`: the selected tracker is not on PATH; see Find an existing install.
- `SETTINGS.UNKNOWN_KEY`, `SETTINGS.SCHEMA_VERSION`, `SETTINGS.INVALID_TRACKER`, `SETTINGS.INVALID_PLAN_DIR`, `SETTINGS.MALFORMED`: fix `.sc/plan-anchor/settings.yaml` to the form above.
- `BEADS.METADATA_INVALID`: `.beads/metadata.json` is not a readable JSON object; repair it before selection can run.
- Installer `INSTALL.DEST_GLOBAL`: `--dest` named the home directory, a global harness directory or a plugin cache; pass a repository or a parent folder of repositories.
- `VALIDATION.PATH` from a gate with plan-anchor installed: the script passed is not the nearest per-folder copy (or is a global one); pass the `jev_judge.py` of the install at or above the repository.
- Installer `INSTALL.CONFLICT`: `data.problems` names each file that was modified since it was installed or is not the installer's; restore or move it, or rerun with `--overwrite`.
- Windows: run `bd` and the script from Git Bash or PowerShell with forward slashes; report short-date formatting falls back gracefully.
