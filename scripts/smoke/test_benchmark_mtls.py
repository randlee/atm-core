from __future__ import annotations

import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

from scripts.smoke.benchmark_account import BenchmarkAccount
from scripts.smoke import benchmark_mtls as MTLS


class BenchmarkMtlsTests(unittest.TestCase):
    def _account(self, home: Path) -> BenchmarkAccount:
        return BenchmarkAccount(
            account_id="uid:4242",
            home=home,
            durable_state_root=home / ".atm" / "db",
            manifest_path=home / ".atm" / "benchmark-account.json",
        )

    def test_regeneration_publishes_account_owned_bundle_and_uses_peer_command(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            (home / ".atm").mkdir()
            atm = home / "atm"
            atm.touch()
            commands: list[list[str]] = []

            def run(command, **_kwargs):
                commands.append(command)
                if command[1:3] == ["req", "-x509"]:
                    Path(command[command.index("-out") + 1]).write_text("CERT", encoding="utf-8")
                    Path(command[command.index("-keyout") + 1]).write_text("KEY", encoding="utf-8")
                    return subprocess.CompletedProcess(command, 0, "", "")
                if command[1] == "x509":
                    return subprocess.CompletedProcess(command, 0, "sha256 Fingerprint=" + "AB:" * 31 + "AB\n", "")
                return subprocess.CompletedProcess(command, 0, "", "")

            with (
                mock.patch.object(MTLS.shutil, "which", return_value="/usr/bin/openssl"),
                mock.patch.object(MTLS.subprocess, "run", side_effect=run),
                mock.patch.object(MTLS.os, "chmod", wraps=os.chmod) as chmod,
            ):
                fingerprint = MTLS.regenerate_mtls_identity(self._account(home), atm)

            bundle = home / ".atm" / MTLS.IDENTITY_DIRECTORY_NAME / MTLS.IDENTITY_BUNDLE_NAME
            self.assertEqual(fingerprint, "ab" * 32)
            self.assertEqual(bundle.read_text(encoding="utf-8"), "CERTKEY")
            if os.name != "nt":
                self.assertEqual(bundle.stat().st_mode & 0o777, 0o600)
            chmod_calls = [(Path(call.args[0]).name, call.args[1]) for call in chmod.call_args_list]
            self.assertIn(("private-key.pem", 0o600), chmod_calls)
            self.assertIn((MTLS.IDENTITY_BUNDLE_NAME, 0o600), chmod_calls)
            self.assertEqual(commands[-1][-1], "--yes")
            self.assertIn(str(bundle), commands[-1])
            self.assertIn(fingerprint, commands[-1])
            self.assertIn("extendedKeyUsage=serverAuth,clientAuth", commands[0])
            self.assertIn("subjectAltName=DNS:atm-benchmark.local,DNS:localhost", commands[0])

    def test_regeneration_refuses_a_symlinked_identity_location(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            (home / ".atm").mkdir()
            destination = home / "elsewhere"
            destination.mkdir()
            (home / ".atm" / MTLS.IDENTITY_DIRECTORY_NAME).symlink_to(destination)
            atm = home / "atm"
            atm.touch()
            with self.assertRaisesRegex(MTLS.BenchmarkMtlsError, "real directory"):
                MTLS.regenerate_mtls_identity(self._account(home), atm)


if __name__ == "__main__":
    unittest.main()
