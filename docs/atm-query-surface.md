# ATM query surface v1

ATM stores generic template identity, frontmatter and merged variables. It
does not reserve orchestration vocabulary: agents discover available template
schemas, then form typed queries from the values actually stored.

## Introspection

`atm templates list [--type TYPE] [--json]` lists every immutable registered
revision matching the optional conventional `metadata.type` value. Each row
contains `template_sha`, `template_type`, `template_name`, `first_seen_at`,
and `first_seen_by`. A type can legitimately select several revisions.

`atm templates schema SHA [--json]` performs an exact SHA lookup and returns
the stored frontmatter as `schema_json`; it never chooses an arbitrary
revision during template drift.

## Typed message search

`atm search [TEXT]` and `GET /v1/atm/messages/search` use the same typed
contract. The HTTP endpoint is bodyless: its required `request` URL query
parameter is unpadded URL-safe base64 of the JSON `SearchRequest`, so no
second filter grammar exists at the HTTP boundary. `TEXT` is a literal phrase
by default — punctuation such as `NEAR`,
boolean operators and FTS column syntax have no special meaning. `--raw-match`
enables only ATM's bounded grammar: words, quoted phrases, `AND`, `OR`,
`NOT`, and `NEAR(term term[, distance])` expressions. They may be composed
with the same bounded boolean grammar; parsing finishes before storage sees it
and is never raw FTS5 syntax.

Generic filters are `--template-meta KEY=VALUE`, `--type VALUE` (the `type`
metadata shorthand), `--template-sha SHA`, repeatable `--var KEY=VALUE`,
`--tag`, `--category`, `--from`, `--team`, `--agent`, `--since`, `--until`,
`--limit`, `--cursor`, and `--per-mailbox`. Keys match
`^[A-Za-z_][A-Za-z0-9_-]{0,63}$`; every value is parameterized by the storage
adapter. Simple aggregates use the same filters: `--count`,
`--group-by var:KEY|team|agent|from_agent|template_type|category`, and
`--min message_at` / `--max message_at`.

Template-metadata values are exact by default; a final `*` requests the
documented prefix match (`--type qa-*`). Variable values are always exact.

The HTTP search endpoint is local-only. Authenticated UDS and capability
loopback callers may search; a peer request is rejected by core policy before
any search capability is selected.

## Analyst SQL extension

`atm-query-python` builds the local Python module `atm_query`:

```python
from atm_query import open_readonly

rows = open_readonly().query(
    "SELECT team, agent, template_type FROM decomposed_messages WHERE template_type = ?",
    ("qa-report",),
)
```

It is the analytical escape hatch, not a network client. With no explicit
path, it opens the current OS account's `~/.atm/db/mail.db`, never a
workspace-selected `ATM_HOME` database. It uses `query_only`, defensive
settings, an SQLite authorizer, one prepared read-only statement, parameter
binding, and fixed execution/row/result-byte budgets. The authorizer permits
the versioned `decomposed_messages` view (and the view's internal reads) but
does not expose base tables as a contract. Raw SQL is never accepted by the
ATM CLI, HTTP endpoint, `atm-core`, peer ingress, or `atm-graft`.
