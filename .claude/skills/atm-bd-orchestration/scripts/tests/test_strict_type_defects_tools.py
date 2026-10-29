"""Regressions for runtime defects exposed while making the tools strict."""

import importlib.util
import os
from importlib.machinery import SourceFileLoader
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

JUDGE_PATH = Path(__file__).parents[3] / "prompt-rewrite-judge/scripts/judge.py"
BEAD_GROUPS_PATH = Path(__file__).parents[1] / "bead-groups"
INSTALLER_PATH = Path(__file__).parents[4] / "install.py"
SPEC = importlib.util.spec_from_file_location("prompt_rewrite_judge", JUDGE_PATH)
assert SPEC is not None and SPEC.loader is not None
judge = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(judge)
BEAD_GROUPS_SPEC = importlib.util.spec_from_loader(
    "bead_groups_type_defects", SourceFileLoader("bead_groups_type_defects", str(BEAD_GROUPS_PATH))
)
assert BEAD_GROUPS_SPEC is not None and BEAD_GROUPS_SPEC.loader is not None
bead_groups = importlib.util.module_from_spec(BEAD_GROUPS_SPEC)
BEAD_GROUPS_SPEC.loader.exec_module(bead_groups)


class StrictTypeDefectTests(unittest.TestCase):
    @unittest.skipUnless(INSTALLER_PATH.is_file(), "install.py is not shipped inside the installed skill tree")
    def test_installer_help_and_dependency_preflight_without_site_packages(self):
        environment = os.environ.copy()
        environment.pop("PYTHONPATH", None)
        help_result = subprocess.run(
            [sys.executable, "-S", str(INSTALLER_PATH), "--help"],
            capture_output=True,
            text=True,
            env=environment,
            check=False,
        )
        self.assertEqual(help_result.returncode, 0, help_result.stderr)
        self.assertIn("usage:", help_result.stdout)
        self.assertNotIn("Traceback", help_result.stderr)

        with tempfile.TemporaryDirectory() as directory:
            preflight = subprocess.run(
                [sys.executable, "-S", str(INSTALLER_PATH), "--dest", str(Path(directory) / ".claude")],
                capture_output=True,
                text=True,
                env=environment,
                check=False,
            )
        self.assertEqual(preflight.returncode, 1)
        self.assertIn("Pydantic v2", preflight.stderr)
        self.assertIn("python3 -m pip install", preflight.stderr)
        self.assertNotIn("Traceback", preflight.stderr)

    def run_client(self, output: str) -> tuple[object, object]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            client = root / "client.py"
            client.write_text(f"print({output!r})\n")
            request = root / "request.json"
            request.write_text("{}\n")
            return judge.run(client, request)

    def test_non_object_jev_response_is_a_failure_record(self):
        answers, error = self.run_client("[]\n")

        self.assertIsNone(answers)
        self.assertIsNotNone(error)
        self.assertIn("response must be a JSON object", str(error))
        self.assertIn("[]", str(error))

    def test_selected_choice_without_probability_is_a_failure_record(self):
        response = (
            '{"success":true,"data":{"answers":{"question":'
            '{"choice":"yes","probabilities":{"no":1.0}}}},"error":null}'
        )
        answers, error = self.run_client(response)

        self.assertIsNone(answers)
        self.assertIsNotNone(error)
        self.assertIn("success response has invalid answers", str(error))

    def test_failed_compose_receipt_does_not_require_a_graph(self):
        receipt = {
            "payload": {
                "outcome": "failed",
                "error": {"code": "BEADS_RENDER_FAILED", "message": "undefined variable: extra"},
            }
        }

        self.assertTrue(bead_groups.matches_typeddict(receipt, bead_groups.Receipt))

    def test_missing_finding_remedy_reaches_domain_validation(self):
        document = {
            "sprint": "atm-bn-4",
            "round": 1,
            "filed_by": "atm-bn-4.group-qa",
            "found_at_commit": "e25d5087",
            "findings": [
                {"ref": "qa1-f1", "severity": "blocking", "reviewer": "qa", "title": "finding"}
            ],
        }

        self.assertTrue(bead_groups.matches_typeddict(document, bead_groups.FindingsDocument))


if __name__ == "__main__":
    unittest.main()
