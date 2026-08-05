# `atm-core::config`

Owns configuration discovery, precedence rules, alias and bridge-hostname
resolution, and translation into validated core request inputs.

Also owns persisted config/team loading policy:
- deterministic compatibility defaults for documented schema drift
- classification of missing-document, record-level, and document-level failures
- recovery guidance and parser-context preservation for config errors
- refusal to guess identity or routing data during recovery
- parsing and validation of `[atm].claude_jsonl_body_export_max_bytes` for the
  ATM-authored Claude JSONL compatibility-envelope rule
- defaulting that export cap to `128 KiB`, while allowing `0` to force
  retrieval-stub-only ATM-authored JSONL projection
- normalization of `[[atm.post_send_hooks]].command[0]` so leading `~`, `~/`,
  and `~\\` expand to the current user home while relative hook paths still
  resolve from the declaring `.atm.toml`

Launcher-owned `[rmux]` and `[scmux]` sections remain outside this module's
parse/validation surface. The proposed `[scmux]` contract is documented in
[`../../scmux-config-proposal.md`](../../scmux-config-proposal.md); it is
explicitly documentation-only until the launcher owners adopt it.

References:

- Product requirements: `docs/requirements.md` §3.3, §3.4, and §4
- `REQ-P-CONTRACT-001`
- `REQ-P-IDENTITY-001`
- `REQ-P-CONFIG-HEALTH-001`
- `REQ-CORE-CONFIG-001` for `[atm].team_members`, obsolete `[atm].identity`,
  and `[[atm.post_send_hooks]]`
- `REQ-CORE-CONFIG-002` for `[atm].aliases` resolution and canonical address
  rewrite
- `REQ-CORE-CONFIG-003`
- `REQ-CORE-COMPAT-001`
- `REQ-CORE-MAILBOX-001`
- Migration artifact: `docs/archive/file-migration-plan.md`
