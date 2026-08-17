# ADR-049 — hermes-atm/atm-graft First Public PyPI Release Versioning

| Field | Value |
| --- | --- |
| Status | Proposed |
| Relates to | ADR-042; `.just/check_version_sync.py`; PR #897, #906, #907 |

## Context

`hermes-atm` and `atm-graft-python` have never been published to PyPI. Their
package versions are kept in lockstep with the internal workspace release
version via `version.workspace = true` and `.just/check_version_sync.py`
(the sole canonical version-alignment gate, established in PR #897 after
consolidating two earlier duplicate/conflicting checks). The workspace is
currently at `1.4.2`.

quality-mgr's QA-RELEASE-READINESS-PYPI-1 investigation flagged this as an
unresolved gap: publishing a package's *first* public release as `1.4.2`
implies a `1.0.0`–`1.4.1` history that never existed publicly. This can
confuse consumers evaluating the package's maturity/stability via its version
number alone, and there is no existing ADR governing the public-facing
versioning story for these two packages specifically. ADR-042 covers the
CLI/daemon product release, schema, and HTTP API SemVer split, and Homebrew
formula publishing — it does not address PyPI package version numbering.

Two other Phase AL/AM PRs (#906, #907) have already closed every other
PyPI publish-readiness blocker (manylinux/musllinux wheel pipeline, wheel
artifact persistence, Windows/aarch64 targets, sdist verification, manifest
registration, package metadata). This ADR is the last open item before actual
publication; PyPI account/token/Trusted-Publisher setup is tracked
separately and is not a versioning question.

## Decision

Keep `hermes-atm` and `atm-graft-python`'s PyPI version numbers unified with
the internal workspace version, unchanged from current practice. Do not
introduce a second, independently-tracked public version number for these
packages.

To address the "implied history" concern without weakening the
version-alignment guarantee `check_version_sync.py` already enforces,
the first published release's PyPI project description/README and GitHub
release notes must explicitly state that this is the first public release,
and that the `1.x` starting point reflects the version already reached
during this project's internal (pre-public) development — not a gap in
publication history. No `0.x` renumbering, no separate public/internal
version mapping, and no change to `version.workspace = true` or the
`check_version_sync.py` gate.

## Consequences

- No new engineering: the version-sync consolidation work from PR #897
  continues to be the single source of truth, with no dual-tracking
  logic to build or maintain.
- Every future workspace version bump remains automatically reflected in
  the published packages with no separate release-numbering decision
  required per release.
- The published package's version number does not, by itself, communicate
  "this is a first release" — that context must live in the README/release
  notes and stays a documentation obligation each time the packages are
  cited publicly (e.g. announcement posts), not just at first publish.
- A consumer comparing `hermes-atm`'s PyPI version history against its
  publish date will see the package "starting" mid-range; this is
  documented as expected, not investigated as a packaging error, by anyone
  auditing the release later.

## Rejected alternatives

1. **Reset to a new independent public version track (e.g. start at
   `0.1.0` or `1.0.0`), decoupled from the workspace version.** Requires
   building and maintaining a second versioning scheme and an
   internal-to-public version mapping, adds an extra manual step to every
   release, and reopens exactly the kind of dual-canonical-source problem
   `check_version_sync.py` was built to eliminate (see PR #897's
   RBQA-F002 finding). Rejected as disproportionate engineering cost for a
   documentation-shaped problem.
2. **Silently publish at `1.4.2` with no explanation.** Cheapest option,
   but does nothing to address the actual concern raised — a consumer has
   no way to distinguish "mature, stable package" from "internal-only
   history now exposed" without documentation. Rejected as not actually
   resolving the flagged gap.

## Required evidence

- First-publish README/release notes explicitly state this is the initial
  public release and explain the `1.x` starting version, before the first
  `twine upload` (or equivalent) to PyPI/TestPyPI.
- `check_version_sync.py` continues to pass unchanged; this ADR introduces
  no new version-comparison logic to test.
