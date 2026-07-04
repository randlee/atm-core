# Smoke Thorough

- status: `passed`
- timestamp: `2026-07-04T20:30:29.103331+00:00`
- binary SHA: `cf3b3c732f9c53111467a45a3c791d98387bad3c`
- duration secs: `4.77`
- summary: `pass=20`, `fail=0`, `skip=0`

| Row | Flow | Verdict | Notes |
| --- | --- | --- | --- |
| `AD11-CMD-SEND-001` | send command preserves caller-context ownership across environment and explicit override paths | `PASS` | send stays bound to the shared caller-context contract across environment and explicit override paths |
| `AD11-CMD-READ-001` | read command preserves caller-context ownership across environment and explicit override paths | `PASS` | read stays bound to the shared caller-context contract across environment and explicit override paths |
| `AD11-CMD-ACK-001` | ack command preserves caller-context ownership across environment and explicit override paths | `PASS` | ack stays bound to the shared caller-context contract across environment and explicit override paths |
| `AD11-CMD-LIST-001` | list command preserves retained filters while keeping caller-context ownership explicit | `PASS` | list preserves retained filters while staying bound to explicit caller-context ownership |
| `AD11-CMD-CLEAR-001` | clear command preserves caller-context ownership across environment and explicit override paths | `PASS` | clear stays bound to the shared caller-context contract across environment and explicit override paths |
| `AD11-CMD-LOG-001` | log command remains daemon-independent with caller-context enforcement at the CLI boundary | `PASS` | log remains daemon-independent and still fails locally when caller context is unavailable |
| `AD11-CMD-MEMBERS-001` | members command remains daemon-independent while preserving explicit team override handling | `PASS` | members remains daemon-independent while preserving retained caller-team semantics |
| `AD11-CMD-TEAMS-001` | teams list command remains daemon-independent on the retained CLI surface | `PASS` | teams list remains daemon-independent on the retained CLI surface |
| `AD11-CMD-TEAMS-ADD-MEMBER-001` | teams add-member preserves the retained home-dir payload contract | `PASS` | teams add-member preserves the retained home-dir payload contract |
| `AD11-CMD-TEAMS-UPDATE-MEMBER-001` | teams update-member preserves caller context and fails locally when mandatory caller context is missing | `PASS` | teams update-member preserves caller context and still fails locally when mandatory caller context is missing |
| `AD11-CMD-TEAMS-BACKUP-001` | teams backup preserves retained team scoping and remains daemon-independent in dry-run execution | `PASS` | teams backup preserves retained team scoping and remains daemon-independent in dry-run execution |
| `AD11-CMD-TEAMS-RESTORE-001` | teams restore preserves retained path and dry-run behavior without requiring the daemon | `PASS` | teams restore preserves retained path and dry-run behavior without requiring the daemon |
| `AD11-CMD-DOCTOR-001` | doctor remains identity-free while preserving optional team scoping and the direct local path | `PASS` | doctor remains identity-free while preserving optional team scoping and the direct local path |
| `AD11-POSTSEND-LOCAL-TMUX-001` | local tmux post-send requires and uses authoritative pane metadata | `PASS` | local tmux post-send remains bound to authoritative roster pane metadata |
| `AD11-POSTSEND-WARNING-001` | sender-visible warning fallback survives failed post-send emission | `PASS` | failed post-send emission still degrades into a sender-visible warning after durable send success |
| `AD11-ROSTER-REPAIR-001` | fixture evidence preserves repaired pane metadata through team-admin and doctor projections | `PASS` | fixture-backed smoke evidence proves pane repair survives the accepted team-admin and doctor projection paths |
| `AD11-XREPO-001` | sender roster home_dir governs post-send config lookup across repos | `PASS` | post-send config discovery remains anchored to sender roster metadata rather than ambient caller cwd, preserving cross-repo local-send behavior |
| `AD11-GRAFT-001` | graft-backed post-send emission path remains optional and explicit | `PASS` | the graft-backed emission seam delegates through the dedicated graft port and surfaces failure without leaking graft ownership into the core send path |
| `AD11-AUTH-001` | update-member auth checks and infallible add-member projection are closed | `PASS` | the promoted AD.9 auth and infallible findings are closed: update-member consumes caller context materially, and add-member projection no longer pretends to fail |
| `AD11-READINESS-001` | phase-ad readiness and boundary artifacts fail closed | `PASS` | Phase AD readiness records, smoke artifacts, and PostSendHookEmitter boundary inventory are all present and wired into the retained validation gate |
