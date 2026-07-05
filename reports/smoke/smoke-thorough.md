# Smoke Thorough

- status: `passed`
- timestamp: `2026-07-04T02:35:58.551642+00:00`
- binary SHA: `5a964add3b9a05283239809fd45fee5ba380807a`
- duration secs: `2.711`
- summary: `pass=9`, `fail=0`, `skip=0`

| Row | Flow | Verdict | Notes |
| --- | --- | --- | --- |
| `AD11-ENV-001` | env-only caller context for retained command surfaces | `PASS` | env-only caller context succeeds for retained CLI surfaces that require it, and daemon-independent retained command paths stay operational |
| `AD11-OVERRIDE-001` | explicit CLI caller-context overrides win when supported | `PASS` | commands with retained override surfaces stay bound to explicit CLI caller context instead of ambient environment values |
| `AD11-LOCAL-001` | caller-context failures stay local before retained execution | `PASS` | missing caller identity or caller team still fails at CLI entry instead of guessing or dispatching into retained execution |
| `AD11-DOCTOR-001` | doctor remains identity-free and optional-team scoped | `PASS` | doctor still executes without caller identity or caller team while preserving explicit team scoping |
| `AD11-POSTSEND-001` | local tmux post-send and sender-visible warning fallback | `PASS` | local tmux nudges still use authoritative pane metadata and forced emission failure still degrades into sender-visible warning behavior |
| `AD11-XREPO-001` | sender roster home_dir governs post-send config lookup across repos | `PASS` | post-send config discovery remains anchored to sender roster metadata rather than ambient caller cwd, preserving cross-repo local-send behavior |
| `AD11-GRAFT-001` | graft-backed post-send emission path remains optional and explicit | `PASS` | the graft-backed emission seam delegates through the dedicated graft port and surfaces failure without leaking graft ownership into the core send path |
| `AD11-AUTH-001` | update-member auth checks and infallible add-member projection are closed | `PASS` | the promoted AD.9 auth and infallible findings are closed: update-member consumes caller context materially, and add-member projection no longer pretends to fail |
| `AD11-READINESS-001` | phase-ad readiness and boundary artifacts fail closed | `PASS` | Phase AD readiness records, smoke artifacts, and PostSendHookEmitter boundary inventory are all present and wired into the retained validation gate |
