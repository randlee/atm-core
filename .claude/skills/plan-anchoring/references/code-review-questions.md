---
name: code-review-questions
version: 0.6.0
description: The development-gate (sprint review) Jev question set, the evidence question, what delivered-work text to feed it, and pairing with deterministic checks.
---

# Development-gate questions

The wording of record is `assets/questions/dev.json`. The quality manager runs this gate through `anchor-sprint-review`, against the delivered work.

## What is being judged

Jev judges an account of what was built, assembled by the agent in this priority order. The complete serialized development request is capped at 64,000 bytes; when necessary the script truncates the account from the bottom:

1. The sprint report or completion summary, if one exists.
2. The PR description (title and body).
3. `git diff --stat <base>..<head>` output: the files touched.
4. Test summary lines (names and pass/fail counts, not full logs).
5. Titles and `close_reason` of the sprint's closed child beads (`compose-item --children-json`).

Record what commands printed, never a paraphrase. Sections are labelled so Jev can tell a claim ("we copied the legacy crates") from an artifact ("crates/runtime/src/lib.rs | 412 +-"). Full diffs are not sent: they blow the state budget and add noise.

## The four questions

| ID | Role | Asks | Isolates |
| --- | --- | --- | --- |
| `relevant` | relevance | Did the work touch the subject the anchor governs? | Scope; unrelated sprints are `not_applicable` |
| `implemented` | compliance | Does the account say the directive was followed in what was built? | Abandoned in code after being planned |
| `violated` | conflict | Does the account show the work did what the directive rules out? | Drift during implementation |
| `evidenced` | evidence | Where compliance is claimed, is checkable evidence cited (paths, diffs, commands, tests, comparisons)? | "Said it" vs "showed it" |

The first three mirror the planning gate. The fourth exists because Jev confirms the work *describes* following the directive, not that the code did. A compliance claim without checkable evidence becomes `asserted_only`, which fails the gate until evidence is added or the paired deterministic check is run.

## Pairing with deterministic checks

An anchor's `deterministic_check` is a diff, git log, grep, or test that proves the directive literally. The agent returns every `satisfied` or `asserted_only` anchor's check in `deterministic_checks`; the quality manager runs them and records the outcome beside the Jev verdict. Examples:

| Anchor | Deterministic check |
| --- | --- |
| "copy the libraries from the legacy repo as the starting point" | `git log --follow crates/runtime` shows the legacy history, or `diff -r <legacy>/crates/runtime crates/runtime` is mostly deletions |
| "no new dependency for this" | `git diff <base>..<head> -- Cargo.toml` adds no `[dependencies]` line |
| "the CLI stays thin" | `rg 'sqlx::' src/cli/` returns nothing |

Jev says where to look; the check says what is there.

## Wording rules

As at the planning gate (`plan-anchor-questions.md`), plus:

- `implemented` must say that intent or deferral does not count as followed, or "to be done next sprint" passes.
- `evidenced` must say that "no compliance claim at all" answers `no`, so an account that never mentions the anchor cannot be `evidenced: yes`.

## Reporting

A failing development-gate line reads:

> **sprint-2 · ANC-012 · violated** You said (planning session, Oct 9): "copy the libraries from the legacy repo as the starting point and remove the legacy-specific code" The delivered work went against this anchor.

An anchor faithfully planned but abandoned in code passes the planning gate and fails here; that is why the gates are separate.
