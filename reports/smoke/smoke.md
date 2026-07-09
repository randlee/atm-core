# Smoke

- status: `passed`
- timestamp: `2026-07-09T07:24:12.082687+00:00`
- binary SHA: `f7a4991f1a31995307b258c4e3148a461337672a`
- duration secs: `1.389`
- summary: `pass=5`, `fail=0`, `skip=0`
- row semantics: `PASS` means every command in the row exited `0`; `FAIL`
  records the first failing command only and does not claim sibling commands in
  that row were executed after the failure

| Row | Flow | Verdict | Notes |
| --- | --- | --- | --- |
| `AD29-POSTSEND-EXTERNAL-001` | external post-send hook success suppresses built-in fallback while preserving durable send success | `PASS` | external post-send hook success keeps the built-in nudge path inactive while durable send success remains intact |
| `AD29-POSTSEND-PARTIAL-001` | mixed post-send hook outcomes preserve durable delivery while surfacing sender-visible warnings | `PASS` | mixed hook accounting preserves durable delivery success and retains a sender-visible warning for failed matches |
| `AD29-POSTSEND-BUILTIN-001` | built-in fallback covers both tmux and graft recipients when no external hook matches | `PASS` | built-in fallback stays honest for both tmux-backed and graft-backed recipients when no external hook matches |
| `AD29-POSTSEND-RESET-001` | deleting a prior override row restores the built-in default template path | `PASS` | removing a stored override row re-exposes the built-in default template instead of leaving an implicit disabled state behind |
| `AD29-POSTSEND-DISABLE-001` | explicitly disabled built-in template state skips local post-send delivery cleanly | `PASS` | the explicit disabled-template state becomes a documented no-delivery path instead of an accidental empty-string side effect |
