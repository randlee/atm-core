---
phase: AX
sprint: AX.4
title: Task completion and inspection CLI
branch: feature/ax4-task-cli-and-docs
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ax4-task-cli-and-docs
integration_branch: integrate/phase-ax
status: complete
recommended_agent: Cipher-311d
recommended_model: fast
execution_track: B
parallel_with: [AX.1, AX.2]
dependency_relations:
  - prerequisite: AX.3
    dependent: AX.4
    relation: must_follow
    rationale: the flags are thin plumbing over SendRequest::with_task_complete and TaskStore, both delivered by AX.3; stacked on the AX.3 branch.
  - prerequisite: AX.1
    dependent: AX.4
    relation: parallel_safe
    rationale: no functional dependency; both edit docs/requirements.md (AX.1 lines 1094/1105/4496, AX.4 §6.5/§6.6/§15.4) and docs/user-documents/nudge-templates.md (AX.1 rewrites the kind inventory, AX.4 adds one ADR-061 cross-reference) in different sections; resolved by AX.4 merging integrate/phase-ax forward after track A lands and before opening its PR, the same rule AX.3 follows.
  - prerequisite: AX.4
    dependent: AX.5
    relation: must_follow
    rationale: AX.5 starts only after tracks A and B are merged; its acceptance scenarios drive the state machine through these flags.
---

# AX.4 — Task completion and inspection CLI

Expose the AX.3 state machine to operators: the completion flag, the
two list surfaces, and the normative and user documentation. No storage
or state-machine change. Track B, stacked on AX.3, parallel with track
A.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or
shape-only completion fails the sprint.

- [ ] D1 — `atm send --task-complete <TASK_ID>`
  (`crates/atm/src/commands/send.rs`, clap `conflicts_with = "task_id"`),
  mapped to `SendRequest::with_task_complete`. `atm queue` inherits the
  flag through the flattened `SendCommand` in
  `crates/atm/src/commands/queue.rs`; no separate change. `--json`
  output shows `task_complete` (AX.3 C8).
- [ ] D2 — `atm list --tasks [--member <name>]` and
  `atm list --task-events <TASK_ID> [--member <name>]`
  (`crates/atm/src/commands/list.rs`, which owns parsing and rendering),
  reading through `LocalServiceRuntime::task_store()` and importing
  `TaskRow` / `TaskEventRow` from `atm_storage::contract` (the CLI is an
  `allowed_dependent` of the AX.3 task-store boundary record, the same
  sanctioned path `nudge-template-override-store.toml` grants `atm`);
  `conflicts_with_all` against every mailbox filter including the
  existing `--task <id>`; human and `--json` output per code contract C1.
- [ ] D3 — docs: `docs/requirements.md` §6.5 (`atm send` flags) and §6.6
  (`atm list` flags) list the new flags; §15.4 "Task Metadata Rule"
  extended with the state machine summary, the ack gate,
  `--task-complete`, the completion-from-Assigned acknowledgement rule,
  and the audit rule (pointing at ADR-061 for the tables);
  `docs/team-protocol.md` completion step names
  `atm send <assigner> --task-complete <id> --stdin`;
  `docs/user-documents/nudge-templates.md` cross-references ADR-061 from
  the `task` kind; a new `docs/user-documents/tasks.md` documents the
  three states, the two flags, and the two list surfaces with the C1
  examples.
- [ ] D4 — tests listed under Required validation.

### Paths to delete

None.

## Code contracts

### C1 — list output

`atm list --tasks --json`: JSON array of `TaskRow`.
`atm list --task-events <id> --json`: JSON array of `TaskEventRow`.

Human output, `--tasks` (header then one line per row, newest first;
`--member` keeps only that assignee):

```
TASK_ID     STATE     ASSIGNEE  ASSIGNER   ASSIGNED_AT               REMINDERS
t-42        active    cipher    fenix      2026-09-05T10:12:03Z      3
t-43        assigned  cipher    fenix      2026-09-05T10:12:04Z      0
```

Human output, `--task-events t-42` (in `seq` order; `--member` keeps
only that assignee's key):

```
SEQ  AT                        EVENT      FROM      TO        ACTOR    DETAIL
1    2026-09-05T10:12:03Z      assigned   -         assigned  fenix    -
2    2026-09-05T10:12:40Z      acked      assigned  active    cipher   -
3    2026-09-05T10:13:40Z      reminded   active    active    atm-daemon emitted
```

Flag conflicts exit 2 with clap's usage error. An unknown task id on
`--task-events` prints the header only and exits 0.

### Unchanged surfaces

`TaskStore`; the state machine; `atm read` / `atm ack` argument shapes;
every existing `atm list` filter.

## Acceptance criteria

1. After AX.3 AC 1's scenario, `atm list --tasks --json` returns two
   rows with states `active` and `assigned`, and `atm list --task-events
   <first> --json` returns the `assigned`, `acked` rows in `seq` order;
   `atm list --task-events <second>` shows the `rejected` row with the
   G1 detail.
2. `atm send cipher --task-complete t-42 --stdin` from the assigner
   exits 0 and `atm list --tasks` shows `complete`; the same with an
   unknown id exits 3 and writes no message; `--task-complete` together
   with `--task-id` exits 2.
3. `atm list --tasks --task t-1` and `--task-events t-1 --unread` exit 2.
4. Requirements §6.5/§6.6/§15.4, team-protocol, and `tasks.md` merged;
   `just validate` green.

## Required validation

- CLI tests in `crates/atm/src/commands/send.rs`, `queue.rs`, and
  `list.rs`: AC 2 and 3 conflicts; JSON shapes; the C1 human rendering
  against fixed rows.
- `crates/atm/tests` integration: AC 1 and AC 2 end to end on a rusqlite
  runtime.
- `just validate`; quality-mgr Final Quality Report on the PR.

## Out of scope

Reminder cycle, lead notification, doctor (AX.5, AX.6); `--task-cancel`.
