# AN.8 fixture provenance

These fixtures are byte-for-byte captures, made by AN.1, of existing
agent-authored inputs that AN.8 must render and validate.  They intentionally
retain the original no-final-newline form where present.

| Captured fixture | Source at capture | SHA-256 |
| --- | --- | --- |
| `task-assignment.xml.j2` | `.claude/skills/codex-orchestration/dev-template.xml.j2` | `b15098d1637765acd5adef8fd5aa1e60245073db0932bc8cf3a500cdb385e523` |
| `task-vars.json` | AN.7 executable team-protocol example | `0fc8f4790494bb7e88a15e23ace2189f5a15cf29aa639f613011750df59eb948` |
| `qa-report.xml.j2` | `.claude/skills/codex-orchestration/qa-template.xml.j2` | `1f67d3b34851ca9b9a12499ca504089eddb1489c7f8e7e1cbb9f3225b74a37e2` |
| `claude_inbox_tmpfile_parser.py` | `scripts/claude_inbox_send.py` | `31d52f507e4a9596cb3961cc6bde0f7f8e023f7e50ac2a2d950fa2c7e6db0a91` |

The `claude_inbox_tmpfile_parser.py` fixture includes both JSON mailbox
parsing and its `NamedTemporaryFile` atomic-write path. It is historical
evidence of the file-oriented inbox workflow, not an analytical parser: it
does not answer the motivating Q1–Q4 questions. AN.8 records this correction
explicitly and validates the durable `decomposed_messages` query replacement
instead of inventing an equivalence the captured script cannot provide.

`dolt-template-sha-vectors.json` records the platform-independent template
identity contract. Its base64 input preserves CRLF, UTF-8 BOM, and
final-newline distinctions across checkout platforms; the future adapter must
strictly decode UTF-8, normalize CRLF and lone CR to LF for hashing, preserve
the BOM and final-newline state, and return the recorded SHA. Thus LF and CRLF
forms of the same text share one `TemplateSha` on Windows, macOS, and Linux.
It is deliberately data-only while AN.1 uses the fixture stub: no local ATM
hashing or parser test may consume it. The exact-pinned public `sc-compose` /
`sc-sha` replacement tracked by
`AN1-FIXTURE-STUB-REPLACEMENT-001.ttl` must wire these vectors into its
cross-platform golden-vector test.
