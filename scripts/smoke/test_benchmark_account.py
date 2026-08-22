from __future__ import annotations

import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from scripts.smoke import benchmark_account as ACCOUNT


class BenchmarkAccountTests(unittest.TestCase):
    def _bootstrap(self, home: Path) -> ACCOUNT.BenchmarkAccount:
        with (
            mock.patch.object(ACCOUNT, "account_home", return_value=home),
            mock.patch.object(ACCOUNT, "current_account_id", return_value="uid:4242"),
        ):
            return ACCOUNT.bootstrap_benchmark_account()

    def _require(self, home: Path) -> ACCOUNT.BenchmarkAccount:
        with (
            mock.patch.object(ACCOUNT, "account_home", return_value=home),
            mock.patch.object(ACCOUNT, "current_account_id", return_value="uid:4242"),
        ):
            return ACCOUNT.require_benchmark_account()

    def test_bootstrap_creates_a_private_account_bound_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            account = self._bootstrap(home)
            payload = json.loads(account.manifest_path.read_text(encoding="utf-8"))

            self.assertEqual(account.home, home)
            self.assertEqual(account.durable_state_root, home / ".atm" / "db")
            self.assertEqual(payload["account_id"], "uid:4242")
            self.assertEqual(payload["home"], str(home))
            self.assertFalse(account.durable_state_root.exists())
            self.assertEqual(self._require(home), account)

    def test_bootstrap_refuses_an_account_with_existing_durable_state(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            (home / ".atm" / "db").mkdir(parents=True)
            with self.assertRaisesRegex(ACCOUNT.BenchmarkAccountError, "existing ATM durable state"):
                self._bootstrap(home)

    def test_require_refuses_missing_manifest_before_any_state_access(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            (home / ".atm").mkdir()
            with self.assertRaisesRegex(ACCOUNT.BenchmarkAccountError, "manifest is missing"):
                self._require(home)
            self.assertFalse((home / ".atm" / "db").exists())

    def test_require_refuses_a_symlinked_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            self._bootstrap(home)
            manifest = home / ".atm" / ACCOUNT.MANIFEST_NAME
            replacement = home / "replacement.json"
            replacement.write_text(manifest.read_text(encoding="utf-8"), encoding="utf-8")
            manifest.unlink()
            manifest.symlink_to(replacement)
            with self.assertRaisesRegex(ACCOUNT.BenchmarkAccountError, "must not be a symlink"):
                self._require(home)

    def test_require_refuses_malformed_or_wrong_account_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            account = self._bootstrap(home)
            valid_payload = account.manifest_path.read_text(encoding="utf-8")
            account.manifest_path.write_text("not json", encoding="utf-8")
            with self.assertRaisesRegex(ACCOUNT.BenchmarkAccountError, "malformed"):
                self._require(home)

            payload = json.loads(valid_payload)
            payload["account_id"] = "uid:other"
            account.manifest_path.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(ACCOUNT.BenchmarkAccountError, "does not match the executing account"):
                self._require(home)

    def test_require_refuses_when_manifest_owner_cannot_be_verified(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            self._bootstrap(home)
            with mock.patch.object(
                ACCOUNT,
                "_verify_manifest_owner",
                side_effect=ACCOUNT.BenchmarkAccountError("owner mismatch"),
            ):
                with self.assertRaisesRegex(ACCOUNT.BenchmarkAccountError, "owner mismatch"):
                    self._require(home)

    @unittest.skipUnless(os.name == "nt", "Windows owner verification uses Windows token SIDs")
    def test_windows_owner_lookup_accepts_an_executing_token_principal(self):
        with tempfile.TemporaryDirectory() as temporary:
            manifest = Path(temporary) / ACCOUNT.MANIFEST_NAME
            manifest.write_text("{}", encoding="utf-8")
            self.assertIn(ACCOUNT._windows_file_owner(manifest), ACCOUNT._windows_current_owner_sids())
            ACCOUNT._verify_manifest_owner(manifest, manifest.stat())

    def test_bootstrap_is_never_an_implicit_runner_side_effect(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            (home / ".atm").mkdir()
            with (
                mock.patch.object(ACCOUNT, "account_home", return_value=home),
                mock.patch.object(ACCOUNT, "current_account_id", return_value="uid:4242"),
            ):
                with self.assertRaisesRegex(ACCOUNT.BenchmarkAccountError, "manifest is missing"):
                    ACCOUNT.require_benchmark_account()
            self.assertFalse((home / ".atm" / ACCOUNT.MANIFEST_NAME).exists())


if __name__ == "__main__":
    unittest.main()
