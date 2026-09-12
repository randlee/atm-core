from __future__ import annotations

from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import shlex
import socket
from typing import Any

from scripts.smoke.smoke_common import SmokeError, command_result as command

_PUBLIC_NAMESPACE: dict[str, Any] | None = None


def _public(name: str, fallback: Any) -> Any:
    return _PUBLIC_NAMESPACE.get(name, fallback) if _PUBLIC_NAMESPACE is not None else fallback


def _parse_json(result: dict[str, Any], label: str) -> Any:
    if result["exit_code"] != 0:
        raise SmokeError(f"{label} failed: {result['stderr'].strip() or result['stdout'].strip()}")
    try:
        return json.loads(result["stdout"])
    except json.JSONDecodeError as error:
        raise SmokeError(f"{label} did not return JSON: {error}") from error


def _record_case(*args: Any, **kwargs: Any) -> None:
    _public("add_case", lambda *_args, **_kwargs: None)(*args, **kwargs)

def remote_command(peer: str, remote_atm: str, args: list[str], timeout: float = 20.0) -> dict[str, Any]:
    """Invoke only the public CLI on an already-running SSH peer."""
    remote_identity = os.environ.get("ATM_SMOKE_REMOTE_IDENTITY", "").strip()
    remote_team = os.environ.get("ATM_SMOKE_REMOTE_TEAM", "").strip()
    if remote_identity and remote_team:
        return _public("command", command)(
            ["ssh", peer, "env", f"ATM_IDENTITY={remote_identity}", f"ATM_TEAM={remote_team}", remote_atm, *args],
            timeout=timeout,
        )
    return _public("command", command)(["ssh", peer, remote_atm, *args], timeout=timeout)


def remote_context() -> tuple[str, str]:
    """Return the explicit recipient identity required for live peer sends."""
    identity = os.environ.get("ATM_SMOKE_REMOTE_IDENTITY", "").strip()
    team = os.environ.get("ATM_SMOKE_REMOTE_TEAM", "").strip()
    if not identity or not team:
        raise SmokeError(
            "set ATM_SMOKE_REMOTE_IDENTITY and ATM_SMOKE_REMOTE_TEAM for cross-host ATM delivery smoke"
        )
    return identity, team


def remote_shell(peer: str, script: str, timeout: float = 20.0) -> dict[str, Any]:
    """Run one bounded, quoted diagnostic command in the peer's shell."""
    return _public("command", command)(["ssh", peer, f"sh -lc {shlex.quote(script)}"], timeout=timeout)


@contextmanager
def remote_certificate_workspace(peer: str):
    """Yield a unique remote certificate workspace and remove only its files."""
    shell = _public("remote_shell", remote_shell)
    created = shell(peer, "mktemp -d")
    if created["exit_code"] != 0:
        raise SmokeError(f"{peer} could not create a temporary certificate directory: {created['stderr'].strip()}")
    workspace = created["stdout"].strip()
    if not workspace:
        raise SmokeError(f"{peer} returned no temporary certificate directory")

    completed = False
    try:
        yield workspace
        completed = True
    finally:
        local_public = f"{workspace}/local-public.pem"
        peer_public = f"{workspace}/peer-public.pem"
        cleanup = shell(
            peer,
            "rm -f "
            f"{shlex.quote(local_public)} {shlex.quote(peer_public)} "
            f"&& rmdir {shlex.quote(workspace)}",
        )
        if completed and cleanup["exit_code"] != 0:
            raise SmokeError(
                f"{peer} could not remove temporary certificate workspace: {cleanup['stderr'].strip()}"
            )


def certificate_bundle(atm: str) -> str:
    certificate = _parse_json(
        _public("command", command)([atm, "peer", "certificate", "show", "--json"]),
        "peer certificate show",
    )
    bundle = certificate.get("private_key_ref") if isinstance(certificate, dict) else None
    if not isinstance(bundle, str) or not bundle:
        raise SmokeError("peer certificate show did not expose private_key_ref")
    return bundle


def certificate_authority(pem: Path) -> str:
    result = _public("command", command)(
        ["openssl", "x509", "-in", str(pem), "-noout", "-subject", "-nameopt", "RFC2253"]
    )
    if result["exit_code"] != 0:
        raise SmokeError(f"could not inspect public certificate: {result['stderr'].strip()}")
    match = re.search(r"CN=([^,\n]+)", result["stdout"])
    if match is None:
        raise SmokeError("public certificate subject has no common name")
    return match.group(1)


def resolve_dns_addresses(host: str) -> list[str]:
    """Return every address the local resolver provides for a peer hostname."""
    try:
        records = socket.getaddrinfo(
            host, _public("direct_peer_port", lambda: 43101)(), type=socket.SOCK_STREAM
        )
    except OSError as error:
        raise SmokeError(f"DNS resolution for {host} failed: {error}") from error
    return sorted({record[4][0] for record in records})


def remote_resolve_dns_addresses(peer: str, host: str) -> list[str]:
    """Resolve a hostname through the remote computer's normal resolver."""
    port = _public("direct_peer_port", lambda: 43101)()
    script = (
        "import json, socket; "
        f"print(json.dumps(sorted({{item[4][0] for item in socket.getaddrinfo({host!r}, {port}, type=socket.SOCK_STREAM)}})))"
    )
    result = _public("remote_shell", remote_shell)(peer, f"python3 -c {shlex.quote(script)}")
    try:
        addresses = _parse_json(result, f"{peer} DNS resolution for {host}")
    except SmokeError:
        raise
    if not isinstance(addresses, list) or not all(isinstance(address, str) for address in addresses):
        raise SmokeError(f"{peer} DNS resolution for {host} did not return an address list")
    return addresses


def add_dns_case(
    cases: list[dict[str, Any]], name: str, origin: str, destination: str, hostname: str, expected_ip: str,
    resolver: Any,
) -> None:
    """Record real OS DNS resolution and require the daemon's advertised IP."""
    try:
        addresses = resolver(hostname)
        passed = expected_ip in addresses
        detail = f"{hostname} -> {', '.join(addresses)}"
        if not passed:
            detail += f"; missing advertised IP {expected_ip}"
        _record_case(cases, name, passed, detail, origin=origin, destination=destination)
    except SmokeError as error:
        _record_case(cases, name, False, str(error), origin=origin, destination=destination)


def mtls_rejected_before_http(result: dict[str, Any]) -> bool:
    """Return whether curl observed a TLS rejection with no HTTP response.

    The negative smoke trusts the server certificate but supplies no client
    certificate. A nonzero curl exit plus status 000 proves the connection
    stopped in the mTLS handshake, before Hyper can parse an HTTP request or
    the router can dispatch it.
    """
    return result["exit_code"] != 0 and result["stdout"].strip() == "000"


def add_mtls_rejection_case(
    cases: list[dict[str, Any]], name: str, result: dict[str, Any], origin: str, destination: str
) -> None:
    """Record a bounded, public pre-router mTLS negative result."""
    passed = mtls_rejected_before_http(result)
    if passed:
        detail = "mTLS rejected the unauthenticated client before any HTTP status"
    else:
        http_status = result["stdout"].strip() or "no status marker"
        detail = (
            "expected a pre-router mTLS rejection "
            f"(nonzero curl exit and HTTP status 000); got exit={result['exit_code']}, status={http_status}"
        )
    _record_case(cases, name, passed, detail, origin=origin, destination=destination)



__all__ = (
    'remote_command',
    'remote_context',
    'remote_shell',
    'remote_certificate_workspace',
    'certificate_bundle',
    'certificate_authority',
    'resolve_dns_addresses',
    'remote_resolve_dns_addresses',
    'add_dns_case',
    'mtls_rejected_before_http',
    'add_mtls_rejection_case',
 )
