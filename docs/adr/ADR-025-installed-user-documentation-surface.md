# ADR-025 — Installed User Documentation Surface

| Field | Value |
|---|---|
| ID | ADR-025 |
| Status | **Accepted** |
| Date | 2026-07-11 |
| Deciders | Rand Lee |
| Relates to | ADR-019, ADR-024 |
| Supersedes | none |

---

## Context

ATM now has a retained `atm help` surface, but long-form operator guidance is
still missing from the installed product. The repo holds developer-facing
requirements and architecture docs, while local installs currently ship no
versioned markdown corpus at all.

That leaves three bad outcomes open:

- operators have no installed long-form reference for hooks, nudge templates,
  mailbox workflows, or diagnostics
- `atm help` is pressured to grow into an oversized pseudo-manual
- release/publisher automation can ship binaries without proving the user-doc
  surface exists or was reviewed for the release version

## Decision

Treat installed end-user documentation as a first-class release artifact.

The accepted architecture is:

- the repo-owned authoring tree is `docs/user-documents/`
- packaging copies that tree into `<install-root>/share/doc/atm/`
- the installed primary entrypoint is `<install-root>/share/doc/atm/README.md`
- the default local install root is `~/.local/atm/<version>/`
- installed-doc lookup canonicalizes the resolved `atm` executable path first,
  then derives the installed doc root from that canonical executable location
  using the executable-relative path `../share/doc/atm/`
- this canonicalization step exists specifically so symlinked installs
  (including `~/.local/bin/atm` shims or `current -> <version>` style version
  links) resolve against the real install root on macOS, Linux, and Windows
- when the executable path is not inside an installed ATM tree (for example a
  dev build under `target/debug`), the installed-doc resolver returns no path
  and the help surface falls back to a deterministic README hint instead of
  consulting `ATM_HOME`
- runtime state under `~/.atm/` remains a distinct runtime/data tree and is
  not the installed doc root
- `ATM_HOME` remains the runtime/data root and is not an installed-doc locator
- `atm help` remains concise and points users to the installed corpus for
  long-form operator guidance
- the installed corpus uses relative links only so the copied tree remains
  navigable unchanged
- fenced `json`, `xml`, `toml`, and `bash` examples in that corpus are release
  artifacts and must validate mechanically
- every end-user doc file carries a metadata header including
  `reviewed_for_release`
- one canonical verifier validates both the repo-owned source tree and the
  staged/installed copied tree

## Consequences

### Positive

- ATM ships one trustworthy long-form user reference with the binaries
- `atm help` stays small and avoids becoming a second full documentation system
- publisher/release automation can fail closed on stale or broken user docs

### Negative

- release packaging must now copy and verify a non-code artifact tree
- documentation freshness becomes a hard release gate rather than optional
  polish

## Review Conditions

This ADR remains valid only while all of the following stay true:

- long-form operator guidance remains in installed markdown rather than new
  help-only commands
- the installed corpus is copied from `docs/user-documents/`
- installed-doc lookup remains executable-relative and does not drift onto
  `ATM_HOME`
- relative-link and fenced-example verification remain mechanical release gates
- user docs remain operator-facing and do not become a backdoor for direct
  database-edit guidance
