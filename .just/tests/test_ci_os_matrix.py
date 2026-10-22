"""Deterministic coverage for the CI operating-system policy."""

from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "ci_os_matrix", ROOT / ".github/scripts/ci_os_matrix.py"
)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class CiOsMatrixTests(unittest.TestCase):
    def test_pr_target_matrix(self) -> None:
        expected = {
            "integrate/phase-bk": False,
            "sprint/bk-1": False,
            "develop": True,
            "main": True,
        }
        all_oses = ["ubuntu-latest", "macos-latest", "windows-latest"]
        for base, full_os in expected.items():
            with self.subTest(base=base):
                matrix = MODULE.select_matrices("pull_request", base, "", False)
                test_rows = json.loads(matrix["test_matrix"])
                self.assertEqual([row["os"] for row in test_rows], all_oses)
                self.assertEqual(
                    [row["run_platform"] for row in test_rows],
                    [True, full_os, full_os],
                )
                self.assertEqual(
                    [row["os"] for row in json.loads(matrix["lint_matrix"])],
                    all_oses if full_os else all_oses[:1],
                )

    def test_push_and_workflow_call(self) -> None:
        for event, ref, workflow_call, full_os in (
            ("push", "refs/heads/develop", False, True),
            ("push", "refs/heads/main", False, True),
            ("push", "refs/heads/feature/x", False, False),
            ("pull_request", "", True, True),
            ("workflow_call", "", False, True),
        ):
            with self.subTest(event=event, ref=ref, workflow_call=workflow_call):
                matrix = MODULE.select_matrices(event, "integrate/phase-bk", ref, workflow_call)
                self.assertEqual(matrix["full_os"], str(full_os).lower())

    def test_workflow_uses_selected_matrices_and_preserves_required_test_names(self) -> None:
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertIn("python .github/scripts/ci_os_matrix.py", workflow)
        self.assertIn("include: ${{ fromJSON(needs.ci-scope.outputs.lint_matrix) }}", workflow)
        self.assertIn("include: ${{ fromJSON(needs.ci-scope.outputs.test_matrix) }}", workflow)
        self.assertEqual(workflow.count("name: Test (${{ matrix.os }})"), 1)
        self.assertNotIn("test-platform-checks:", workflow)


if __name__ == "__main__":
    unittest.main()
