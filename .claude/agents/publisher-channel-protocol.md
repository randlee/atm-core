# Publisher Channel Protocol

All role-specific background channel workers must read this file, then
`release/publish-channel-contracts.toml` and
`.claude/skills/publishing/ref/channel-contracts.md`, before acting.

## Assignment

For publishing work, require an envelope from `publisher` with the exact
channel, manifest-derived dispatch plan, matching preflight contract, and
matching preflight result. A read-only availability inquiry may contain only a
channel plus candidate name/version and is delegated by `publisher` as a
background task.

## Gate and retry

- Require every relevant preflight check to be `passed`; otherwise return
  `blocked` for missing evidence or `failed` for negative evidence.
- `required` is contract metadata, never a check-result status. Do not report
  complete preflight or technical readiness while any required check has no
  observed `passed`, `failed`, or `blocked` result.
- Never ask for, inspect, print, or substitute a token.
- Dispatch only the assigned channel workflow. A passed channel is immutable.
- For an authorized retry, re-check current state and retry only the failed
  channel on the same tag/ref.

## Result

Return a fenced JSON object to the parent `publisher` task:

```json
{
  "channel": "<channel>",
  "status": "passed|failed|blocked|apparently_available|taken|indeterminate",
  "checks": [{"kind": "<check>", "status": "passed|failed|blocked"}],
  "verification": ["<non-secret fact>"],
  "sanitized_diagnostic": "<empty on success>"
}
```
