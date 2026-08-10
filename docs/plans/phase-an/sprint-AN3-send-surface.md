---
title: AN.3 Send Surface — Templated Send, Merged Vars, Routing
status: draft
branch: feature/pan-s3-send-surface
worktree: ../atm-core-worktrees/feature/pan-s3-send-surface
target: integrate/phase-an
---

# AN.3 — Send Surface: Templated Send, Merged Vars, Routing

**recommended_agent:** arch-ctm/deep-reasoning (admission semantics and
routing correctness).
**must_follow:** AN.2; merge AN.2's pushed integration line before each dev or
fix round.
**unblocks:** AN.7; provides end-to-end fixtures consumed by AN.8.
**parallel_safe:** AN.4 and AN.5 (non-intersecting: this sprint owns
`crates/atm-core/src/send/` and the CLI send command; AN.4 owns read paths;
AN.5 owns storage-index sync).

**traceability:** plan-phase-an.md Decisions 1, 4, 5, 6, 8; ADR-036's Phase
AN extension; Send/read flow section. Requirement IDs assigned during plan
hardening.

**Entry gate:** Decisions 8 and 12's include-containment and literal
`metadata.type` catalog rules are already settled and must be consumed without
aliases. Decision 14's shared 1 MiB message-admission policy is also settled;
its implementation lands here.

## Deliverables

1. `atm send` flags: `--template <path>`, `--vars <file.json|->`, repeatable
   `--var k=v`, `--env-prefix <PFX_>`. Mutual-exclusion rules with `--file`,
   `--stdin`, and positional text follow the existing `build_message_source`
   validation pattern and produce the same class of typed validation errors.
2. Merged-vars resolution through the core renderer port backed by the
   dedicated `atm-template-sc-compose` adapter, honoring its documented
   precedence (`--var` > `--var-file` > `--env-prefix` > `input_defaults` >
   frontmatter defaults), with env-sourced values captured at compose time:

```rust
/// Fully resolved variables; serializes to `vars_json`. Self-contained:
/// rendering with these vars requires nothing from the environment.
pub struct MergedVars(serde_json::Map<String, serde_json::Value>);

pub fn resolve_merged_vars(
    template: &LoadedTemplate,          // raw bytes + TemplateSha + frontmatter
    var_flags: &[(String, String)],
    var_file: Option<&VarFileSource>,
    env_prefix: Option<&str>,
) -> Result<MergedVars, AtmError>;      // adapter diagnostics wrapped once
```

   Core converts this value at the storage-contract boundary to
   `MergedVarsJson` (AN.2's validated leaf DTO) before calling
   `TemplateCatalogStore::admit_decomposed_message`; `atm-storage` never
   imports or accepts `MergedVars`.

3. Send-side verification render: every templated send renders once before
   admission; a render failure (missing required var, template error) fails
   the send with the wrapped composer diagnostic and a typed atm error
   code. Nothing unrenderable is ever admitted.
4. Routing per Decision 5 has exactly four predicate cells:
   same-team + same-host → call the one semantic
   `admit_decomposed_message` operation (which registers the template and
   inserts the message atomically); same-team + cross-host → plain text;
   foreign-team + same-host → plain text; foreign-team + cross-host → plain
   text. Every plain-text cell stores the verification render as an ordinary
   row with `template_sha IS NULL` and `vars_json IS NULL`, and performs no
   catalog admission/registration for that send. No template content crosses
   hosts or team boundaries.
5. Untyped-template WARN at registration per Decision 12. Detect an include
   directive before decomposed admission; per Decision 8, emit a structured
   WARN and send the verification render as plain text without catalog
   registration or a `Decomposed` row. No implementation may treat a local
   include graph as a durable template dependency. Detection is the
   `TemplateInspection.include_references` result from AN.1, never a CLI
   heuristic. The verification render must call
   `TemplateComposer::render_within_root` with the declared template root;
   its pinned-upstream resolver proves every loaded target is in-root and
   rejects absolute paths, `..`, and symlink escape. If containment cannot be
   proven, or a reference target has vanished/fails, no verified fallback body
   exists: fail the send closed with typed `TEMPLATE_INCLUDE_UNRESOLVED`, do
   not write a catalog/message row, and retain the upstream diagnostic.
6. Classification flags for all sends: `--category`, repeatable `--tag`
   (comma form accepted), `--content-format`; admission validation of
   vocabulary/tag shape/tag count per plan rules.
7. New typed error codes documented in `docs/atm-error-codes.md`: template
   load failure, hash-API failure, missing required variable, render
   verification failure, `TEMPLATE_INCLUDE_UNRESOLVED`, invalid
   tag/category/format, oversized stdin body.
8. Replace `MAX_STDIN_MESSAGE_BYTES` with Decision 14's configuration-backed
   `max_message_bytes` (1 MiB default), applied identically to inline and
   stdin plain sends. The corresponding HTTP body budget is the configured
   message maximum plus documented canonical-envelope overhead, so no valid
   maximum-size message is rejected due only to framing.

## Acceptance criteria

- The four-cell routing matrix is verified against stored rows, not CLI
  output: same-team/same-host → one `Decomposed` row; same-team/cross-host,
  foreign-team/same-host, and foreign-team/cross-host → ordinary plain rows
  with `template_sha` and `vars_json` NULL, verification-render
  `message_text`, and no new catalog admission. The same-host cell runs on
  both UDS and loopback TCP.
- Stored `vars_json` is fully merged: env-prefixed values are present, and a
  subsequent read in an environment without those variables set succeeds
  (asserted by AN.4's fixtures; here by direct storage inspection).
- A send missing a required variable fails with the documented error code
  and surfaces the sc-compose diagnostic text.
- Template registration is idempotent by SHA; two sends of the same template
  file produce one `message_templates` row.
- Include fallback rejects an absolute, `..`, or symlink-escape target before
  rendering; an in-root include may produce only the documented plain-text
  fallback, never a decomposed row.
- Every new error code has a `docs/atm-error-codes.md` entry in the same PR.
- The send path consumes only the core renderer port and storage contracts;
  it has no direct `sc-composer`, SQLite, or FTS dependency.

## Required validation

- CLI integration tests over both local transports
- four-cell team/host routing fixture with stored-row and catalog assertions
- include-root containment fixture suite
- error-code documentation lint
- cargo test/format/lint suite

## Non-closure

Read-side rendering is AN.4. Search/indexing of the new rows is AN.5/AN.6.
`atm compose` and path-body telemetry are AN.7. This sprint does not modify
the shared Claude JSONL export path.
