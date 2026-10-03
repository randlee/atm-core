from __future__ import annotations

import importlib.machinery
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).parents[1]
sys.path.insert(0, str(SCRIPTS))
LOADER = importlib.machinery.SourceFileLoader("sanity_create_findings", str(SCRIPTS / "sanity-create-findings"))
SPEC = importlib.util.spec_from_loader("sanity_create_findings", LOADER)
module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(module)


class ParentLayerTests(unittest.TestCase):
    def test_comparison_report_cannot_create_children(self):
        report = {
            "task_id": "t", "checked_bead": "b", "verdict": "FAIL", "commit": "a" * 40,
            "run_id": "paired", "reviewer": "sanity-jev", "operational_reviewer": "sanity-selected",
            "findings": [{"finding_ref": "D1-F1", "deliverable": 1, "kind": "skipped",
                          "file": "src/lib.rs", "line": 1, "issue": "missing",
                          "depends_on": [], "deliverable_text": "Implement D1.",
                          "reviewer": "sc-sanity-jev"}],
        }
        with patch.object(sys, "argv", ["sanity-create-findings", "--task", "t", "--bead", "b",
                                       "--vars", "unused.json", "--reviewer", "sc-sanity-jev", "--actor", "a"]), \
             patch.object(module, "load_vars", return_value=report), \
             patch.object(module, "command") as command:
            self.assertEqual(module.main(), 2)
            command.assert_not_called()

    def test_blocking_child_ranks_one_above_its_parent(self):
        self.assertEqual([module.child_priority("blocking", p) for p in (0, 1, 2, 3, 4, 5)], [1, 1, 1, 2, 3, 4])
        self.assertEqual(module.child_priority("minor", 1), 4)

    def test_sprint_bead_follows_findings_to_the_sprint(self):
        beads = {
            "d-1": {"labels": ["stage:dev", "stage:sprint"], "metadata": {}},
            "d-1-qa-f7": {"labels": ["stage:finding"], "metadata": {"sprint_bead": "d-1"}},
            "d-1-qa-f7.1": {"labels": ["stage:finding"], "metadata": {"sprint_bead": "d-1-qa-f7"}},
        }
        for bead in beads:
            self.assertEqual(module.sprint_bead_of(bead, beads.__getitem__), "d-1")
        beads["orphan"] = {"labels": ["stage:finding"], "metadata": {}}
        with self.assertRaises(module.HandoffError):
            module.sprint_bead_of("orphan", beads.__getitem__)

    def test_dev_bead_layer(self):
        self.assertEqual(module.parent_layer({"layer": 3, "found_on_layer": 1}), 3)

    def test_finding_bead_falls_back_to_found_on_layer(self):
        self.assertEqual(module.parent_layer({"found_on_layer": "2"}), "2")

    def test_neither_is_a_handoff_error(self):
        with self.assertRaises(module.HandoffError):
            module.parent_layer({"phase": "d"})

    def test_selected_mixed_report_creates_only_real_finding_child(self):
        report = {
            "task_id": "sanity", "checked_bead": "checked", "verdict": "FAIL",
            "run_id": "run", "reviewer": "sanity-selected", "operational_reviewer": "sanity-selected",
            "commit": "a" * 40,
            "selection": [
                {"deliverable": 1, "checker_defect": True},
                {"deliverable": 2, "checker_defect": False},
            ],
            "findings": [{
                "finding_ref": "D2-F1", "deliverable": 2, "kind": "skipped",
                "file": "src/lib.rs", "line": 2, "issue": "missing",
                "depends_on": [], "deliverable_text": "Implement D2.", "reviewer": "sc-sanity-jev",
            }],
        }
        parent = {"labels": ["stage:sprint"], "priority": 2, "metadata": {
            "phase": "d", "sprint": "d-1", "stack": "stack", "layer": 1, "difficulty": "normal",
        }}
        with tempfile.TemporaryDirectory() as directory:
            vars_path = Path(directory) / "vars.json"
            vars_path.write_text(json.dumps(report))

            def command(argv, actor, capture=False):
                if argv[1] == "list":
                    return "[]"
                if argv[1] == "show":
                    return json.dumps([parent])
                if argv[1] == "create":
                    return "child-d2"
                if argv[1] == "update":
                    return ""
                raise AssertionError(argv)

            with patch.object(sys, "argv", ["sanity-create-findings", "--task", "sanity", "--bead", "checked",
                                             "--vars", str(vars_path), "--reviewer", "sc-sanity-selected", "--actor", "a"]), \
                 patch.object(module, "command", side_effect=command) as mocked:
                self.assertEqual(module.main(), 0)
            creates = [call for call in mocked.call_args_list if call.args[0][1] == "create"]
            self.assertEqual(len(creates), 1)
            self.assertIn("D2 not done", creates[0].args[0][2])


if __name__ == "__main__":
    unittest.main()
