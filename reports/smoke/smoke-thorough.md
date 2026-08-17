# Smoke Thorough

- status: `passed`
- timestamp: `2026-08-05T17:39:39.502266+00:00`
- binary SHA: `cfd606bae7a5073ee9ad7ebe6f5ffbc3088e041e`
- duration secs: `7.705`
- summary: `pass=32`, `fail=0`, `skip=0`
- row semantics: `PASS` means every command in the row exited `0`; `FAIL`
  records the first failing command only and does not claim sibling commands in
  that row were executed after the failure

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
| `AD11-GRAFT-001` | graft-backed post-send uses a direct same-host receiver socket with typed warning fallback | `PASS` | the graft-backed emission seam performs one bounded same-host receiver delivery attempt and still surfaces typed sender warnings when the receiver path is unavailable |
| `GRAFT-001` | real same-host atm-graft host registers, consumes an advisory nudge, and completes unary read/ack/send on the shared daemon contract | `PASS` | the real same-host atm-graft host lane succeeded end-to-end on the shared daemon contract |
| `AD11-AUTH-001` | update-member auth checks and infallible add-member projection are closed | `PASS` | the promoted AD.9 auth and infallible findings are closed: update-member consumes caller context materially, and add-member projection no longer pretends to fail |
| `AD11-READINESS-001` | phase-ad readiness and boundary artifacts fail closed | `PASS` | Phase AD readiness records, smoke artifacts, and PostSendHookEmitter boundary inventory are all present and wired into the retained validation gate |
| `AD17-ULID-001` | retained ATM message identity stays ULID-only on the accepted line | `PASS` | ULID-only message identity remains enforced in the retained SQLite mailbox state |
| `AD17-READ-001` | read mutation and contains filtering stay self-consistent on the durable store-backed path | `PASS` | read mutation still reports the post-mutation state and contains filtering still sees the durable full-body projection |
| `AD17-CI-001` | windows CI retains the explicit atm-daemon lane on the accepted line | `PASS` | the explicit atm-daemon CI lane remains present and the Windows skip guard is absent |
| `AD18-RUNTIME-ROOT-001` | shared-host raw CLI bootstrap reuses a single daemon and keeps runtime state under the accepted ATM_HOME root | `PASS` | shared-host smoke proves multi-workspace raw CLI bootstrap reuses one daemon, preserves team isolation, and keeps runtime ownership under the accepted ATM_HOME root |
| `AD19-READ-OUTPUT-001` | read mutation returns the message it actually mutated together with post-mutation bucket counts | `PASS` | read returns the durable message it actually mutated, reports post-mutation unread counts, and leaves ack mutation semantics intact |
| `AD20-READ-CONTAINS-001` | metadata-backed contains stays full-body correct while keeping durable-body reload bounded | `PASS` | metadata-backed contains stays full-body correct and only reloads durable body for surviving summary-miss candidates |
| `AD29-POSTSEND-EXTERNAL-001` | external post-send hook success suppresses built-in fallback while preserving durable send success | `PASS` | external post-send hook success keeps the built-in nudge path inactive while durable send success remains intact |
| `AD29-POSTSEND-PARTIAL-001` | mixed post-send hook outcomes preserve durable delivery while surfacing sender-visible warnings | `PASS` | mixed hook accounting preserves durable delivery success and retains a sender-visible warning for failed matches |
| `AD29-POSTSEND-BUILTIN-001` | built-in fallback covers both tmux and graft recipients when no external hook matches | `PASS` | built-in fallback stays honest for both tmux-backed and graft-backed recipients when no external hook matches |
| `AD29-POSTSEND-RESET-001` | deleting a prior override row restores the built-in default template path | `PASS` | removing a stored override row re-exposes the built-in default template instead of leaving an implicit disabled state behind |
| `AD29-POSTSEND-DISABLE-001` | explicitly disabled built-in template state skips local post-send delivery cleanly | `PASS` | the explicit disabled-template state becomes a documented no-delivery path instead of an accidental empty-string side effect |
