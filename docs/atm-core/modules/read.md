# `atm-core::read`

Owns read selection, bucket classification, seen-state updates, timeout
behavior, and readable result shaping for the CLI layer to render.

References:

- Product requirements: `docs/requirements.md` §7 and §14
- `REQ-P-READ-001`
- `REQ-P-WORKFLOW-001`
- `REQ-CORE-WORKFLOW-001`
- Cross-cutting behavior: `docs/read-behavior.md`
- CLI surface: `docs/atm/commands/read.md`
