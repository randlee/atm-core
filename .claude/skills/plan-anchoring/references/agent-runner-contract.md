---
name: agent-runner-contract
version: 0.4.1
description: Resolve one trusted harness agent, validate one fenced request and response, and write a credential-safe invocation attestation.
---

# Agent runner contract

The planner resolves a harness agent once with
`scripts/anchor_runner_contract.py resolve`. The resolver reads the selected
plugin root's registry and agent frontmatter as data, verifies confinement,
regular-file status, identity, version, harness-role model, and SHA-256, and
returns an immutable runtime descriptor. A failed supplied root or registry
fails closed; callers do not search for another installation.

Render `templates/agent-invocation.md.j2` with that descriptor and invoke the
named definition through the harness **Agent** tool. The invocation contains
exactly one fenced JSON request. The response must contain exactly one fenced
JSON result and pass `validate-fence` before any follow-on action.

Write one attestation per invocation with `build_attestation` and
`atomic_write_json`. Attestations contain only typed identity, hashes, timing,
outcome, and an optional typed error code. They never contain request bodies,
result bodies, environment values, credentials, or interpolated commands.

Leaf review and tuning agents consume the already-resolved descriptor. They do
not search installation paths or repeat registry/frontmatter validation.
