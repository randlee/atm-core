# AI.52 Windows smoke evidence

This tracked snapshot preserves the generated evidence from the dedicated
Windows `atm` account and isolated AI.52 worktree. The runner's timestamped
outputs are normally ignored; their generated contents are retained here (with
repository line-ending normalization) so the sprint's local smoke precondition
is reviewable in the PR.

| Command | Evidence | Result |
| --- | --- | --- |
| `just smoke` | `normal.json`, `normal.md` | PASS, 28 rows |
| `just smoke localhost` | `localhost.json`, `localhost.xhtml`, `localhost.html`, `localhost-index.html` | PASS, 10 live attempts |
| `just smoke local-ip` | `local-ip.json`, `local-ip.xhtml`, `local-ip.html`, `local-ip-index.html` | PASS, 10 live attempts |
| `atm doctor --json` | `localhost.json` and `local-ip.json` `doctor` cases | PASS on every live attempt |
| `just test` | Windows rerun: 396 tests, 2 skipped | PASS (exit 0) |

The live artifacts identify host `FastPC4`, advertised address `10.10.100.98`,
and matched release version `1.4.0-beta-ai.38`. They contain exact message IDs
for send/read, requires-ack, and acknowledgement-reply checks.
