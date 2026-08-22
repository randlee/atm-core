from __future__ import annotations

from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest import mock

from scripts.smoke import benchmark_account as ACCOUNT
from scripts.smoke import benchmark_snapshot as SNAPSHOT


class BenchmarkSnapshotTests(unittest.TestCase):
    def _account(self, home: Path) -> ACCOUNT.BenchmarkAccount:
        with (
            mock.patch.object(ACCOUNT, "account_home", return_value=home),
            mock.patch.object(ACCOUNT, "current_account_id", return_value="uid:4242"),
        ):
            account = ACCOUNT.bootstrap_benchmark_account()
        account.durable_state_root.mkdir()
        return account

    def _database(self, account: ACCOUNT.BenchmarkAccount, entries: int) -> Path:
        database = account.durable_state_root / SNAPSHOT.MAIL_DATABASE_NAME
        with sqlite3.connect(database) as connection:
            connection.execute("CREATE TABLE entries(value INTEGER NOT NULL)")
            connection.executemany("INSERT INTO entries(value) VALUES (?1)", [(value,) for value in range(entries)])
            connection.execute("PRAGMA user_version = 52")
        return database

    def _snapshot(self, account: ACCOUNT.BenchmarkAccount) -> SNAPSHOT.VerifiedSnapshot:
        with mock.patch.object(SNAPSHOT, "require_benchmark_account", return_value=account):
            return SNAPSHOT.create_verified_snapshot()

    def _verify(self, account: ACCOUNT.BenchmarkAccount, snapshot_id: str) -> SNAPSHOT.VerifiedSnapshot:
        with mock.patch.object(SNAPSHOT, "require_benchmark_account", return_value=account):
            return SNAPSHOT.verify_completed_snapshot(snapshot_id)

    def test_snapshot_is_sqlite_verified_hashed_and_account_bound(self):
        with tempfile.TemporaryDirectory() as temporary:
            account = self._account(Path(temporary))
            self._database(account, 3)

            snapshot = self._snapshot(account)

            self.assertTrue((snapshot.directory / SNAPSHOT.SNAPSHOT_MANIFEST_NAME).is_file())
            self.assertGreater(snapshot.byte_count, 0)
            self.assertEqual(snapshot.user_version, 52)
            self.assertGreater(snapshot.page_count, 0)
            self.assertEqual(snapshot, self._verify(account, snapshot.snapshot_id))

    def test_tampered_completed_snapshot_is_not_a_restore_candidate(self):
        with tempfile.TemporaryDirectory() as temporary:
            account = self._account(Path(temporary))
            self._database(account, 1)
            snapshot = self._snapshot(account)
            snapshot.database.write_bytes(b"tampered")

            with self.assertRaisesRegex(SNAPSHOT.BenchmarkSnapshotError, "SQLite verification failed|does not match"):
                self._verify(account, snapshot.snapshot_id)

    def test_interrupted_snapshot_preserves_staging_without_publishing_candidate(self):
        with tempfile.TemporaryDirectory() as temporary:
            account = self._account(Path(temporary))
            self._database(account, 1)
            with mock.patch.object(SNAPSHOT, "_write_manifest", side_effect=OSError("injected interruption")):
                with self.assertRaisesRegex(SNAPSHOT.BenchmarkSnapshotError, "preserved staging material"):
                    self._snapshot(account)

            root = account.home / ".atm" / SNAPSHOT.SNAPSHOT_ROOT_NAME
            self.assertEqual(list(root.glob("snapshot-*")), [])
            staged = list(root.glob(".snapshot-*.staging"))
            self.assertEqual(len(staged), 1)
            self.assertFalse((staged[0] / SNAPSHOT.SNAPSHOT_MANIFEST_NAME).exists())

    def test_restore_activates_only_a_verified_staged_database(self):
        with tempfile.TemporaryDirectory() as temporary:
            account = self._account(Path(temporary))
            database = self._database(account, 1)
            snapshot = self._snapshot(account)
            with sqlite3.connect(database) as connection:
                connection.execute("INSERT INTO entries(value) VALUES (99)")

            with mock.patch.object(SNAPSHOT, "require_benchmark_account", return_value=account):
                restored = SNAPSHOT.restore_verified_snapshot(snapshot.snapshot_id)

            with sqlite3.connect(database) as connection:
                observed = connection.execute("SELECT COUNT(*) FROM entries").fetchone()
            self.assertEqual(observed, (1,))
            self.assertEqual(restored, snapshot)
            self.assertEqual(list(account.durable_state_root.glob(".mail.db.restore-staging-*")), [])

    def test_restore_refuses_tampered_snapshot_without_changing_live_database(self):
        with tempfile.TemporaryDirectory() as temporary:
            account = self._account(Path(temporary))
            database = self._database(account, 1)
            snapshot = self._snapshot(account)
            with sqlite3.connect(database) as connection:
                connection.execute("INSERT INTO entries(value) VALUES (99)")
            snapshot.database.write_bytes(b"tampered")

            with mock.patch.object(SNAPSHOT, "require_benchmark_account", return_value=account):
                with self.assertRaises(SNAPSHOT.BenchmarkSnapshotError):
                    SNAPSHOT.restore_verified_snapshot(snapshot.snapshot_id)
            with sqlite3.connect(database) as connection:
                observed = connection.execute("SELECT COUNT(*) FROM entries").fetchone()
            self.assertEqual(observed, (2,))

    def test_restore_refuses_sidecars_instead_of_deleting_or_swapping_them(self):
        with tempfile.TemporaryDirectory() as temporary:
            account = self._account(Path(temporary))
            database = self._database(account, 1)
            snapshot = self._snapshot(account)
            sidecar = database.with_name(f"{database.name}-wal")
            sidecar.write_text("active sidecar", encoding="utf-8")

            with mock.patch.object(SNAPSHOT, "require_benchmark_account", return_value=account):
                with self.assertRaisesRegex(SNAPSHOT.BenchmarkSnapshotError, "daemon to be stopped"):
                    SNAPSHOT.restore_verified_snapshot(snapshot.snapshot_id)
            self.assertEqual(sidecar.read_text(encoding="utf-8"), "active sidecar")


if __name__ == "__main__":
    unittest.main()
