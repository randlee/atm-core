from __future__ import annotations

import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from scripts.smoke import benchmark_account as ACCOUNT


class BenchmarkAccountTests(unittest.TestCase):
    @unittest.skipUnless(os.name == "nt", "Windows profile resolution only")
    def test_windows_account_home_ignores_userprofile_override(self):
        actual_profile = Path("C:/token-owned-profile")
        with (
            mock.patch.dict(os.environ, {"USERPROFILE": "C:/spoofed-profile"}),
            mock.patch.object(ACCOUNT, "_windows_profile_home", return_value=actual_profile),
        ):
            self.assertEqual(ACCOUNT.account_home(), actual_profile)

    @unittest.skipUnless(os.name == "nt", "Windows profile resolution only")
    def test_windows_token_profile_home_is_an_existing_absolute_directory(self):
        profile = ACCOUNT._windows_profile_home()
        self.assertTrue(profile.is_absolute())
        self.assertTrue(profile.is_dir())

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

    @unittest.skipUnless(os.name == "nt", "Windows owner verification uses Win32 APIs")
    def test_windows_owner_lookup_matches_executing_account_sid(self):
        with tempfile.TemporaryDirectory() as temporary:
            manifest = Path(temporary) / ACCOUNT.MANIFEST_NAME
            manifest.write_text("{}", encoding="utf-8")
            self.assertEqual(ACCOUNT._windows_file_owner(manifest), ACCOUNT._windows_current_principal())

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

    def test_clear_removes_only_disposable_databases_and_keeps_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            account = self._bootstrap(home)
            snapshot = home / ".atm" / "benchmark-snapshots" / "snapshot-test"
            snapshot.mkdir(parents=True)
            (snapshot / "mail.db").write_bytes(b"snapshot")
            account.durable_state_root.mkdir()
            (account.durable_state_root / "mail.db").write_bytes(b"benchmark")
            retained = home / ".atm" / "benchmark-account.json"
            with (
                mock.patch.object(ACCOUNT, "account_home", return_value=home),
                mock.patch.object(ACCOUNT, "current_account_id", return_value="uid:4242"),
            ):
                self.assertEqual(ACCOUNT.clear_benchmark_database_state(), account)
            self.assertTrue(retained.is_file())
            self.assertFalse(account.durable_state_root.exists())
            self.assertFalse(snapshot.parent.exists())

    def test_clear_refuses_a_symlinked_disposable_database_root(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary)
            account = self._bootstrap(home)
            replacement = home / "replacement"
            replacement.mkdir()
            account.durable_state_root.symlink_to(replacement, target_is_directory=True)
            with (
                mock.patch.object(ACCOUNT, "account_home", return_value=home),
                mock.patch.object(ACCOUNT, "current_account_id", return_value="uid:4242"),
            ):
                with self.assertRaisesRegex(ACCOUNT.BenchmarkAccountError, "must not be a symlink"):
                    ACCOUNT.clear_benchmark_database_state()


if __name__ == "__main__":
    unittest.main()
