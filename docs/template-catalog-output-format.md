# Template Catalog Output Formats

This is the single operator procedure for AN.13 catalog format migration.

## Durable contract

The only production adapter, `atm-template-sc-compose`, classifies a
file-backed template with the exact pinned `sc-composer` API at admission. The
result is stored as `text` or `json` beside the immutable template SHA.
`atm-core` and storage carry that value; they never infer it from template
bytes, frontmatter, metadata, or a historical filename.

## Existing rows

Rows created before AN.13 receive a new nullable `output_format` column with
no backfill. `NULL` means **legacy/unverified**, not `text`. Such rows remain
available for historical catalog inspection, but they are not evidence of the
1.4.1 checked-render contract and render-on-read refuses to claim that they
are checked.

## Re-register a legacy source

1. Locate the original template source under its approved template root.
2. Send or register it again through the current file-backed template path.
   The adapter derives its canonical identity and output format from the
   source path and stores the resulting immutable SHA in the catalog.
3. Use the newly admitted SHA for future decomposed messages. Do not update a
   legacy row in place: immutable catalog entries retain their original facts.
4. Verify the new row records `text` or `json`; only that classified revision
   can later participate in AN.14 checked rendering.

Changing the source, including a one-character change, produces a new
immutable SHA and therefore a separately classified catalog entry. Repository
approval, expected metadata, lineage, and protected-template policy remain
the responsibility of the repository that supplies the template, not ATM.
