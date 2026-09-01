from __future__ import annotations

import os
import unittest
from unittest import mock

from scripts.smoke import read_benchmark as benchmark


class ReadBenchmarkProvenanceTests(unittest.TestCase):
    @unittest.skipUnless(benchmark.pwd is not None, "requires a POSIX passwd database")
    def test_capture_returns_process_identity_fields(self) -> None:
        identity = benchmark.capture_execution_identity()
        self.assertTrue(identity.execution_account)
        self.assertGreaterEqual(identity.uid, 0)
        self.assertTrue(identity.home)
        self.assertTrue(identity.hostname)
        self.assertEqual(
            set(identity.as_dict()), {"execution_account", "uid", "home", "hostname"}
        )

    def test_payload_embeds_identity_separately_from_host_label(self) -> None:
        identity = benchmark.ExecutionIdentity(
            execution_account="atmbench",
            uid=501,
            home="/Users/atmbench",
            hostname="rand-m5.local",
        )
        payload = benchmark.build_payload(
            [], benchmark.deterministic_corpus(), "operator-label", identity
        )
        self.assertEqual(payload["host_label"], "operator-label")
        self.assertEqual(payload["execution_identity"], identity.as_dict())

    def test_official_run_fails_closed_when_identity_capture_is_unavailable(self) -> None:
        with mock.patch.object(
            benchmark, "capture_execution_identity",
            side_effect=benchmark.ReadBenchmarkError("identity unavailable"),
        ):
            with mock.patch.dict(
                os.environ, {"ATM_CAPACITY_HOST_LABEL": "rand-m5.local"}, clear=False
            ):
                with self.assertRaisesRegex(benchmark.ReadBenchmarkError, "identity unavailable"):
                    benchmark.execute(list(benchmark.FAMILY_BY_ID), diagnostic_only=False)


if __name__ == "__main__":
    unittest.main()
