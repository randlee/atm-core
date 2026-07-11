---
id: AE.7
title: User-Doc Graph And Example Verification
status: planned
branch: feature/pAE-s7-user-doc-graph-verification
worktree: ../atm-core-worktrees/feature/pAE-s7-user-doc-graph-verification
target: integrate/phase-AE
---

# Sprint AE.7 — User-Doc Graph And Example Verification

## Goal

Add one mechanical verifier that fails closed on broken links, missing target
docs, or invalid fenced examples.

## Hard Dependencies

- `AE.6` complete
- `docs/plans/phase-AE/plan-phase-AE.md`

## Exact Targets

- `scripts/`
- `.just/`
- `Justfile`
- user-doc tests under the existing repo test harness

## Interfaces To Add Or Modify

The verifier must expose one direct validation surface:

```python
def extract_fenced_blocks(markdown_text: str) -> list[FencedBlock]: ...
def validate_json_block(block: FencedBlock) -> list[str]: ...
def validate_xml_block(block: FencedBlock) -> list[str]: ...
def validate_toml_block(block: FencedBlock) -> list[str]: ...
def validate_bash_block(block: FencedBlock) -> list[str]: ...
def validate_relative_links(doc_root: Path) -> list[str]: ...
```

## Deliverables

- one verifier checks:
  - every linked user-doc target exists
  - every linked path is relative
  - every fenced `json` block parses
  - every fenced `xml` block parses
  - every fenced `toml` block parses
  - every fenced `bash` block passes `bash -n`
- failures name:
  - the document path
  - the fenced language
  - the fenced-block ordinal
  - the broken link target when applicable

## Acceptance Criteria

- broken docs fail locally before publisher/release work starts
- the verifier operates on the repo-owned source tree and does not depend on
  installed side effects
- verifier coverage includes nested relative links, not just README siblings

## Required Validation

- `just test`
- `git diff --check`
