# bc.4 sc-publish consumer qualification

## provenance

| item | result |
| --- | --- |
| upstream repository | `https://github.com/randlee/sc-publish` |
| qualified source SHA | `22137c2da13bf4638b4267b69c6c2f021617da73` |
| consumer branch | `feature/bc4-sc-publish-immutable-consumer` |
| consumer base | `5b30535acb8714eea7563b3789ae46b9981c21e7` |
| pin | `release/sc-publish-pin.toml` records the full source SHA |

## installer and generated parity

The kit was exported at the pinned SHA into an isolated temporary checkout.
The canonical bootstrap and installer were run from that checkout with
`release/sc-publish-consumer-input.json`; no generated shared file was
hand-edited.

```text
bootstrap_sc_compose.py --venv <isolated-kit>/.venv
<isolated-kit>/.venv/bin/python plugins/sc-publish/install.py \
  --input release/sc-publish-consumer-input.json .
<isolated-kit>/.venv/bin/python plugins/sc-publish/install.py --dry-run \
  --input release/sc-publish-consumer-input.json .
```

The installer copied the kit assets, rendered the two consumer manifests, and
the repeat dry-run returned `Publish-kit assets are in sync.` (exit 0). This is
the generated-parity proof for the pinned source and consumer input.

## consumer checks

- `release/sc-publish-pin.toml` contains the exact 40-character source SHA.
- The installer output is byte-for-byte kit output; only the two documented
  manifests are rendered from consumer input.
- PR #101 is retained only as historical evidence; PR #106 is the combined
  acceptance source.
- No repository setting, credential, release, tag, registry, or channel was
  mutated.
- Inherited or deferred FIX03–FIX10 items are explicitly not claimed by bc.4.

## disposition

`u4` is qualified for this consumer at the exact source SHA above. Any
setting/credential activation belongs to separately authorized bc.5 evidence.
