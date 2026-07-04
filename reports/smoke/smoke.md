# Smoke

- status: `passed`
- timestamp: `2026-07-04T19:53:49.763640+00:00`
- binary SHA: `102026285f45ba8685a8b787379c2769753add0d`
- duration secs: `10.37`
- summary: `pass=16`, `fail=0`, `skip=0`

| Row | Flow | Verdict | Notes |
| --- | --- | --- | --- |
| `AD11-SEND-ENV-001` | send command uses environment caller context when overrides are absent | `PASS` | send caller-context resolution stays bound to environment when explicit overrides are absent |
| `AD11-READ-ENV-001` | read command uses environment caller context when overrides are absent | `PASS` | read caller-context resolution stays bound to environment when explicit overrides are absent |
| `AD11-MEMBERS-ENV-001` | members command remains daemon-independent under environment-only caller context | `PASS` | members remains daemon-independent while using the shared caller-context resolver |
| `AD11-TEAMS-ENV-001` | teams command remains daemon-independent under environment-only caller context | `PASS` | teams remains daemon-independent while using the shared caller-context resolver |
| `AD11-LOG-ENV-001` | log command remains daemon-independent under environment-only caller context | `PASS` | log remains daemon-independent while using the shared caller-context resolver |
| `AD11-DOCTOR-TEAM-001` | doctor preserves optional team override without caller identity | `PASS` | doctor preserves optional team scoping without caller identity |
| `AD11-DOCTOR-DIRECT-001` | doctor still executes the direct local path without caller identity | `PASS` | doctor still executes the direct local path without caller identity |
| `AD11-POSTSEND-PANE-001` | local tmux post-send requires and uses authoritative pane metadata | `PASS` | local tmux post-send remains bound to authoritative roster pane metadata |
| `AD11-POSTSEND-WARN-001` | sender-visible warning fallback survives failed post-send emission | `PASS` | failed post-send emission still degrades into a sender-visible warning after durable send success |
| `AD11-SEND-OVERRIDE-001` | send command prefers explicit CLI caller-context overrides over environment values | `PASS` | send remains bound to explicit CLI caller context when provided |
| `AD11-READ-OVERRIDE-001` | read command prefers explicit CLI caller-context overrides over environment values | `PASS` | read remains bound to explicit CLI caller context when provided |
| `AD11-MEMBERS-OVERRIDE-001` | members command preserves explicit team override | `PASS` | members preserves explicit CLI team override instead of ambient environment values |
| `AD11-UPDATE-MEMBER-IDENTITY-LOCAL-001` | update-member fails locally when caller identity is unavailable | `PASS` | update-member rejects missing caller identity locally before any retained execution |
| `AD11-UPDATE-MEMBER-TEAM-LOCAL-001` | update-member fails locally when caller team is unavailable | `PASS` | update-member rejects missing caller team locally before any retained execution |
| `AD11-LOG-LOCAL-001` | log command fails locally when caller context is unavailable | `PASS` | log fails at CLI entry instead of guessing or dispatching into retained execution |
| `AD11-ROSTER-REPAIR-001` | fixture evidence preserves repaired pane metadata through team-admin and doctor projections | `PASS` | fixture-backed smoke evidence proves pane repair survives the accepted team-admin and doctor projection paths |
