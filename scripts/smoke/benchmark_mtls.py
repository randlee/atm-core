"""Minimal mTLS identity bootstrap for the disposable benchmark account.

The physical benchmark runner launches the normal daemon in mutual-TLS mode.
Its identity must therefore outlive the runner's temporary ``ATM_HOME``.
This module owns only a disposable account's PEM bundle and records its path
through the ordinary ``atm peer certificate init`` command; it owns no peer
trust, listener, or runtime lifecycle.

Each regeneration creates a self-signed certificate solely for a temporary,
account-scoped benchmark daemon.  The two-day validity period deliberately
limits the fixture's lifetime; the runner regenerates it for each mTLS
benchmark campaign.  It is never a production identity, certificate authority,
or reusable trust anchor.
"""
from __future__ import annotations

import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

from scripts.smoke.benchmark_account import BenchmarkAccount


IDENTITY_DIRECTORY_NAME = "benchmark-mtls"
IDENTITY_BUNDLE_NAME = "identity.pem"
class BenchmarkMtlsError(RuntimeError):
    """A disposable benchmark identity could not be regenerated safely."""


def _run(command: list[str], description: str) -> str:
    try:
        result = subprocess.run(command, capture_output=True, text=True, check=False)
    except OSError as error:
        raise BenchmarkMtlsError(f"could not {description}: {error}") from error
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or f"exit {result.returncode}"
        raise BenchmarkMtlsError(f"could not {description}: {detail}")
    return result.stdout


def _identity_directory(account: BenchmarkAccount) -> Path:
    directory = account.home / ".atm" / IDENTITY_DIRECTORY_NAME
    if directory.exists() or directory.is_symlink():
        if directory.is_symlink() or not directory.is_dir():
            raise BenchmarkMtlsError("benchmark mTLS identity directory must be a real directory")
        return directory
    try:
        directory.mkdir(mode=0o700)
    except OSError as error:
        raise BenchmarkMtlsError(f"could not create benchmark mTLS identity directory: {error}") from error
    return directory


def regenerate_mtls_identity(account: BenchmarkAccount, atm: Path) -> str:
    """Regenerate one account-owned PEM bundle and persist its fingerprint.

    The existing bundle remains intact until a freshly generated certificate
    and key have been combined and atomically published. The account has
    already passed the benchmark-account ownership contract before this call.
    """
    openssl = shutil.which("openssl")
    if openssl is None:
        raise BenchmarkMtlsError("benchmark mTLS bootstrap requires the pinned OpenSSL tool")
    if not atm.is_file():
        raise BenchmarkMtlsError(f"benchmark CLI is missing: {atm}")
    directory = _identity_directory(account)
    bundle = directory / IDENTITY_BUNDLE_NAME
    if bundle.is_symlink():
        raise BenchmarkMtlsError("benchmark mTLS identity bundle must not be a symlink")
    with tempfile.TemporaryDirectory(prefix=".identity-", dir=directory) as temporary:
        staging = Path(temporary)
        certificate = staging / "certificate.pem"
        private_key = staging / "private-key.pem"
        _run(
            [
                openssl,
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-sha256",
                "-nodes",
                "-days",
                "2",
                "-subj",
                "/CN=atm-benchmark.local",
                "-addext",
                "basicConstraints=critical,CA:FALSE",
                "-addext",
                "keyUsage=critical,digitalSignature,keyEncipherment",
                "-addext",
                "extendedKeyUsage=serverAuth,clientAuth",
                "-addext",
                "subjectAltName=DNS:atm-benchmark.local,DNS:localhost",
                "-keyout",
                str(private_key),
                "-out",
                str(certificate),
            ],
            "generate a benchmark mTLS certificate",
        )
        try:
            os.chmod(private_key, 0o600)
            bundle_bytes = certificate.read_bytes() + private_key.read_bytes()
            if not bundle_bytes:
                raise OSError("generated PEM files were empty")
            published = staging / IDENTITY_BUNDLE_NAME
            published.write_bytes(bundle_bytes)
            os.chmod(published, 0o600)
        except OSError as error:
            raise BenchmarkMtlsError(f"could not assemble benchmark mTLS bundle: {error}") from error
        fingerprint_output = _run(
            [openssl, "x509", "-in", str(published), "-noout", "-fingerprint", "-sha256"],
            "read the benchmark mTLS certificate fingerprint",
        )
        fingerprint = re.sub(r"[^0-9a-fA-F]", "", fingerprint_output.rsplit("=", 1)[-1]).lower()
        if len(fingerprint) != 64:
            raise BenchmarkMtlsError("generated benchmark mTLS certificate fingerprint was malformed")
        try:
            os.replace(published, bundle)
        except OSError as error:
            raise BenchmarkMtlsError(f"could not publish benchmark mTLS bundle: {error}") from error

    _run(
        [
            str(atm),
            "peer",
            "certificate",
            "init",
            "--fingerprint",
            fingerprint,
            "--private-key-ref",
            str(bundle),
            "--yes",
        ],
        "record the benchmark mTLS identity",
    )
    return fingerprint
