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

**Entry gate:** Decision 8's include containment and Decision 12's literal
`metadata.type` catalog rule are already settled and must be consumed without
aliases. Open question 4 (`MAX_STDIN_MESSAGE_BYTES` value) must be resolved
in the plan before this sprint starts; its implementation lands here.

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

3. Send-side verification render: every templated send renders once before
   admission; a render failure (missing required var, template error) fails
   the send with the wrapped composer diagnostic and a typed atm error
   code. Nothing unrenderable is ever admitted.
4. Routing per Decision 5: recipient same-team → store `Decomposed`
   (registering the template in the same transaction); foreign-team or
   cross-host recipient → store/send the verification render as an ordinary
   plain-text message. No template content crosses hosts.
5. Untyped-template WARN at registration per Decision 12. Detect an include
   directive before decomposed admission; per Decision 8, emit a structured
   WARN and send the verification render as plain text without catalog
   registration or a `Decomposed` row. No implementation may treat a local
   include graph as a durable template dependency. Detection is the
   `TemplateInspection.include_references` result from AN.1, never a CLI
   heuristic. If a reference was detected but its target has vanished (or
   fails) during the required verification render, no verified fallback body
   exists: fail the send closed with typed `TEMPLATE_INCLUDE_UNRESOLVED`, do
   not write a catalog/message row, and retain the upstream diagnostic.
6. Classification flags for all sends: `--category`, repeatable `--tag`
   (comma form accepted), `--content-format`; admission validation of
   vocabulary/tag shape/tag count per plan rules.
7. New typed error codes documented in `docs/atm-error-codes.md`: template
   load failure, hash-API failure, missing required variable, render
   verification failure, `TEMPLATE_INCLUDE_UNRESOLVED`, invalid
   tag/category/format, oversized stdin body.
8. `MAX_STDIN_MESSAGE_BYTES` becomes config-driven at the value resolved for
   Open question 4; applies to inline and stdin plain sends only.

## Acceptance criteria

- A templated same-team send round-trips as `Decomposed` on UDS and loopback
  TCP; the identical send to a cross-host peer arrives as plain text; both
  verified against the stored rows, not CLI output.
- Stored `vars_json` is fully merged: env-prefixed values are present, and a
  subsequent read in an environment without those variables set succeeds
  (asserted by AN.4's fixtures; here by direct storage inspection).
- A send missing a required variable fails with the documented error code
  and surfaces the sc-compose diagnostic text.
- Template registration is idempotent by SHA; two sends of the same template
  file produce one `message_templates` row.
- Every new error code has a `docs/atm-error-codes.md` entry in the same PR.
- The send path consumes only the core renderer port and storage contracts;
  it has no direct `sc-composer`, SQLite, or FTS dependency.

## Required validation

- CLI integration tests over both local transports
- cross-host peer fixture test for the plain-text fallback
- error-code documentation lint
- cargo test/format/lint suite

## Non-closure

Read-side rendering is AN.4. Search/indexing of the new rows is AN.5/AN.6.
`atm compose` and path-body telemetry are AN.7. This sprint does not modify
the shared Claude JSONL export path.
