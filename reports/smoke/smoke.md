# Smoke

- status: `passed`
- timestamp: `2026-07-04T02:35:57.946116+00:00`
- binary SHA: `5a964add3b9a05283239809fd45fee5ba380807a`
- duration secs: `2.162`
- summary: `pass=5`, `fail=0`, `skip=0`

| Row | Flow | Verdict | Notes |
| --- | --- | --- | --- |
| `AD11-ENV-001` | env-only caller context for retained command surfaces | `PASS` | env-only caller context succeeds for retained CLI surfaces that require it, and daemon-independent retained command paths stay operational |
| `AD11-DOCTOR-001` | doctor remains identity-free and optional-team scoped | `PASS` | doctor still executes without caller identity or caller team while preserving explicit team scoping |
| `AD11-POSTSEND-001` | local tmux post-send and sender-visible warning fallback | `PASS` | local tmux nudges still use authoritative pane metadata and forced emission failure still degrades into sender-visible warning behavior |
| `AD11-OVERRIDE-001` | explicit CLI caller-context overrides win when supported | `PASS` | commands with retained override surfaces stay bound to explicit CLI caller context instead of ambient environment values |
| `AD11-LOCAL-001` | caller-context failures stay local before retained execution | `PASS` | missing caller identity or caller team still fails at CLI entry instead of guessing or dispatching into retained execution |
