# Installation and Troubleshooting

`atm-bd-orchestration` verifies every command-line requirement in its manifest
before an install writes anything:

| CLI | Minimum | Install | Used for |
| --- | --- | --- | --- |
| `uv` | 0.4 | `brew install uv` | runs every package script under Python 3.11 with pydantic, PyYAML and the sc-compose wheel (the scripts' PEP 723 inline metadata); the `python3` on PATH is never used |
| `bd` | 1.3.0 | `brew install beads` | the plan, the work graph, `bd sync` |
| `atm` | 1.6.3 | `brew install randlee/tap/atm` | tasks, messages, `atm compose`, `atm hook`, `atm cron` |
| `sc-compose` | 1.7.0 (below 2), CLI the same version as the wheel `uv run --script` resolves | `brew install randlee/tap/sc-compose`; `uv cache clean sc-compose` refreshes a stale wheel | strict renders of bead and reviewer templates (CLI), the scripts' renders (wheel) |
| `jq` | 1.6 | `brew install jq` | every bead query |
| `git` | 2.27 | `brew install git` | repository and stack commands |
| `atm-bd` | 0.1.0 | `uv tool install git+https://github.com/randlee/atm-bd-orchestration.git#subdirectory=apps/atm-bd-console` | phase dashboard and cron |

`gh` plus gh-stack remain required for lead-only stacked-PR work, but are not
part of the installer's manifest preflight.

## Verify

```bash
for c in uv bd atm sc-compose jq git atm-bd; do command -v "$c" >/dev/null && echo "ok $c" || echo "MISSING $c"; done
uv --version; bd --version; atm --version; sc-compose --version; jq --version; git --version; atm-bd --version
gh stack --version    # lead only, not checked by the installer
```

If a CLI is missing from `PATH`, look in the usual locations; Claude Code's
shell may not share the interactive shell's `PATH`:

```bash
for p in /opt/homebrew/bin "$HOME/.cargo/bin" "$HOME/.local/bin"; do
  ls "$p"/{bd,atm,sc-compose,jq,git,atm-bd,gh} 2>/dev/null
done
```

When one is found there, `export PATH="<dir>:$PATH"` for the session. If a
CLI is not installed or is older than the minimum, stop and tell the user
which one, with its install line above. Do not work around a missing CLI.

## Common Failures

| Symptom | Cause | Fix |
| --- | --- | --- |
| `bd version` is below 1.3.0 | an old `bd` is first on `PATH` | `brew upgrade beads`, then `hash -r` |
| `validate-plan` warns: "bd doctor produced no JSON" | `bd doctor` cannot run in this beads mode (its stderr follows the message, for example `proxy.doctor.unsupported` in proxied-server mode) | run beads in server mode; report the stderr to the lead |
| `validate-plan` exits 2: "origin/<branch>:<plan> is missing" | the plan file is not pushed to the phase's integration branch | push the plan file and `.atm-bd/phase-<x>.toml` to it, or pass `--index` |
| `validate-plan` exits 5: "metadata.integration_branch is …; .atm-bd/phase-<x>.toml names …" | the phase root's integration branch is missing or differs from the phase file | `bd update <root> --set-metadata integration_branch=<branch>` |
| `bd doctor` reports an error | the beads database is unhealthy | report the check to the user; never import into or dispatch from it |
| `sc-compose render` rejects `autoescape` or `}}}` | an XML or markdown template given to `sc-compose` | render those with `atm compose`; `sc-compose` is for the JSON templates |
| `gh stack` hangs | an interactive form of the command | always pass branch names, `--auto`, `--json`, `--yes` (see the `sc-gh-stack` skill) |

Identity problems (`BEADS_ACTOR`, assignees) are in
[`../resources/troubleshooting.md`](../resources/troubleshooting.md).
