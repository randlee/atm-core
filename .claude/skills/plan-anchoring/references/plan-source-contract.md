---
name: plan-source-contract
version: 0.6.1
description: Hardened discovery and parity contract for Markdown phase plans and bd/br epic copies.
---

# Plan source contract

`scripts/plan_source.py` materializes a hardened phase as ordered, complete sprint
documents. It accepts either the authoritative Markdown phase index/directory or
an optional exact copy under a `bd` or `br` epic. It never executes Markdown,
HTML, links, repository content, or tracker text.

## Discover

Run from the repository root:

```bash
P="<this skill's directory>/scripts/plan_source.py"
python3 "$P" discover-markdown docs/plans/phase-a/plan-phase-a.md
python3 "$P" discover-markdown docs/plans/phase-a/
python3 "$P" discover-tracker --tracker br --epic pa-sza
```

The directory form must contain exactly one `plan-phase-*.md` index. The index
must contain exactly one sprint table. Every linked sprint must be a confined,
non-symlink regular file with one Sprint title and exactly one authoritative
scope, deliverables, acceptance, validation, and non-closure section.

The tracker form calls only the selected executable with fixed argument arrays:
`show <epic> --json`, then `show <direct child>... --json`. Each child
description must be one complete sprint document. Title, acceptance, design,
notes, tracker identifiers, and revisions are metadata and never alter item
text. Records are ordered by the Sprint identifier in their copied document,
not tracker return order.

## Verify or refresh an epic copy

Markdown is authoritative. Verify without writing:

```bash
python3 "$P" copy docs/plans/phase-a/ --tracker br --epic pa-sza
```

Every action must report `status: verified` before review. `content_equal: true`
with `metadata_equal: false` means the copied document is exact but its
`external_ref` lacks the current normalized digest. Apply safe copy/provenance
updates explicitly:

```bash
python3 "$P" copy docs/plans/phase-a/ --tracker br --epic pa-sza --apply
```

Writes use `--description-file`, a fixed argv, and the tracker's
`--if-unchanged <updated_at>` guard. The recorded reference is
`plan-copy:<repository path>@sha256:<normalized SHA-256>`. A differing tracker
description is writable only when its current digest equals the previously
recorded source digest. Otherwise the command returns `SOURCE.COPY_CONFLICT` and
does not overwrite independently changed text.

## Review boundary

Each descriptor contains source kind/reference/revision, phase and sprint IDs,
dependency relation, raw and normalized digests, shape evidence, and the
complete normalized sprint text. Only these fields enter a Jev request:

```json
{
  "model": "<model>",
  "state": {
    "anchor": {"id": "ANC-001", "statement": "...", "scope": "..."},
    "item": {"id": "A2", "title": "...", "text": "<complete sprint>"}
  },
  "questions": {}
}
```

`canonical_jev_body` uses the same serializer as `jev_judge.encode_body`, so
direct, Markdown, `bd`, and `br` materializations of equivalent content are
byte-identical at the HTTP boundary. Source paths, tracker IDs, run IDs,
revisions, and credentials stay in local run bindings and attestations.

`shape` fails with `PLAN.TOO_LARGE` and evidence when a sprint exceeds the
reliable limit. It never truncates text or raises the cap. Source and structure
failures are typed and occur before agent delegation.
