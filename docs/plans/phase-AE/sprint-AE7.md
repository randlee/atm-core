---
id: AE.7
title: User-Doc Graph And Example Verification
status: complete
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

- the dedicated user-document verifier (retired in AT.2 after its manifest
  schema became unreachable)
- `.just/`
- `Justfile`
- `scripts/validate_release.py`
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
def verify_installed_copy(source_root: Path, installed_root: Path) -> list[str]: ...
```

## Deliverables

- one canonical verifier script checked:
  - every linked user-doc target exists
  - every linked path is relative
  - every fenced `json` block parses
  - every fenced `xml` block parses
  - every fenced `toml` block parses
  - every fenced `bash` block passes `bash -n`
- the same verifier supports two modes:
  - source-tree verification for `docs/user-documents/`
  - installed-copy verification for
    `target/phase-ae/staged-install-root/share/doc/atm/`
- installed-copy verification reuses the deterministic staged install root
  defined by `AE.5`; `AE.7` must not introduce a second staging path or a
  second install-copy verifier
- `.just/` and `Justfile` wire `just test` to invoke the source-tree
  verification path
- `scripts/validate_release.py` invokes the installed-copy verification path
  rather than inventing another documentation checker
- user-doc tests under the existing repo test harness enumerate every
  `HelpTopic`, call `doc_link_for_topic`, and assert each `relative_path`
  resolves to a real file under both `docs/user-documents/` and
  `target/phase-ae/staged-install-root/share/doc/atm/`
- failures name:
  - the document path
  - the fenced language
  - the fenced-block ordinal
  - the broken link target when applicable

## Acceptance Criteria

- broken docs fail locally before publisher/release work starts
- the verifier operates on the repo-owned source tree and does not depend on
  installed side effects
- installed-copy verification reuses the same script and failure format
- verifier coverage includes nested relative links, not just README siblings
- `AE.7` is the only sprint allowed to add fenced-example and relative-link
  validation to the installed-copy path
- `.just/`, `Justfile`, and `scripts/validate_release.py` all reference the
  same canonical verifier instead of introducing a second documentation path
- the help-table-to-doc-corpus cross-boundary gap is closed explicitly here;
  markdown-link verification alone is not treated as sufficient coverage for
  `doc_link_for_topic`

## Required Validation

- `python3 scripts/release_artifacts.py stage-install-docs --manifest release/publish-artifacts.toml --output-root target/phase-ae/staged-install-root`
- the retired verifier against `docs/user-documents`
- the retired verifier against `target/phase-ae/staged-install-root/share/doc/atm`
- `cargo test -p agent-team-mail commands::help::tests::doc_link_for_every_topic_resolves_in_source_and_installed_copy -- --nocapture`
- reference scan of the verifier's former runtime surfaces
- `just test`
- `git diff --check`
