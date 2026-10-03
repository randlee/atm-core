"""The plan root carries the integration branch validate-plan reads the plan from; it is never guessed."""
from __future__ import annotations

import json
import subprocess
import tempfile
import unittest
from pathlib import Path

SKILL = Path(__file__).resolve().parents[1]


def render(values: dict) -> subprocess.CompletedProcess:
    with tempfile.TemporaryDirectory() as d:
        path = Path(d) / "vars.json"
        path.write_text(json.dumps(values))
        return subprocess.run(["sc-compose", "render", "--file", str(SKILL / "templates/plan-root.json.j2"),
                               "--var-file", str(path), "--strict"], capture_output=True, text=True)


class PlanRootTests(unittest.TestCase):
    def setUp(self):
        self.values = json.loads((SKILL / "examples/plan-root-vars.json").read_text())

    def test_integration_branch_is_rendered_as_given(self):
        result = render({**self.values, "integration_branch": "release/phase-d"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["metadata"]["integration_branch"], "release/phase-d")

    def test_integration_branch_is_required(self):
        result = render({k: v for k, v in self.values.items() if k != "integration_branch"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("integration_branch", result.stderr)


if __name__ == "__main__":
    unittest.main()
