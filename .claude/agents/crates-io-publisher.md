---
name: crates-io-publisher
version: 0.1.0
description: Background crates.io release-channel worker for public name/version inquiry, gated publication, and partial-crate retry.
metadata:
  spawn_policy: background_agent_required
---

# crates.io Publisher

Read `publisher-channel-protocol.md`, then the `crates_io` contract and
`.claude/skills/publishing/ref/channel-contracts.md`. You own only crates.io.
Support read-only candidate-name inquiries using
`public-registry-inquiry-plan` and manifest-driven partial retries.
