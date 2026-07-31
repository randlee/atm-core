# ADR-044 — Public Verification Report Classification

| Field | Value |
| --- | --- |
| ID | ADR-044 |
| Status | Proposed |
| Scope | Durable `site/` verification reports and GitHub Pages publication |
| Relates to | `REQ-CORE-REPORT-001`, ADR-003 |

## Context

GitHub Pages exposes the repository's durable verification reports beyond the
machine that produced them. Hostnames, IP addresses, ATM homes, endpoint
records, filesystem paths, identities, credentials, and message content are
not public verification evidence. A literal OS hostname in a report envelope
would publish infrastructure information without a release need.

## Decision

1. Treat all `site/` report artifacts as public, regardless of current
   repository or Pages visibility. They contain no secrets, credentials, raw
   hostname/IP, endpoint, filesystem path, agent/team identity, or message
   content.
2. Published report envelopes use a caller-supplied, non-identifying
   `host_label`, not an observed hostname. It is validated as a safe opaque
   label and may describe a test class such as `mac-arm64-01`; raw host data
   stays local to the runner and is never serialized below `site/`.
3. Repository Pages uses GitHub Actions as its source and publishes only the
   generated `site/` directory. A repository administrator must explicitly
   enable that source; disabling Pages leaves the evidence in git but creates
   no alternate publisher.

## Consequences

- Public reports retain transport/platform comparison without disclosing host
  infrastructure.
- Producers must reject unsafe labels before writing an artifact.
- Existing report envelopes with a raw `host` field are migrated or rejected;
  the index never republishes them.

## Alternatives considered

- **Publish literal hostnames because the repository is private:** rejected;
  visibility can change and report artifacts may be copied or linked.
- **Hash hostnames:** rejected; predictable hostnames/IPs are dictionary
  reversible and the hash would preserve unnecessary correlation.
- **Keep raw host data in a second public sidecar:** rejected; it defeats the
  classification boundary.
