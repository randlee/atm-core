from __future__ import annotations

from pathlib import Path
import json
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from run_fuzz import FuzzInputError
from run_fuzz import build_result
from run_fuzz import validate_campaign
from run_fuzz import validate_worker_result


FIXTURES = JUST_DIR / "fixtures/fuzz"


class FuzzRunnerTests(unittest.TestCase):
    def test_success_campaign_is_deterministic_and_four_workers(self) -> None:
        campaign = validate_campaign(json.loads((FIXTURES / "success.json").read_text()), Path.cwd())
        first = build_result(campaign)
        second = build_result(campaign)
        self.assertEqual(first, second)
        self.assertEqual([item["correlation_id"] for item in first["workers"]], [
            "shape-probe", "template-probe", "boundary-probe", "differential-probe"
        ])
        self.assertEqual(first["schema_version"], "adversarial-fuzzing/v1")

    def test_timeout_result_is_preserved_as_structured_failure(self) -> None:
        result = json.loads((FIXTURES / "timeout.json").read_text())
        validated = validate_worker_result(result)
        self.assertEqual(validated["status"], "timed_out")
        self.assertEqual(validated["error"]["code"], "worker_timeout")

    def test_malformed_result_fails_closed(self) -> None:
        result = json.loads((FIXTURES / "malformed-result.json").read_text())
        with self.assertRaisesRegex(FuzzInputError, "missing worker result fields"):
            validate_worker_result(result)

    def test_unsafe_worktree_fails_closed(self) -> None:
        payload = json.loads((FIXTURES / "unsafe-path.json").read_text())
        with tempfile.TemporaryDirectory() as tempdir:
            with self.assertRaisesRegex(FuzzInputError, "inside the repository"):
                validate_campaign(payload, Path(tempdir))

    def test_worker_cap_rejects_more_than_four(self) -> None:
        payload = json.loads((FIXTURES / "success.json").read_text())
        payload["max_workers"] = 5
        with self.assertRaisesRegex(FuzzInputError, "max_workers"):
            validate_campaign(payload, Path.cwd())


if __name__ == "__main__":
    unittest.main()
