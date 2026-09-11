# BA.6 — Documentation, CLAUDE.md, ADR index

| Field | Value |
| --- | --- |
| Design | whole document (commit `18db5acc3`) |
| Outcomes | B14 (queryability documented); phase acceptance 11, 12 |
| Recommended | Cipher-311d / fast |
| Depends on | `must_follow` BA.1–BA.5 (PR completion) |
| Worktree | `docs/ba6-task-nudge-documentation` off `integrate/phase-ba` |
| Governed interfaces | none |

## Scope

Make the shipped surface the documented surface. The ADR/requirements
alignment that precedes the code (ADR-062/054 amendments, ADR-063
superseded, requirements §6.5/§15.4/§22.1 MUST lists) already landed in the
plan PR; this sprint reconciles those texts with what BA.1–BA.5 actually
shipped and updates the operator-facing docs.

## Deliverables

- [ ] D1 — `CLAUDE.md:227` (resend assignment via `atm send`) and the ATM
  quick-reference table: add `atm task assign|close|move|list|events`; state
  the aliases; state "use `atm queue` for anything that must not interrupt
  the current task".
- [ ] D2 — `docs/team-protocol.md:11`: closing = `atm task close <task-id>
  completed --stdin` (positional outcome, design §5 / BA.4; `reason` is the
  optional third positional for `refused` / `cancelled`) or the alias
  `atm send <assigner> --task-id <task-id> --task-complete --stdin`; add the refused/cancelled/reassigned
  paragraph; add "an ack never changes task state".
- [ ] D3 — `docs/requirements.md:2482` doctor remediation literal → the
  string the shipped code emits; add `TaskQueueGap` to the doctor table.
- [ ] D4 — `docs/atm-daemon/http-api.md`: the final `1.6.0` surface —
  `task_op` on write requests and the `TaskRow` wire additions (1.5.0, BA.2),
  `TaskMove` request/response (1.6.0, BA.4), peer-ingress rejection of task
  ops — with a version-history table listing both minor bumps
  (FNX-BA-CRIT-035).
- [ ] D5 — ADR-062 Phase-BA amendment: replace "R1 pending" wording with the
  recorded decision; mark the blocked-episode cooldown paragraph superseded
  (BA.3 deleted `BLOCKED_RENOTIFY_MS`); ADR-061 version records: SQLite
  MAJOR row (R0 approval — D6 entry + comment URL), HTTP `1.5.0` row (BA.2)
  **and** HTTP `1.6.0` row (BA.4).
- [ ] D6 — `docs/project-plan.md`: Phase BA section (six sprints, status),
  AZ section already marked retired.
- [ ] D7 — `docs/plans/phase-ba/phase-ba-plan.md` §4: each R-row gets its
  "decided: … (comment URL)" line; §9 additions table reconciled against
  `git diff develop...integrate/phase-ba --stat` (phase acceptance 10).
- [ ] D8 — `docs/architecture.md` Task Storage: one paragraph per state
  machine (task, disposition, ephemeral item), each a copy of the plan §3
  table — no prose restatement.

## Paths to delete

- `docs/plans/phase-ax/*` references to `atm list --tasks` → pointer to
  `atm task list` (one-line edit each; history otherwise untouched)

## Tests

- `just lint-docs` (doc-lint path) passes.
- `crates/atm/tests/cli_surface_docs.rs` (existing surface-dump test, if
  present; else add): every `atm task` subcommand and both aliases appear in
  `CLAUDE.md` and `docs/team-protocol.md`.
- Link check on the three amended ADRs.

## Acceptance criteria

1. `grep -rn "atm list --tasks\|--task-complete <\|task close .* --outcome" CLAUDE.md docs/*.md` → nothing.
1a. Every `atm task` example in `CLAUDE.md` and `docs/team-protocol.md` parses:
    a test extracts the fenced `atm task …` lines and runs them through the
    clap parser (`Cli::try_parse_from`) — the full close example included, not
    just the subcommand name (FNX-BA-CRIT-034).
1b. The HTTP version in `docs/atm-daemon/http-api.md` equals `HTTP_API_VERSION`
    at the integrated head (`grep` in the docs test).
2. Every "decided:" line in plan §4 has a URL.
3. `quality-mgr` Final Quality Report posted on the phase PR.

## Required validation

`just lint-docs`; the phase-ending critical review (five reviewers on the
integrate head) runs after this sprint merges.

## Out of scope

Any code change. Findings against code go back to the owning sprint.
