"""Safe, structured metadata for repository-owned live smoke reports."""
from __future__ import annotations

import json
from pathlib import Path
import re
from typing import Any, Callable

from smoke_common import SmokeError


Command = Callable[[list[str]], dict[str, Any]]
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
HOST_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,253}$")


def _json_result(result: dict[str, Any], label: str) -> Any:
    if result.get("exit_code") != 0:
        raise SmokeError(f"{label} did not complete successfully")
    try:
        return json.loads(str(result.get("stdout", "")))
    except json.JSONDecodeError as error:
        raise SmokeError(f"{label} did not return JSON") from error


def _configured_interfaces(payload: Any) -> list[str]:
    if not isinstance(payload, list):
        return []
    return sorted(
        {
            interface["advertise_host"]
            for interface in payload
            if isinstance(interface, dict)
            and interface.get("enabled") is not False
            and isinstance(interface.get("advertise_host"), str)
            and HOST_RE.fullmatch(interface["advertise_host"])
        }
    )


def _trusted_peer_fingerprints(payload: Any) -> list[dict[str, Any]]:
    if not isinstance(payload, list):
        return []
    records: list[dict[str, Any]] = []
    for peer in payload:
        if not isinstance(peer, dict):
            continue
        host = peer.get("host")
        fingerprint = peer.get("fingerprint")
        if not isinstance(host, str) or not HOST_RE.fullmatch(host):
            continue
        if not isinstance(fingerprint, str) or not fingerprint:
            continue
        records.append(
            {
                "host": host,
                "fingerprint": fingerprint,
                "enabled": peer.get("enabled") is True,
            }
        )
    return sorted(records, key=lambda peer: peer["host"])


def _local_fingerprint(payload: Any) -> str | None:
    if not isinstance(payload, dict):
        return None
    fingerprint = payload.get("fingerprint")
    return fingerprint if isinstance(fingerprint, str) and fingerprint else None


def _observed_hostnames(cases: list[dict[str, Any]]) -> list[str]:
    values = {
        endpoint
        for case in cases
        if isinstance(case, dict)
        for endpoint in (case.get("origin"), case.get("destination"))
        if isinstance(endpoint, str) and HOST_RE.fullmatch(endpoint)
    }
    return sorted(values)


def _safe_optional_json(command: Command, argv: list[str], label: str) -> Any | None:
    try:
        return _json_result(command(argv), label)
    except SmokeError:
        return None


def collect_live_evidence_metadata(
    *,
    command: Command,
    repo_root: Path,
    atm: str,
    feature: str,
    version: str,
    operating_system: str,
    architecture: str,
    cases: list[dict[str, Any]],
) -> dict[str, Any]:
    """Collect only public, durable facts needed to audit one smoke candidate.

    The helper deliberately selects public certificate fingerprints and trusted
    host names from CLI JSON. It never serializes certificate bundle paths,
    key references, capabilities, command output, or environment values.
    """
    revision = str(command(["git", "-C", str(repo_root), "rev-parse", "HEAD"]).get("stdout", "")).strip()
    if not SHA_RE.fullmatch(revision):
        raise SmokeError("could not resolve the candidate Git SHA for smoke evidence")

    interfaces = _safe_optional_json(
        command, [atm, "peer", "interface", "list", "--json"], "peer interface list"
    )
    certificate = _safe_optional_json(
        command, [atm, "peer", "certificate", "show", "--json"], "peer certificate show"
    )
    trusted_peers = _safe_optional_json(
        command, [atm, "peer", "trust", "list", "--json"], "peer trust list"
    )
    configured_hosts = _configured_interfaces(interfaces)
    trusted = _trusted_peer_fingerprints(trusted_peers)
    registered_hosts = sorted(
        set(configured_hosts) | {peer["host"] for peer in trusted} | set(_observed_hostnames(cases))
    )
    return {
        "candidate": {"git_sha": revision, "version": version},
        "environment": {"os": operating_system, "architecture": architecture},
        "registered_hostnames": registered_hosts,
        "public_tls_fingerprints": {
            "local": _local_fingerprint(certificate),
            "trusted_peers": trusted,
        },
        "commands": [
            f"just smoke {feature}",
            "atm doctor --json",
            "atm peer interface list --json",
            "atm peer certificate show --json",
            "atm peer trust list --json",
        ],
    }
