"""Shared Apple Development signing rules for local ATM binaries."""

from __future__ import annotations

from dataclasses import dataclass
import os
import re
import subprocess
from typing import Sequence


APPLE_DEVELOPMENT_PREFIX = "Apple Development"
DEFAULT_TEAM_IDENTIFIER = "4869P2ZYC6"
SIGNING_IDENTITY_ENVIRONMENT_VARIABLE = "ATM_SIGNING_IDENTITY"
CLI_IDENTIFIER = "com.synapticcanvas.atm.cli"
DAEMON_IDENTIFIER = "com.synapticcanvas.atm.daemon"
WWDR_G3_INSTRUCTION = (
    "Install Apple WWDR G3 from "
    "https://www.apple.com/certificateauthority/AppleWWDRCAG3.cer, then retry."
)

_IDENTITY_LINE = re.compile(r'^\s*\d+\)\s+([0-9A-F]{40})\s+"(.+)"\s*$')
_TEAM_IDENTIFIER = re.compile(r"(?:^|[,/])OU=([^,/]+)")


class SigningIdentityError(RuntimeError):
    """The local Apple Development identity cannot safely sign ATM binaries."""


@dataclass(frozen=True)
class SigningIdentity:
    """A keychain signing identity selected without machine-name metadata."""

    fingerprint: str
    common_name: str
    team_identifier: str = ""


def _run(command: Sequence[str], *, input_text: str | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        check=False,
        capture_output=True,
        input=input_text,
        text=True,
        timeout=10.0,
    )


def parse_identities(output: str) -> tuple[SigningIdentity, ...]:
    """Parse the valid identity rows emitted by ``security find-identity``."""
    matches = (_IDENTITY_LINE.match(line) for line in output.splitlines())
    return tuple(
        SigningIdentity(match.group(1), match.group(2))
        for match in matches
        if match is not None
    )


def _certificate_team_identifier(common_name: str) -> str | None:
    certificate = _run(["security", "find-certificate", "-c", common_name, "-p"])
    if certificate.returncode != 0 or not certificate.stdout:
        return None
    subject = _run(
        ["openssl", "x509", "-noout", "-subject", "-nameopt", "RFC2253"],
        input_text=certificate.stdout,
    )
    if subject.returncode != 0:
        return None
    match = _TEAM_IDENTIFIER.search(subject.stdout)
    return match.group(1) if match is not None else None


def _valid_identities() -> tuple[SigningIdentity, ...]:
    result = _run(["security", "find-identity", "-v", "-p", "codesigning"])
    if result.returncode != 0:
        return ()
    return parse_identities(result.stdout)


def _identity_with_team_identifier(identity: SigningIdentity) -> SigningIdentity | None:
    team_identifier = _certificate_team_identifier(identity.common_name)
    if team_identifier is None:
        return None
    return SigningIdentity(identity.fingerprint, identity.common_name, team_identifier)


def _identity_exists_but_is_not_valid() -> bool:
    result = _run(["security", "find-identity", "-p", "codesigning"])
    return result.returncode == 0 and any(
        identity.common_name.startswith(APPLE_DEVELOPMENT_PREFIX)
        for identity in parse_identities(result.stdout)
    )


def resolve_apple_development_identity() -> SigningIdentity:
    """Resolve one valid Apple Development identity, or fail with recovery."""
    override = os.environ.get(SIGNING_IDENTITY_ENVIRONMENT_VARIABLE, "").strip()
    valid_identities = _valid_identities()
    if override:
        candidates = tuple(
            resolved
            for identity in valid_identities
            if override in {identity.fingerprint, identity.common_name}
            if (resolved := _identity_with_team_identifier(identity)) is not None
        )
        if len(candidates) == 1:
            return candidates[0]
        raise SigningIdentityError(
            f"{SIGNING_IDENTITY_ENVIRONMENT_VARIABLE} must identify exactly one valid signing identity."
        )

    candidates = tuple(
        resolved
        for identity in valid_identities
        if identity.common_name.startswith(APPLE_DEVELOPMENT_PREFIX)
        if (resolved := _identity_with_team_identifier(identity)) is not None
        if resolved.team_identifier == DEFAULT_TEAM_IDENTIFIER
    )
    if len(candidates) == 1:
        return candidates[0]
    if not candidates and _identity_exists_but_is_not_valid():
        raise SigningIdentityError(
            "Apple Development identity is installed but invalid. " + WWDR_G3_INSTRUCTION
        )
    raise SigningIdentityError(
        "Expected exactly one valid Apple Development identity for team "
        f"{DEFAULT_TEAM_IDENTIFIER}; found {len(candidates)}."
    )


def verify_apple_signature(
    binary: str, expected_identifier: str, expected_team_identifier: str = DEFAULT_TEAM_IDENTIFIER
) -> bool:
    """Return whether one binary has the stable Apple Development signature."""
    verification = _run(["codesign", "--verify", "--strict", binary])
    if verification.returncode != 0:
        return False
    details = _run(["codesign", "-dvv", binary])
    output = f"{details.stdout}\n{details.stderr}"
    return details.returncode == 0 and all(
        line in output.splitlines()
        for line in (
            f"Identifier={expected_identifier}",
            f"TeamIdentifier={expected_team_identifier}",
        )
    )
