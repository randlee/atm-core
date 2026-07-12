---
id: AE.4
title: Hooks And Nudge Template Corpus
status: complete
branch: feature/pAE-s4-hooks-and-nudge-template-corpus
worktree: ../atm-core-worktrees/feature/pAE-s4-hooks-and-nudge-template-corpus
target: integrate/phase-AE
---

# Sprint AE.4 — Hooks And Nudge Template Corpus

## Goal

Author the installed user-doc set for startup/idle hooks and built-in nudge
template overrides.

## Hard Dependencies

- `AE.3` complete
- `docs/plans/phase-AE/plan-phase-AE.md`
- `docs/plans/phase-AD/sprint-AD21.md`

## Exact Targets

- `docs/user-documents/hooks.md`
- `docs/user-documents/nudge-templates.md`
- `docs/user-documents/examples/hooks/`
- `docs/user-documents/examples/nudge-templates/`

## Deliverables

- `docs/user-documents/hooks.md` explains:
  - where ATM-enabled repo-local hook configuration lives
  - which supported hook surfaces exist today
  - how `ATM_IDENTITY` / `ATM_TEAM` affect ATM-aware hook behavior
  - where installed docs and runtime state differ
- `docs/user-documents/nudge-templates.md` explains:
  - the six built-in template kinds
  - the exact supported variables:
    - `{{from}}`
    - `{{team}}`
    - `{{message_id}}`
    - `{{description}}`
    - `{{task_id}}`
  - override precedence
  - disable/reset behavior
  - complete example template bodies
- `docs/user-documents/examples/hooks/` and
  `docs/user-documents/examples/nudge-templates/` include working fenced
  examples for:
  - `toml`
  - `xml`
  - `bash`
  - `json`

## Acceptance Criteria

- hook docs describe only supported operator-facing behavior
- nudge-template docs are explicit about the exact variable surface and do not
  imply Jinja or unsupported conditionals
- every XML template example includes `message_id`
- acknowledge-template examples stay compact and match the accepted AD.21
  contract
- `docs/user-documents/hooks.md`, `docs/user-documents/nudge-templates.md`,
  `docs/user-documents/examples/hooks/`, and
  `docs/user-documents/examples/nudge-templates/` are all owned and validated
  by this sprint

## Required Validation

- `python3 -c "from pathlib import Path; files=[Path('docs/user-documents/hooks.md'), Path('docs/user-documents/nudge-templates.md')]; dirs=[Path('docs/user-documents/examples/hooks'), Path('docs/user-documents/examples/nudge-templates')]; assert all(p.is_file() for p in files); assert all(d.is_dir() for d in dirs)"`
- `python3 - <<'PY'\nimport pathlib, tomllib\nfor path in pathlib.Path('docs/user-documents/examples/hooks').glob('*.toml'):\n    tomllib.loads(path.read_text(encoding='utf-8'))\nPY`
- `python3 - <<'PY'\nimport pathlib, xml.etree.ElementTree as ET\nfor path in pathlib.Path('docs/user-documents/examples/nudge-templates').glob('*.xml'):\n    ET.fromstring(path.read_text(encoding='utf-8'))\nPY`
- `find docs/user-documents/examples/nudge-templates -name '*.json' -print0 | xargs -0 -n1 python3 -m json.tool >/dev/null`
- `find docs/user-documents/examples/hooks docs/user-documents/examples/nudge-templates -name '*.sh' -print0 | xargs -0 -n1 bash -n`
- `git diff --check`
