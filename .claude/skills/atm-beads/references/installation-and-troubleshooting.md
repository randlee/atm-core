# Installation and Troubleshooting

Both `atm-beads` and `atm-bd-orchestration` need these CLIs:

| CLI | Minimum | Install | Used for |
| --- | --- | --- | --- |
| `bd` | 1.3.0 | `brew install beads` | the plan, the work graph, `bd sync` |
| `atm` | 1.6.1 | `brew install randlee/tap/atm` | tasks, messages, `atm compose` |
| `sc-compose` | 1.6.1 | `brew install randlee/tap/sc-compose` | strict JSON renders of bead and reviewer templates |
| `jq` | 1.6 | `brew install jq` | every bead query |
| `gh` + gh-stack | gh 2.0, gh-stack 0.1.0 | `brew install gh`, then `gh extension install github/gh-stack` | the phase stack (lead only) |

## Verify

```bash
for c in bd atm sc-compose jq gh; do command -v "$c" >/dev/null && echo "ok $c" || echo "MISSING $c"; done
bd version; atm --version; sc-compose --version
gh stack --version    # lead only
```

If a CLI is missing from `PATH`, look in the usual locations; Claude Code's
shell may not share the interactive shell's `PATH`:

```bash
for p in /opt/homebrew/bin "$HOME/.cargo/bin" "$HOME/.local/bin"; do
  ls "$p"/{bd,atm,sc-compose,gh} 2>/dev/null
done
```

When one is found there, `export PATH="<dir>:$PATH"` for the session. If a
CLI is not installed or is older than the minimum, stop and tell the user
which one, with its install line above. Do not work around a missing CLI.

## Common Failures

| Symptom | Cause | Fix |
| --- | --- | --- |
| `validate-plan` exits 2: "bd … is older than 1.3.0" | an old `bd` is first on `PATH` | `brew upgrade beads`, then `hash -r` |
| `validate-plan` exits 2: "bd doctor produced no JSON" | `bd doctor` cannot run in this beads mode (its stderr follows the message, for example `proxy.doctor.unsupported` in proxied-server mode) | run beads in server mode; report the stderr to the lead |
| `validate-plan` exits 2: "has no metadata.integration_branch" | the phase root was created without its integration branch | `bd update <root> --set-metadata integration_branch=<branch>`, or pass `--index` |
| `bd doctor` reports an error | the beads database is unhealthy | report the check to the user; never import into or dispatch from it |
| `sc-compose render` rejects `autoescape` or `}}}` | an XML or markdown template given to `sc-compose` | render those with `atm compose`; `sc-compose` is for the JSON templates |
| `gh stack` hangs | an interactive form of the command | always pass branch names, `--auto`, `--json`, `--yes` (see the `sc-gh-stack` skill) |

Identity problems (`BEADS_ACTOR`, assignees) are in
[`../resources/troubleshooting.md`](../resources/troubleshooting.md).
