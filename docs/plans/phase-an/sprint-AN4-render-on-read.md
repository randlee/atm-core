---
title: AN.4 Render-on-Read — Read Paths, Export, Determinism
status: draft
branch: feature/pan-s4-render-on-read
worktree: ../atm-core-worktrees/feature/pan-s4-render-on-read
target: integrate/phase-an
---

# AN.4 — Render-on-Read: Read Paths, Export, Determinism

**recommended_agent:** arch-ctm/deep-reasoning (touches every read path and
the export boundary).
**must_follow:** AN.2; merge AN.2's pushed integration line before each dev or
fix round. AN.6 must merge this sprint before implementing query-result
rendering, because this sprint owns the sole core renderer port.
**unblocks:** AN.8.
**parallel_safe:** AN.3 and AN.5. Fixtures are storage-layer seeded
`Decomposed` rows (via AN.2's write API), so this sprint does not require the
send surface; full send→read end-to-end is proven in AN.8.

**traceability:** plan-phase-an.md Decisions 1, 2, 10; Design principle
"Determinism is a tested invariant"; renderer non-determinism risk entry.
Requirement IDs assigned during plan hardening.

## Deliverables

1. NULL-body handling across every path that reads `message_text`: `atm
   read`, `atm peek`, `atm list`, `--json` outputs, and the shared Claude
   JSONL export. Decomposed rows resolve their body by rendering:

```rust
/// Render a decomposed message body. Pure function of stored state:
/// no environment, clock, locale, or host input may influence output.
pub fn render_decomposed(
    template: &StoredTemplate,   // content loaded by template_sha
    vars: &MergedVars,           // from vars_json
) -> Result<RenderedBody, AtmError>; // core port; composer adapter delegate
```

2. Shared Claude JSONL export: render first, then apply the existing
   `claude_jsonl_body_export_max_bytes` stub rules unchanged — an oversized
   rendered body exports as the existing `atm read --message-id <id>`
   retrieval stub exactly as an oversized plain body does.
3. Peek/list snippet source for decomposed rows: leading characters of the
   render, computed on demand (no stored snippet column).
4. Corruption error path: a decomposed row whose `template_sha` cannot be
   loaded, or whose render fails, produces a typed error naming `message_id`
   and `template_sha`. Documented recovery: re-register the template file
   (same SHA) — no repair machinery beyond that.
5. Determinism CI: a fixture corpus (including the AN.1 real-template
   fixtures) rendered on macOS, Linux, and Windows lanes with byte-equality
   asserted across platforms and across repeated runs. Any reachable
   non-deterministic template feature found is a Blocking finding on this
   sprint, not a documented caveat.

## Acceptance criteria

- Every read surface presents rendered bodies for decomposed rows; no
  consumer-visible output contains a NULL/empty body for a valid decomposed
  message.
- Render output is byte-identical across all three CI platforms and across
  repeated runs for the full fixture corpus.
- Export stubbing behaves identically for oversized rendered and oversized
  plain bodies (snapshot test).
- The corruption path returns the documented typed error and identifiers;
  re-registering the template restores readability in the fixture test.
- Reads succeed in an environment stripped of the env variables that
  populated the fixtures' vars (proves merged-vars self-containment).
- The read paths use the same core renderer port as send and query-result
  snippets; no read-facing crate calls `sc-composer` directly.

## Required validation

- read/peek/list/`--json` integration tests over seeded decomposed rows
- cross-platform byte-equality CI lanes (macOS/Linux/Windows)
- JSONL export snapshot tests (stub parity)
- corruption/recovery fixture test
- cargo test/format/lint suite

## Non-closure

Search snippets and query surfaces are AN.6. This sprint does not add send
flags and does not create FTS structures.
