---
name: anchor-capture
version: 0.6.1
description: How the planning orchestrator detects candidate anchors, writes each one as a clear testable requirement with the user's verbatim words kept as provenance, deduplicates, and gets the user's approval before locking.
---

# Capturing anchors

An anchor is a fact the user stated while planning, written as a requirement a plan bead or sprint report can be judged against. It keeps two texts:

| Field | What it is | Who writes it | Used for |
| --- | --- | --- | --- |
| `quote` | The user's exact words | nobody; copied verbatim | Provenance; quoted in every failing verdict. Immutable |
| `statement` | One clear, testable requirement | the orchestrator; approved by the user | What Jev judges. Reworded with `update --statement` |

**The quote is not the anchor.** Speech is compressed, deictic ("these crates", "like the legacy repo") and bundled; Jev reads literally, so a statement that is just the quote yields `underspecified` and false `not_addressed` verdicts. `validate` and `add` refuse it. Capture is interactive and stays in the main session.

## What counts as an anchor candidate

Listen for directives about how the work must be done, not what it is:

- A starting point: "copy the libraries from the legacy repo as the starting point", "reuse the existing parser".
- A prohibition: "don't add a new dependency for this", "never write to the DB from the CLI layer".
- A method or order: "migrate the schema first, then the API", "keep the trait object-safe".
- A boundary: "the CLI must stay thin; business logic lives in the library".
- A comparison to an existing system to reproduce or avoid.
- A correction: "no, I said ..." means the fact has already drifted once.

Not anchors: task lists, deliverables, estimates, restatements of the intent, agreement ("yes, do that"), or anything already in the requirement or ADR register (add those as REQ or ADR anchors).

## Be selective

A phase needs five to fifteen anchors, not one per sentence. Before proposing, ask:

1. Would a bead that ignores this be wrong, or merely different? Only the first is an anchor.
2. Is it covered by an existing anchor, REQ or ADR? Then it is a duplicate (see Deduplication).
3. Can it be one requirement with an observable outcome? If not, ask the user one clarifying question first.

## Three kinds of anchor

Each anchor carries a `kind` (`add --kind`, default `directive`); Jev cannot judge every fact the same way:

| Kind | Example | How it is judged |
| --- | --- | --- |
| `directive` | "The runtime crates are created by copying the legacy repo's runtime crates ..." | relevant, addressed, contradicted |
| `prohibition` | "No bead in this phase changes a layer's responsibilities, an ADR decision, or the crate set." | relevant and contradicted only; silence is compliance (measured-behaviour.md) |
| `structural` | "Every sprint bead's design gives the code for each deliverable." | its `deterministic_check` only (`shape` metrics, a grep, a count); Jev is not asked at the planning gate |

A conditional prohibition ("... unless the user approved it first") cannot be contradicted by Jev, even when the contradicting sentence is the whole item (measured-behaviour.md). Split it: the prohibition as one anchor, the approval record as a directive ("a bead that changes a layer's responsibilities names the discussion where the user approved it").

## Writing a testable statement

Write a requirement about the system or the work: third person, one constraint, the artifact named, the outcome observable. Prefer the positive form with concrete tokens: crate names, paths, commands, issue numbers. Such anchors got the highest relevance confidence and the only correct `satisfied` verdicts (measured-behaviour.md).

Pattern: `<subject> <must | must not | is | are | never | only> <action or property> [<scope or condition>]`.

| Quote (verbatim) | Weak statement | Testable statement |
| --- | --- | --- |
| "copy the libraries from the legacy repo as the starting point and remove the legacy-specific code" | "Copy the libraries from the legacy repo as the starting point" | "The runtime crates are created by copying the legacy repo's runtime crates and deleting the legacy-specific modules; no runtime crate is written from scratch." |
| "don't add a new dependency for this" | "Don't add dependencies" | "Sprint 2 adds no new `[dependencies]` entry to any Cargo.toml." |
| "the CLI should stay thin" | "The CLI should stay thin" | "Business logic lives in the library crates; `src/cli/` contains argument parsing and output formatting only." |

Rules `lint` enforces:

- Not identical to the quote (error).
- One constraint. Two requirement modals ("must ... and must never ...") means two anchors.
- No hedges: should, could, maybe, probably, try to, as needed, if possible.
- No first or second person. "We copy ..." is speech; "The crates are copied ..." is a requirement.
- Name the thing: a crate, path, command, file, table; not "the code" or "this".
- State the observable outcome, not a quality: "mostly deletions against <legacy>/crates/runtime", not "clean".
- Under about 240 characters. Longer means it bundles.

Run `python3 "$S" lint --anchors "$A" --id ANC-012` after writing; fix warnings before asking for approval.

## Proposing and confirming

Propose at once, with both texts:

> Anchor candidate ANC-012. You said: "copy the libraries from the legacy repo as the starting point and remove the legacy-specific code". Proposed anchor: "The runtime crates are created by copying the legacy repo's runtime crates and deleting the legacy-specific modules; no runtime crate is written from scratch." Lock it?

Record only when the user confirms or says "lock this" or "anchor this". If "lock this" refers to something said turns ago, quote back what you believe they mean and confirm. `--status proposed` records a candidate so it survives compression; it is not judged until locked. Never batch candidates to the end of the conversation; a proposal that waits gets compressed. If the user corrects the wording, that is the statement being authored; the quote never changes.

## Deduplication

Before `add`, compare the candidate against every anchor in `anchors.md`, including REQ and ADR entries. Two anchors are duplicates when a plan that satisfies one necessarily satisfies the other. `add` refuses a candidate lexically close to an existing anchor (`ANCHOR.DUPLICATE_CANDIDATE`) and names the match; pass `--allow-similar` only after reading the match and finding a different constraint.

For a set that has grown:

```bash
python3 "$S" dedupe --anchors "$A"            # lexical pairs
python3 "$S" dedupe --anchors "$A" --jev      # asks Jev per candidate pair: same constraint?
```

For each confirmed pair, keep the better statement, fold any missing condition into it with `update --statement`, and `retire --id ANC-0xx --replaced-by ANC-0yy --reason duplicate` the other. Two anchors for one constraint double-count in coverage and reports.

## Granularity

One testable constraint per anchor. "Copy the legacy crates and strip the legacy-specific code" is one act, one constraint. "Copy the legacy crates and keep the CLI thin" is two anchors, each quoting the sentence that carries it. The test: can Jev answer "does this bead address the directive" with one yes or no? If a bead could address half of it, split.

## Fields you write at lock time

| Field | Purpose | How to write it |
| --- | --- | --- |
| `statement` | What Jev judges | As above; lint clean; approved by the user |
| `quote` | Provenance | Verbatim, trimmed of leading and trailing filler only; interior elisions marked `[...]` and avoided |
| `scope` | Feeds the relevance question so unrelated beads are `not_applicable`, not failures | Bead-level signals, not a topic sentence: "adds or changes a CLI subcommand", "edits a Cargo.toml", "touches crates/<name>" (measured-behaviour.md) |
| `kind` | Chooses the gating | `directive` unless the statement forbids something (`prohibition`) or describes the shape of every bead (`structural`) |
| `summary` | Table label | Under 12 words |
| `deterministic_check` | Code-level proof paired at the development gate | A diff, git log, grep, or test showing the statement literally holds |
| `fixtures.pass` / `fixtures.fail` | What a bead would say if it satisfied or violated the anchor | One sentence each; draft, get approval, record with `update --fixture-pass/--fixture-fail`; `check-fixtures` confirms they land |

```bash
python3 "$S" add --anchors "$A" \
  --quote "copy the libraries from the legacy repo as the starting point and remove the legacy-specific code" \
  --statement "The runtime crates are created by copying the legacy repo's runtime crates and deleting the legacy-specific modules; no runtime crate is written from scratch." \
  --stated-by "<name>" --context "planning session" --summary "Runtime crates copied from the legacy repo, not rewritten" \
  --scope "the runtime crates sprint and any bead that creates the core library implementation" \
  --deterministic-check "diff -r <legacy>/crates/runtime crates/runtime is mostly deletions" --status locked
```

`add` allocates the next ANC number, runs the lint and duplicate check, and refuses on an error. Then `validate`, `render`, create the bead tree (`bead-layout.md`), and spin off the question tuner (`question-tuning.md`); an anchor is not gate-ready until its own `addressed` and `relevant` wording is tested. Watch for homonyms: if the plan uses a key word of the statement in another sense ("layer" as stacked-PR position versus architectural layer), say so in the summary and the override; that homonym caused false passes (measured-behaviour.md).

## Re-opening a plan

When a planning session resumes, read `anchors.md` before the plan and say so: "Loaded 14 anchors for phase-current." The user never re-states a locked fact; if they do, it is the same anchor, not a second. If their wording now implies a different constraint, `update --statement`; if the directive itself changed, add a new anchor and retire the old.
