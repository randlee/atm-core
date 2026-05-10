# `atm-core::read`

Owns single-message selection, successor-chain terminal-node collapse for
logical current-message matching, bucket classification, seen-state updates,
timeout behavior, and detailed message-result shaping for the CLI layer to
render.

Module rules:

- read returns one selected message only
- shared sender/timestamp/task/contains filters match the same logical
  terminal-node set used by `atm list`
- exact `message_id` lookup bypasses logical-current collapse so the addressed
  physical message can still be inspected directly
- read owns selected-message match metadata:
  - `selected_message_id`
  - `match_count`
  - `additional_match_count`

References:

- Product requirements: `docs/requirements.md` §7 and §14
- `REQ-P-READ-001`
- `REQ-CORE-LIST-001`
- `REQ-P-WORKFLOW-001`
- `REQ-CORE-WORKFLOW-001`
- Cross-cutting behavior: `docs/read-behavior.md`
- CLI surface: `docs/atm/commands/read.md`
