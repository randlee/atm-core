---
name: todo-triage
version: 1.0.0
description: >
  Run the repo Rust TODO finder during sprint-end or integration review and
  turn every discovered TODO into a QA finding rather than deferred work.
depends_on:
  quality-management-gh: 1.x
---

# TODO Triage

Audience: `quality-mgr` and `team-lead`.

Use this during sprint-end QA or integration review when Rust TODO hygiene must
be checked explicitly.

## Policy

In `atm-core`, TODO comments are not an approved way to defer work.

Every discovered TODO is a policy violation until it is resolved through one of
these outcomes:
- `fix-now`
- `remove-now`
- `rewrite-now-as-explanatory-comment`

Do not treat a TODO comment as backlog, planning authority, or silent scope
deferral.

If the underlying concern is truly out of sprint scope, raise it as a QA
finding and let the normal triage/planning flow capture the follow-up. Do not
leave the TODO comment behind as the tracking mechanism.

## When To Run

Run this:
- during every implementation sprint-end QA pass
- before integration-branch merge approval
- whenever reviewer output suggests deferred-by-comment behavior

## Command

From repo root:

```bash
python3 scripts/find_todos.py
```

The script scans repo Rust source files only.

Output format:

```text
file:line:tag:text
```

Tag rules:
- `TODO(P6)` -> `P6`
- `TODO(fix)` -> `fix`
- plain `TODO` -> `untagged`

## Required Workflow

1. Run the finder script.
2. Group rows by tag for reporting convenience only.
3. Treat every Rust TODO row as a QA finding candidate.
4. For each TODO, decide whether it must be:
   - fixed immediately
   - removed immediately
   - rewritten immediately as a non-action explanatory comment
5. If the underlying work is genuinely out of sprint scope, record it through
   the normal QA finding / Turtle triage flow and remove or rewrite the TODO in
   source. The source comment itself must not remain as deferral authority.

## Required Report Shape

Produce a concise report grouped by tag:
- tag
- count
- rows
- chosen disposition
- finding id or triage record path if the item enters the QA/Turtle flow

The triage pass fails if any TODO remains unresolved at the end.
