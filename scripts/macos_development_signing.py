"""Shared development signing rules for local ATM binaries."""

from __future__ import annotations

from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import subprocess
from typing import Sequence


APPLE_DEVELOPMENT_PREFIX = "Apple Development"
DEFAULT_TEAM_IDENTIFIER = "4869P2ZYC6"
SIGNING_IDENTITY_ENVIRONMENT_VARIABLE = "ATM_SIGNING_IDENTITY"
SIGNING_IDENTITY_CONFIG_FILENAME = "signing-identity.json"
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

    @property
    def is_apple_issued(self) -> bool:
        """Return whether this is the Apple Development identity path."""
        return self.common_name.startswith(APPLE_DEVELOPMENT_PREFIX)


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


def unique_identities(identities: Sequence[SigningIdentity]) -> tuple[SigningIdentity, ...]:
    """Return one identity per certificate fingerprint in stable order.

    ``security find-identity`` can enumerate one usable certificate more than
    once.  A signing decision is about the certificate fingerprint, not the
    number of rows emitted by that command.
    """
    selected: dict[str, SigningIdentity] = {}
    for identity in identities:
        selected.setdefault(identity.fingerprint, identity)
    return tuple(selected.values())


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
    return unique_identities(parse_identities(result.stdout))


def _identity_with_team_identifier(identity: SigningIdentity) -> SigningIdentity | None:
    team_identifier = _certificate_team_identifier(identity.common_name)
    if team_identifier is None:
        return None
    return SigningIdentity(identity.fingerprint, identity.common_name, team_identifier)


def signing_identity_config_path() -> Path:
    """Return the durable per-user signing identity configuration path."""
    root = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    return root / "atm" / SIGNING_IDENTITY_CONFIG_FILENAME


def _validated_fingerprint(value: object) -> str | None:
    if not isinstance(value, str):
        return None
    fingerprint = value.strip().upper()
    return fingerprint if re.fullmatch(r"[0-9A-F]{40}", fingerprint) else None


def _load_configured_identity() -> tuple[str, str] | None:
    path = signing_identity_config_path()
    if not path.is_file():
        return None
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SigningIdentityError(f"unable to read signing identity configuration {path}: {error}") from error
    if not isinstance(data, dict):
        raise SigningIdentityError(f"signing identity configuration must be an object: {path}")
    fingerprint = _validated_fingerprint(data.get("fingerprint"))
    common_name = data.get("common_name")
    if fingerprint is None or not isinstance(common_name, str) or not common_name.strip():
        raise SigningIdentityError(
            f"signing identity configuration must contain a 40-character fingerprint and common_name: {path}"
        )
    return fingerprint, common_name.strip()


def save_signing_identity(identity: SigningIdentity) -> None:
    """Persist the selected certificate pin for later builds and host sessions."""
    path = signing_identity_config_path()
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    path.write_text(
        json.dumps(
            {"common_name": identity.common_name, "fingerprint": identity.fingerprint.upper()},
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    os.chmod(path, 0o600)


def _resolve_explicit_identity(identity: SigningIdentity) -> SigningIdentity | None:
    """Resolve either an Apple-issued or explicitly selected self-signed identity."""
    if not identity.is_apple_issued:
        return SigningIdentity(identity.fingerprint.upper(), identity.common_name)
    return _identity_with_team_identifier(identity)


def _configured_identity(identities: Sequence[SigningIdentity]) -> SigningIdentity | None:
    configured = _load_configured_identity()
    if configured is None:
        return None
    fingerprint, common_name = configured
    candidates = tuple(
        identity
        for identity in identities
        if identity.fingerprint.upper() == fingerprint and identity.common_name == common_name
    )
    if len(candidates) != 1:
        raise SigningIdentityError(
            "configured signing identity is not installed exactly once: "
            f"{common_name} ({fingerprint})"
        )
    resolved = _resolve_explicit_identity(candidates[0])
    if resolved is None:
        raise SigningIdentityError(
            f"configured Apple Development identity is missing its team identifier: {common_name}"
        )
    return resolved


def _distinct_fingerprints(identities: Sequence[SigningIdentity]) -> tuple[SigningIdentity, ...]:
    """Return identities once per certificate fingerprint, preserving keychain order.

    ``security find-identity`` may emit the same certificate once for every
    keychain in the active search list.  Those rows are aliases for one signing
    identity, not an ambiguous choice.  Different fingerprints deliberately
    remain distinct so the caller continues to fail closed.
    """
    selected: dict[str, SigningIdentity] = {}
    for identity in identities:
        selected.setdefault(identity.fingerprint, identity)
    return tuple(selected.values())


def _identity_exists_but_is_not_valid() -> bool:
    result = _run(["security", "find-identity", "-p", "codesigning"])
    return result.returncode == 0 and any(
        identity.common_name.startswith(APPLE_DEVELOPMENT_PREFIX)
        for identity in parse_identities(result.stdout)
    )


def resolve_apple_development_identity() -> SigningIdentity:
    """Resolve one configured or valid signing identity, or fail with recovery."""
    override = os.environ.get(SIGNING_IDENTITY_ENVIRONMENT_VARIABLE, "").strip()
    valid_identities = _valid_identities()
    if override:
        override_fingerprint = _validated_fingerprint(override)
        candidates = _distinct_fingerprints(tuple(
            resolved
            for identity in valid_identities
            if (
                (override_fingerprint is not None and identity.fingerprint.upper() == override_fingerprint)
                or override == identity.common_name
            )
            if (resolved := _resolve_explicit_identity(identity)) is not None
        ))
        if len(candidates) == 1:
            selected = candidates[0]
            if not selected.is_apple_issued:
                save_signing_identity(selected)
            return selected
        raise SigningIdentityError(
            f"{SIGNING_IDENTITY_ENVIRONMENT_VARIABLE} must identify exactly one valid signing identity."
        )

    configured = _configured_identity(valid_identities)
    if configured is not None:
        return configured

    candidates = _distinct_fingerprints(tuple(
        resolved
        for identity in valid_identities
        if identity.common_name.startswith(APPLE_DEVELOPMENT_PREFIX)
        if (resolved := _identity_with_team_identifier(identity)) is not None
        if resolved.team_identifier == DEFAULT_TEAM_IDENTIFIER
    ))
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
    binary: str,
    expected_identifier: str,
    expected_team_identifier: str = DEFAULT_TEAM_IDENTIFIER,
    *,
    expected_leaf_fingerprint: str | None = None,
    expected_common_name: str | None = None,
) -> bool:
    """Return whether one binary has its stable Apple or pinned self-signed signature."""
    verification = _run(["codesign", "--verify", "--strict", binary])
    if verification.returncode != 0:
        return False
    details = _run(["codesign", "-dvv", binary])
    output = f"{details.stdout}\n{details.stderr}"
    if details.returncode != 0 or f"Identifier={expected_identifier}" not in output.splitlines():
        return False
    if expected_leaf_fingerprint is not None:
        if expected_common_name is not None and f"Authority={expected_common_name}" not in output.splitlines():
            return False
        fingerprint = _validated_fingerprint(expected_leaf_fingerprint)
        if fingerprint is None:
            return False
        requirement = f'certificate leaf = H"{fingerprint}"'
        pinned = _run(["codesign", "--verify", "--strict", f"-R={requirement}", binary])
        return pinned.returncode == 0
    return f"TeamIdentifier={expected_team_identifier}" in output.splitlines()
