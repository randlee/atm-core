# BA.6 — Documentation, CLAUDE.md, ADR index

| Field | Value |
| --- | --- |
| Design | whole document (commit `18db5acc3`) |
| Recommended | Cipher-311d / fast |
| Depends on | `must_follow` BA.1–BA.5 (PR completion) |
| Worktree | `docs/ba6-task-nudge-documentation` off `integrate/phase-ba` |
| Governed interfaces | none |

## Tasks

1. Add `atm task assign|close|move|list|events` and the aliases to the quick reference — `CLAUDE.md:227` (see D1).
2. Rewrite the closing protocol to `atm task close <task-id> <outcome>` — `docs/team-protocol.md:11` (see D2).
3. Update the doctor remediation literal — `docs/requirements.md:2482` (see D3).
4. Document the `1.5.0` and `1.6.0` HTTP surface with a version-history table — `docs/atm-daemon/http-api.md` (see D4).
5. Reconcile ADR-062 and ADR-061 version records — `docs/adr/` (see D5).
6. Add the Phase BA section — `docs/project-plan.md` (see D6).
7. Reconcile plan §2 decisions and §7 additions against the integrated diff — `docs/plans/phase-ba/phase-ba-plan.md` (see D7).
8. Add the task-state table to Task Storage — `docs/architecture.md` (see D8).
9. Repoint `atm list --tasks` references — `docs/plans/phase-ax/*` (see "Paths to delete").

## Deliverables

- [ ] D1 — `CLAUDE.md:227` (resend assignment via `atm send`) and the ATM
  quick-reference table: add `atm task assign|close|move|list|events`; state
  the aliases; state "use `atm queue` for anything that must not interrupt
  the current task".
- [ ] D2 — `docs/team-protocol.md:11`: closing = `atm task close <task-id>
  completed --stdin` (positional outcome, design §5 / BA.4; `reason` is the
  optional third positional for `refused` / `cancelled`) or the alias
  `atm send <assigner> --task-id <task-id> --task-complete --stdin`; add the
  refused/cancelled paragraph; add "an ack never changes task state". No
  example names a `reassigned` close outcome; reassignment is an existing-id
  assign.
- [ ] D3 — `docs/requirements.md:2482` doctor remediation literal → the
  string the shipped code emits.
- [ ] D4 — `docs/atm-daemon/http-api.md`: the final `1.6.0` surface —
  `task_op` on write requests and the `TaskRow` wire additions (1.5.0, BA.2),
  `TaskMove` request/response (1.6.0, BA.4), peer-ingress rejection of task
  ops — with a version-history table listing both minor bumps.
- [ ] D5 — ADR-062 Phase-BA amendment: replace "R1 pending" wording with the
  recorded decision; mark the blocked-episode cooldown paragraph superseded
  (BA.3 deleted `BLOCKED_RENOTIFY_MS`); ADR-061 version records: SQLite
  MAJOR row (R0 approval — D6 entry + comment URL), HTTP `1.5.0` row (BA.2)
  **and** HTTP `1.6.0` row (BA.4).
- [ ] D6 — `docs/project-plan.md`: Phase BA section (six sprints, status),
  AZ section already marked retired.
- [ ] D7 — `docs/plans/phase-ba/phase-ba-plan.md` §2: each R-row gets its
  "decided: … (comment URL)" line; §7 additions table reconciled against
  `git diff develop...integrate/phase-ba --stat` (phase acceptance 10).
- [ ] D8 — `docs/architecture.md` Task Storage: the plan §1 task-state table,
  one paragraph each for the disposition function and the ephemeral item.

## Paths to delete

- `docs/plans/phase-ax/*` references to `atm list --tasks` → pointer to
  `atm task list` (one-line edit each; history otherwise untouched)

## Tests

- `just lint-docs` (doc-lint path) passes.
- `crates/atm/tests/cli_surface_docs.rs` (existing surface-dump test, if
  present; else add): every `atm task` subcommand and both aliases appear in
  `CLAUDE.md` and `docs/team-protocol.md`; every fenced `atm task …` line in
  those two files parses through `Cli::try_parse_from`; the HTTP version in
  `docs/atm-daemon/http-api.md` equals `HTTP_API_VERSION`.
- Link check on the three amended ADRs.

## Acceptance criteria

1. `grep -rn "atm list --tasks\|--task-complete <\|task close .* --outcome" CLAUDE.md docs/*.md` → nothing.
2. The `cli_surface_docs.rs` test above passes.
3. `grep -c '^| \*\*R[0-9]\*\* ' docs/plans/phase-ba/phase-ba-plan.md` → 7, and `grep -n 'ab44564bc' docs/plans/phase-ba/phase-ba-plan.md` finds the R0 row.
4. `quality-mgr` Final Quality Report posted on the phase PR.

## Required validation

`just lint-docs`; the phase-ending critical review (five reviewers on the
integrate head) runs after this sprint merges.
