"""The plan root carries the integration branch validate-plan reads the plan from; it is never guessed."""
from __future__ import annotations

import json
import subprocess
import sys
import unittest
from pathlib import Path

SKILL = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SKILL / "scripts"))
import sc_render  # noqa: E402


def render(values: dict) -> subprocess.CompletedProcess:
    template = SKILL / "templates/plan-root.json.j2"
    try:
        return subprocess.CompletedProcess([str(template)], 0, sc_render.render(template, values), "")
    except sc_render.RenderError as exc:
        return subprocess.CompletedProcess([str(template)], 1, "", sc_render.error_text(exc))


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

    def test_stack_bead_uses_root_values(self):
        template = SKILL / "templates/stack-bead.json.j2"
        bead = json.loads(sc_render.render(template, {**self.values, "id": "atm-phase-bj", "phase": "bj",
                                                      "integration_branch": "integrate/phase-bj"}))
        self.assertEqual(bead["id"], "atm-phase-bj.stack")
        self.assertEqual(bead["dependencies"][0]["depends_on_id"], "atm-phase-bj")
        self.assertEqual(bead["metadata"], {"phase": "bj", "trunk": "integrate/phase-bj",
                                            "stack_number": None, "layers": []})


if __name__ == "__main__":
    unittest.main()
