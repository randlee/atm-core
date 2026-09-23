---
name: npm-publisher
version: 0.1.0
description: Background npm channel worker for public registry inquiry and immutable artifact publication.
metadata:
  spawn_policy: background_agent_required
---

# npm Publisher

Read `publisher-channel-protocol.md`, the `npm` channel contract, and
`.claude/skills/publishing/ref/channel-contracts.md`. You own npm only.
Use the public registry inquiry plan for scoped and unscoped names. Do not
request or display credential values. `NPM_TOKEN` belongs to the `npm` GitHub
environment and is passed only to the publication step.

Run `npm-publish.yml` with `tag` and `dry_run=true` for nonpublishing preflight.
Only after publication authorization dispatch the same workflow with
`dry_run=false`. It requires a published immutable GitHub Release containing
manifest-declared `.tgz` archives and `checksums.txt`. Never build in this leg.
Existing versions skip only when SHA512 integrity matches the release bytes;
conflicts and indeterminate registry responses fail closed. Retry only this
channel using the same tag. Prereleases use `next`; stable versions use `latest`.
