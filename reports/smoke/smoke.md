# Smoke

- status: `passed`
- timestamp: `2026-07-04T20:30:18.153263+00:00`
- binary SHA: `cf3b3c732f9c53111467a45a3c791d98387bad3c`
- duration secs: `7.932`
- summary: `pass=16`, `fail=0`, `skip=0`

| Row | Flow | Verdict | Notes |
| --- | --- | --- | --- |
| `AD11-CMD-SEND-001` | send command preserves caller-context ownership across environment and explicit override paths | `PASS` | send stays bound to the shared caller-context contract across environment and explicit override paths |
| `AD11-CMD-READ-001` | read command preserves caller-context ownership across environment and explicit override paths | `PASS` | read stays bound to the shared caller-context contract across environment and explicit override paths |
| `AD11-CMD-MEMBERS-001` | members command remains daemon-independent while preserving explicit team override handling | `PASS` | members remains daemon-independent while preserving retained caller-team semantics |
| `AD11-CMD-TEAMS-001` | teams list command remains daemon-independent on the retained CLI surface | `PASS` | teams list remains daemon-independent on the retained CLI surface |
| `AD11-CMD-LOG-001` | log command remains daemon-independent with caller-context enforcement at the CLI boundary | `PASS` | log remains daemon-independent and still fails locally when caller context is unavailable |
| `AD11-CMD-DOCTOR-001` | doctor remains identity-free while preserving optional team scoping and the direct local path | `PASS` | doctor remains identity-free while preserving optional team scoping and the direct local path |
| `AD11-POSTSEND-LOCAL-TMUX-001` | local tmux post-send requires and uses authoritative pane metadata | `PASS` | local tmux post-send remains bound to authoritative roster pane metadata |
| `AD11-POSTSEND-WARNING-001` | sender-visible warning fallback survives failed post-send emission | `PASS` | failed post-send emission still degrades into a sender-visible warning after durable send success |
| `AD11-CMD-ACK-001` | ack command preserves caller-context ownership across environment and explicit override paths | `PASS` | ack stays bound to the shared caller-context contract across environment and explicit override paths |
| `AD11-CMD-LIST-001` | list command preserves retained filters while keeping caller-context ownership explicit | `PASS` | list preserves retained filters while staying bound to explicit caller-context ownership |
| `AD11-CMD-CLEAR-001` | clear command preserves caller-context ownership across environment and explicit override paths | `PASS` | clear stays bound to the shared caller-context contract across environment and explicit override paths |
| `AD11-CMD-TEAMS-ADD-MEMBER-001` | teams add-member preserves the retained home-dir payload contract | `PASS` | teams add-member preserves the retained home-dir payload contract |
| `AD11-CMD-TEAMS-UPDATE-MEMBER-001` | teams update-member preserves caller context and fails locally when mandatory caller context is missing | `PASS` | teams update-member preserves caller context and still fails locally when mandatory caller context is missing |
| `AD11-CMD-TEAMS-BACKUP-001` | teams backup preserves retained team scoping and remains daemon-independent in dry-run execution | `PASS` | teams backup preserves retained team scoping and remains daemon-independent in dry-run execution |
| `AD11-CMD-TEAMS-RESTORE-001` | teams restore preserves retained path and dry-run behavior without requiring the daemon | `PASS` | teams restore preserves retained path and dry-run behavior without requiring the daemon |
| `AD11-ROSTER-REPAIR-001` | fixture evidence preserves repaired pane metadata through team-admin and doctor projections | `PASS` | fixture-backed smoke evidence proves pane repair survives the accepted team-admin and doctor projection paths |
