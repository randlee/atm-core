from __future__ import annotations

import importlib.machinery
import importlib.util
from pathlib import Path
import sys
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
        report = {"run_id": "paired", "reviewer": "sanity-jev", "operational_reviewer": "sanity-llm"}
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


if __name__ == "__main__":
    unittest.main()
