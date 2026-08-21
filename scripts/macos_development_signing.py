"""Shared macOS development-signing identity rules for build and deployment tools."""

from __future__ import annotations

import re


DEVELOPMENT_SIGNING_IDENTITY = "atm-daemon-dev"
_EXACT_IDENTITY_LINE = re.compile(
    rf'^\s*\d+\)\s+\S+\s+"{re.escape(DEVELOPMENT_SIGNING_IDENTITY)}"\s*$'
)


def find_identity_output_has_development_identity(output: str) -> bool:
    """Return whether ``security find-identity`` lists the exact development key."""
    return any(_EXACT_IDENTITY_LINE.match(line) for line in output.splitlines())
