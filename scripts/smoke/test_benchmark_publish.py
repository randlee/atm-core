"""Tests for publication of the bounded benchmark report surface."""
from __future__ import annotations

from pathlib import Path
import subprocess
import unittest

from scripts.smoke.benchmark_publish import REPORT_ARTIFACTS, main, publish


class BenchmarkPublishTests(unittest.TestCase):
    def test_stages_only_report_artifacts_after_index_check(self) -> None:
        calls: list[tuple[list[str], dict[str, object]]] = []

        def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
            calls.append((command, kwargs))
            return subprocess.CompletedProcess(command, 0, "", "")

        publish(Path("/repo"), run)

        self.assertEqual(calls[0][0], ["just", "reports-index", "--check"])
        self.assertEqual(
            calls[1][0],
            ["git", "add", "--", *(path.as_posix() for path in REPORT_ARTIFACTS)],
        )
        self.assertNotIn("unrelated.txt", calls[1][0])

    def test_does_not_stage_anything_when_index_check_fails(self) -> None:
        calls: list[list[str]] = []

        def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
            calls.append(command)
            return subprocess.CompletedProcess(command, 1, "", "index stale")

        with self.assertRaisesRegex(RuntimeError, "index stale"):
            publish(Path("/repo"), run)
        self.assertEqual(calls, [["just", "reports-index", "--check"]])

    def test_public_recipe_delegates_only_to_the_bounded_publisher(self) -> None:
        justfile = (Path(__file__).resolve().parents[2] / "Justfile").read_text(encoding="utf-8")
        self.assertIn("benchmark-publish:\n    {{python_cmd}} scripts/smoke/benchmark_publish.py", justfile)

    def test_publisher_rejects_arguments_without_any_staging(self) -> None:
        self.assertEqual(main(["unexpected"]), 2)


if __name__ == "__main__":
    unittest.main()
