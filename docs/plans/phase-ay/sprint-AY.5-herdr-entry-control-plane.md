---
phase: AY
sprint: AY.5
title: Transactional Herdr entry control plane
branch: feature/ay5-herdr-entry-control-plane
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay5-herdr-entry-control-plane
integration_branch: integrate/phase-ay
status: draft
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: core
parallel_with: [AY.8]
stack_parent: feature/ay4-herdr-breaker-lifecycle
pr_target: feature/ay4-herdr-breaker-lifecycle
dependency_relations:
  - prerequisite: AY.4
    dependent: AY.5
    relation: must_follow
    rationale: the operator control plane is introduced only after AY.4 proves that Herdr absence/failure remains a bounded Tokio/Axum runtime condition and is never repaired implicitly by daemon lifecycle code.
  - prerequisite: AY.5
    dependent: AY.6
    relation: must_follow
    rationale: AY.6 restarts a selected endpoint exclusively through the owned entry identifiers, transaction state, doctor ingestion, and fail-closed JSON envelope delivered here.
  - prerequisite: AY.5
    dependent: AY.8
    relation: parallel_safe
    rationale: AY.5 edits daemon-switch scripts/tests, requirements, ADR-053, and skill documentation; AY.8 edits atm-herdr socket transport, fixtures, boundary revision, and architecture exemption after their shared AY.3 contracts merge.
---

# AY.5 — Transactional Herdr entry control plane

Give operators explicit install, remove, status, and repair operations for
per-user Herdr start-at-login entries. Every operation consumes native doctor
state, emits one stable JSON result, owns only marker-bearing objects, and is
recoverable after interruption. Nothing runs implicitly during an ATM switch,
restart, restore, or daemon startup.

## Delivery topology and `/gh-stack`

AY.5 is stacked between breaker lifecycle and coordinated restart:

```text
integrate/phase-ay <- AY.2 <- AY.3 <- AY.4 <- AY.5 <- AY.6 <- AY.7
```

Use the `/gh-stack` skill only through noninteractive forms:

```bash
git config rerere.enabled true
git config remote.pushDefault origin
gh stack link --base integrate/phase-ay \
  feature/ay2-herdr-transport-seam \
  feature/ay3-herdr-endpoint-doctor-config \
  feature/ay4-herdr-breaker-lifecycle \
  feature/ay5-herdr-entry-control-plane
gh pr view feature/ay5-herdr-entry-control-plane \
  --json headRefName,baseRefName,state
```

Append AY.6 and AY.7 with `gh stack link <stack-number> <branch>`. Phase AY
uses `link` for its external-worktree stack and verifies bases with
`gh pr view --json`.
It forbids `gh stack rebase`, `gh stack sync`, and `gh stack merge`; use merge
commits, no force-push, and parent-first PR completion. AY.4 development pushed
triggers merge-forward into AY.5 before every development/fix round.

AY.8 is a separate branch from `integrate/phase-ay` and is parallel-safe with
AY.5. Neither branch merges an unmerged sibling.

## Preconditions

- P-A and P-B from the Phase AY plan are satisfied.
- AY.4 development is pushed, and AY.5 is created from
  `feature/ay4-herdr-breaker-lifecycle`.
- AY.3's `atm doctor --json` schema and AY.4's optional/failure lifecycle are
  green on the parent stack.
- The implementer has read the current `.claude/skills/daemon-switch/SKILL.md`,
  `scripts/daemon-switch.py`, its tests, REQ-P-DAEMON-SWITCH-001, and ADR-053.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or shape-only
completion fails the sprint.

- [ ] D1 — Add REQ-P-DAEMON-SWITCH-002 entry-management clauses to
  `docs/requirements.md` and an ADR-053 addendum defining explicit
  `herdr-entry install|remove|status [--repair]`, endpoint identifiers,
  ownership marker, transaction/journal, repair rules, JSON envelope, exit
  statuses, and the complete error inventory below.
- [ ] D2 — Make daemon-switch determine Herdr configuration and endpoint input
  only by executing native `atm doctor --json`. `herdr.configured` must be
  exactly true for install. False is a safe not-configured refusal; null,
  missing, malformed JSON, or a nonzero doctor exit is an operational failure.
  Python never reimplements roster/backend selection.
- [ ] D3 — Implement per-endpoint `herdr-entry install`, `remove`, and `status`
  in `.claude/skills/daemon-switch/scripts/daemon-switch.py` using C2–C4.
  Support macOS LaunchAgents, Linux user units, and Windows per-user scheduled
  tasks. Installation uses a durable journal and atomic object write;
  removal touches only owned objects.
- [ ] D4 — Implement `status --repair` for every interrupted transaction point.
  It verifies and completes a correct object or unregisters/deletes only a
  marker-bearing half-written object. Foreign objects, digest mismatch, account
  mismatch, and ambiguity fail closed.
- [ ] D5 — Extend daemon-switch's operator-side status projection, tests, and
  skill documentation with entry presence, marker, digest, journal, platform
  registration state, stale/missing remedies, and Windows account/session.
  Native `atm doctor` remains AY.3's endpoint-health authority; platform entry
  inspection is not added to either daemon runtime.

### Paths to delete

None.

## Required work and exact targets

| Ownership | Exact targets |
| --- | --- |
| Governance | `docs/requirements.md`, `docs/adr/ADR-053-typed-temporary-daemon-service-launch-overlay.md` |
| Control plane | `.claude/skills/daemon-switch/scripts/daemon-switch.py` and platform helpers in that directory |
| Tests | `.claude/skills/daemon-switch/tests/test_daemon_switch.py` plus platform-fake fixtures under the same skill |
| Operator documentation | `.claude/skills/daemon-switch/SKILL.md` |

The existing matched ATM CLI/daemon selection and ADR-053 typed temporary
overlay remain unchanged. The Herdr entry journal is a separate record in the
ADR-053 journal directory and never becomes the ATM overlay journal. Do not add
platform service I/O, a supervisor, polling loop, or Herdr start operation to
`atm-http-runtime`, `atm-daemon-bootstrap`, or the frozen synchronous daemon.

## Command and data contracts

### C1 — Explicit grammar

```text
daemon-switch herdr-entry install [--endpoint <default-or-session>]
daemon-switch herdr-entry remove [--endpoint <default-or-session>]
daemon-switch herdr-entry status [--endpoint <default-or-session>] [--repair]
```

Omitting `--endpoint` applies the operation to all doctor-reported configured
endpoints in doctor order. `herdr-entry` is never invoked by ordinary
daemon-switch `switch`, `restart`, or `restore`. Transition from configured to
unconfigured is the explicit `remove` operation prompted by the stale-entry
status finding.

### C2 — Identifier and object contract

```python
def herdr_entry_identifier(platform: str, endpoint: str) -> str:
    """Return the deterministic start-at-login identifier for one endpoint."""
```

| Platform | Default endpoint | Named session `<name>` | Owned object |
| --- | --- | --- | --- |
| macOS | `com.randlee.atm.herdr-server` | `com.randlee.atm.herdr-server.<name>` | LaunchAgent plist + bootstrap state; `RunAtLoad`, no `KeepAlive` |
| Linux | `atm-herdr-server.service` | `atm-herdr-server@<name>.service` | user unit + enable state |
| Windows | `ATM Herdr Server` | `ATM Herdr Server (<name>)` | per-user logon task + enabled state |

Default runs `herdr server`; a session runs
`herdr --session <name> server`. An endpoint with `socket_path` provenance is
externally owned and gets no entry. Every owned definition contains
`managed-by=atm daemon-switch` and its canonical-render digest. Windows refuses
service/session-0 or a different account.

### C3 — Transaction journal

```python
@dataclass(frozen=True)
class HerdrEntryJournal:
    entry_id: str
    platform: str
    digest: str
    action: Literal["install", "remove"]
    phase: Literal["planned", "written", "registered", "verified"]
```

```text
install: plan(render,digest) -> durable journal -> atomic write -> register
         -> verify marker/digest/registration -> complete journal
remove:  verify ownership -> durable journal -> unregister -> delete owned file
         -> verify absent -> complete journal
```

An incomplete journal blocks install/remove. `status --repair` observes actual
platform state and either completes verification or rolls back by unregistering
and deleting only the marker-bearing half-written object. An unmarked collision
or marker-bearing object with a different digest is never overwritten/deleted.

### C4 — JSON and exit contract

Every invocation prints exactly one JSON object on stdout; human diagnostics go
to stderr.

```json
{
  "ok": true,
  "code": "HERDR_ENTRY_STATUS_OK",
  "message": "Herdr entries inspected",
  "remedy": "none",
  "entries": [
    {
      "endpoint": "default",
      "identifier": "com.randlee.atm.herdr-server",
      "owned": true,
      "registered": true,
      "digest_matches": true,
      "journal_phase": null
    }
  ]
}
```

Exit 0 is success, 3 is safe refusal, and 4 is operational failure.
`HERDR_DOCTOR_UNREADABLE` always exits 4. Success codes are stable and include
`HERDR_ENTRY_INSTALLED`, `HERDR_ENTRY_REMOVED`, `HERDR_ENTRY_STATUS_OK`, and
`HERDR_ENTRY_REPAIRED`.

## Error inventory

| Code | Exit | Cause | Required recovery |
| --- | --- | --- | --- |
| `HERDR_NOT_CONFIGURED` | 3 | doctor reports configured false | Configure Herdr or omit the entry operation |
| `HERDR_DOCTOR_UNREADABLE` | 4 | doctor failed or returned null/missing/malformed data | Fix native doctor and rerun; never guess |
| `HERDR_ENTRY_FOREIGN` | 3 | identifier exists without ownership marker | Remove/rename foreign object manually |
| `HERDR_ENTRY_JOURNAL_ACTIVE` | 3 | earlier transaction incomplete | Run `herdr-entry status --repair` |
| `HERDR_ENTRY_DIGEST_MISMATCH` | 3 | marker exists but digest differs | Inspect and explicitly repair/remove the object |
| `HERDR_ENTRY_REGISTER_FAILED` | 4 | platform register/enable failed | Correct the reported platform failure, then repair/retry |
| `HERDR_ENTRY_ACCOUNT_MISMATCH` | 3 | Windows object/ATM daemon is another user, service, or session 0 | Reinstall both per-user under one account |
| `HERDR_SOCKET_PATH_NO_ENTRY` | 3 | endpoint is externally owned by explicit socket path | Manage that Herdr invocation outside daemon-switch |

## Acceptance criteria

- [ ] A1 — REQ-P-DAEMON-SWITCH-002 and ADR-053 contain every C1–C4 rule;
  req-qa maps every normative rule to a fixture.
- [ ] A2 — Doctor-ingestion fixtures cover true, false, null+error, missing
  field, malformed JSON, and nonzero exit; source review finds no Python roster
  predicate.
- [ ] A3 — Default-only and default-plus-two-session fixtures create exactly one
  correctly identified/marked object per endpoint on each platform fake.
- [ ] A4 — Remove deletes owned objects only. Foreign collision, digest
  mismatch, socket-path provenance, and Windows account mismatch leave platform
  state unchanged and emit the documented code.
- [ ] A5 — Failure injection after write and after registration leaves a durable
  journal; install/remove refuse; repair deterministically completes or safely
  rolls back.
- [ ] A6 — Every success and error code has an exact one-object stdout JSON
  snapshot and asserted exit class. Human prose never corrupts stdout.
- [ ] A7 — Source/architecture review finds no implicit invocation, daemon-side
  Herdr lifecycle ownership, or legacy synchronous-daemon modification.
- [ ] A8 — Merge gate is 0 blocking, 0 important, and 0 minor in scope;
  quality-mgr posts PASS and CI is green at merge time.

## Required validation

This is the authoritative validation list.

- [ ] V1 — `python3 .claude/skills/daemon-switch/tests/test_daemon_switch.py`
  exits zero with doctor-ingestion, identifier, transaction, injection, repair,
  JSON, and exit-code fixtures enabled.
- [ ] V2 — Run isolated macOS/Linux platform-fake install -> status -> remove
  flows and assert object, registration, marker, digest, journal, and cleanup.
  Windows is fixture-complete here; AY.7 owns live FastPC4 verification.
- [ ] V3 — `just lint spell` and `just lint adr-index` exit zero.
- [ ] V4 — `just validate` exits zero.
- [ ] V5 — `gh pr view feature/ay5-herdr-entry-control-plane --json
  headRefName,baseRefName,state` reports base
  `feature/ay4-herdr-breaker-lifecycle`; AY.8 is not in this stack.

## Non-closure and out of scope

- Coordinated Herdr restart and ATM restart preflight are AY.6.
- Live Windows entry verification is AY.7; socket transport/cutover/proof are
  AY.8–AY.10.
- This sprint never installs/updates the Herdr binary, directly executes
  `herdr server`, or probes whether Herdr is running. Platform registration may
  apply its native start-at-login behavior.
- No legacy synchronous-daemon runtime or dispatch work is permitted.
