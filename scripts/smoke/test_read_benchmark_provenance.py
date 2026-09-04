from __future__ import annotations

import os
import unittest
from unittest import mock

from scripts.smoke import read_benchmark as benchmark
from scripts.smoke import benchmark_account


class ReadBenchmarkProvenanceTests(unittest.TestCase):
    def test_windows_identity_uses_sid_and_numeric_rid(self) -> None:
        identity = benchmark._windows_execution_identity(
            "sid:S-1-5-21-100-200-300-4000",
            benchmark.Path("C:/Users/benchmark"),
            "windows-x64-01",
        )
        self.assertEqual(identity.execution_account, "sid:S-1-5-21-100-200-300-4000")
        self.assertEqual(identity.uid, 4000)
        self.assertEqual(identity.home, "C:\\Users\\benchmark")
        self.assertEqual(identity.hostname, "windows-x64-01")

    def test_windows_identity_rejects_malformed_sid(self) -> None:
        with self.assertRaisesRegex(benchmark.ReadBenchmarkError, "invalid Windows SID"):
            benchmark._windows_execution_identity(
                "benchmark", benchmark.Path("C:/Users/benchmark"), "windows-x64-01"
            )

    def test_managed_daemon_reaps_only_its_started_child(self) -> None:
        process = object()
        output = mock.Mock()
        with mock.patch(
            "scripts.smoke.run_admission_capacity.start_capacity_daemon",
            return_value=(process, output),
        ) as start, mock.patch(
            "scripts.smoke.run_admission_capacity.reap_owned_daemon",
        ) as reap, mock.patch.object(benchmark, "execute", return_value=0) as execute:
            self.assertEqual(
                benchmark.execute_with_managed_daemon(["read-fanout"], diagnostic_only=True), 0
            )
        self.assertEqual(start.call_args.args[3], "plaintext-test")
        self.assertEqual(start.call_args.args[2]["ATM_DAEMON_READY_STDOUT"], "1")
        execute.assert_called_once_with(["read-fanout"], diagnostic_only=True)
        reap.assert_called_once_with(process)
        output.join.assert_called_once_with()

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
        events: list[str] = []

        def capture_failure() -> None:
            events.append("capture")
            raise benchmark.ReadBenchmarkError("identity unavailable")

        with mock.patch.object(
            benchmark, "capture_execution_identity",
            side_effect=capture_failure,
        ):
            with mock.patch.object(
                benchmark, "load_baselines", side_effect=lambda _path: events.append("baselines")
            ), mock.patch.object(
                benchmark_account, "require_benchmark_account", side_effect=lambda: events.append("account")
            ), mock.patch.object(
                benchmark, "_atm_binary", side_effect=lambda: events.append("atm")
            ), mock.patch.object(
                benchmark, "deterministic_corpus", side_effect=lambda: events.append("corpus")
            ), mock.patch.object(
                benchmark, "prepare_corpus", side_effect=lambda *_args: events.append("prepare")
            ):
                with mock.patch.dict(
                    os.environ, {"ATM_CAPACITY_HOST_LABEL": "rand-m5.local"}, clear=False
                ):
                    with self.assertRaisesRegex(benchmark.ReadBenchmarkError, "identity unavailable"):
                        benchmark.execute(list(benchmark.FAMILY_BY_ID), diagnostic_only=False)
        self.assertEqual(events, ["capture"], "identity must be captured before setup/workload")


if __name__ == "__main__":
    unittest.main()
