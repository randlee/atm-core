from __future__ import annotations

import importlib.machinery
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import Mock, patch

SCRIPTS = Path(__file__).parents[1]
sys.path.insert(0, str(SCRIPTS))


class FindingGroupTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("sc-compose"), "sc-compose is required to render bead formulas")
    def test_finding_group_propagates_optional_provenance_to_fix(self):
        loader = importlib.machinery.SourceFileLoader("bead_groups_provenance", str(SCRIPTS / "bead-groups"))
        spec = importlib.util.spec_from_loader(loader.name, loader)
        assert spec is not None
        groups = importlib.util.module_from_spec(spec)
        loader.exec_module(groups)
        runner = object.__new__(groups.Run)
        runner.option = None
        runner.config = {"worktree_base": "/worktrees", "lint_command": "just lint", "policy_path": "policy.md", "max_sanity_rounds": 5}
        runner.bd = Mock()
        runner.bd.show.return_value = None
        stack = {"id": "t-phase-d.stack", "metadata": {"trunk": "integrate/phase-d", "stack_number": None, "layers": []}}
        container = {"status": "open", "title": "d-1: the work", "metadata": {"phase": "d", "sprint": "d-1", "stack": "stack", "difficulty": "normal",
                                                     "layer": 4, "requirements": ["REQ-1"], "adrs": ["NONE"]}}
        runner.show = lambda bead: stack if bead == "t-phase-d.stack" else container
        runner.sidecar = lambda _: {"default_option": "C", "options": {"C": {}}, "ref": "{vars[finding_ref]}-r{vars[round]}"}
        formula = SCRIPTS.parent / "formulas/finding-group.formula.toml.j2"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            runner.repo = root
            findings = root / "findings.json"
            shutil.copyfile(formula, root / "formula.toml.j2")
            for provenance in (15, None):
                row: dict[str, object] = {"ref": "qa1-f1", "severity": "blocking", "reviewer": "qa", "title": "issue", "remedy": "fix"}
                if provenance is not None:
                    row["found_on_layer"] = provenance
                findings.write_text(json.dumps({"sprint": "d-1", "round": 1, "filed_by": "qa",
                                                "found_at_commit": "a" * 40, "findings": [row]}))
                with patch.object(groups.sprint_index_common, "load_phase_config", return_value={"root": "t-phase-d"}), \
                     patch.object(groups, "stack_bead_id", wraps=groups.stack_bead_id) as stack_id:
                    target = runner.finding_targets(findings)[0]
                stack_id.assert_called_once_with("t-phase-d")
                request = {"schema": "sc-compose/beads/v1", "operation": "render", "working_directory": directory,
                           "template": "formula.toml.j2", "rendered_formula": "rendered.formula.toml",
                           "bead_variables": {}, "compose_variables": target["compose"]}
                (root / "request.json").write_text(json.dumps(request))
                output = subprocess.run(["sc-compose", "bead", "render", "--request", "request.json", "--json"],
                                        cwd=root, capture_output=True, text=True, timeout=30)
                self.assertEqual(output.returncode, 0, output.stdout + output.stderr)
                metadata = tomllib.loads((root / "rendered.formula.toml").read_text())["steps"][0]["metadata"]
                if provenance is None:
                    self.assertNotIn("found_on_layer", metadata)
                else:
                    self.assertEqual(metadata["found_on_layer"], provenance)

class FoundOnLayerGuardTests(unittest.TestCase):
    def targets(self, layer):
        loader = importlib.machinery.SourceFileLoader("bead_groups_layer_guard", str(SCRIPTS / "bead-groups"))
        spec = importlib.util.spec_from_loader(loader.name, loader)
        assert spec is not None
        groups = importlib.util.module_from_spec(spec)
        loader.exec_module(groups)
        runner = object.__new__(groups.Run)
        runner.option = None
        runner.config = {"worktree_base": "/worktrees", "lint_command": "just lint", "policy_path": "policy.md", "max_sanity_rounds": 5}
        runner.bd = Mock()
        runner.bd.show.return_value = None
        stack = {"id": "t-phase-d.stack", "metadata": {"trunk": "integrate/phase-d", "stack_number": None, "layers": []}}
        container = {"status": "open", "title": "d-1: the work", "metadata": {"phase": "d", "sprint": "d-1", "stack": "stack", "difficulty": "normal",
                                                     "requirements": ["REQ-1"], "adrs": ["NONE"]}}
        runner.show = lambda bead: stack if bead == "t-phase-d.stack" else container
        runner.sidecar = lambda _: {"default_option": "C", "options": {"C": {}}, "ref": "{vars[finding_ref]}-r{vars[round]}"}
        row = {"ref": "qa1-f1", "severity": "blocking", "reviewer": "qa", "title": "issue", "remedy": "fix"}
        if layer is not None:
            row["found_on_layer"] = layer
        with tempfile.TemporaryDirectory() as directory:
            runner.repo = Path(directory)
            findings = runner.repo / "findings.json"
            findings.write_text(json.dumps({"sprint": "d-1", "round": 1, "filed_by": "qa",
                                            "found_at_commit": "a" * 40, "findings": [row]}))
            with patch.object(groups.sprint_index_common, "load_phase_config", return_value={"root": "t-phase-d"}), \
                 patch.object(groups.subprocess, "run") as sc_compose:
                result = runner.finding_targets(findings)
            return groups, result, sc_compose

    def test_string_layer_is_a_typed_error_and_nothing_is_composed(self):
        for layer in ("main (39cfd9210, 0.18.1 request)", "5", True):
            groups, result, sc_compose = self.targets(layer)
            self.assertEqual(len(result), 1)
            self.assertIsInstance(result[0], groups.TargetError)
            self.assertEqual(result[0].code, "BEAD_GROUPS_FOUND_ON_LAYER_NOT_INTEGER")
            self.assertIn("qa1-f1", str(result[0]))
            self.assertIn("omit found_on_layer", str(result[0]))
            sc_compose.assert_not_called()

    def test_integer_or_absent_layer_proceeds(self):
        for layer, expected in ((4, True), (None, False)):
            groups, result, _ = self.targets(layer)
            self.assertNotIsInstance(result[0], groups.TargetError)
            self.assertEqual(result[0]["compose"]["has_found_on_layer"], expected)


class PhaseEndFindingTests(unittest.TestCase):
    def targets(self, **extra):
        loader = importlib.machinery.SourceFileLoader("bead_groups_phase_end", str(SCRIPTS / "bead-groups"))
        spec = importlib.util.spec_from_loader(loader.name, loader)
        assert spec is not None
        groups = importlib.util.module_from_spec(spec)
        loader.exec_module(groups)
        runner = object.__new__(groups.Run)
        runner.option = None
        runner.config = {"worktree_base": "/worktrees", "lint_command": "just lint", "policy_path": "policy.md", "max_sanity_rounds": 5}
        runner.bd = Mock()
        runner.bd.show.return_value = None
        stack = {"id": "t-phase-d.stack", "metadata": {"trunk": "integrate/phase-d", "stack_number": None, "layers": []}}
        epic = {"status": "open", "title": "phase-d: feature", "metadata": {"integration_branch": "integrate/phase-d", "phase": "d", "plan_scope": "epic"}}
        runner.show = lambda bead: stack if bead == "t-phase-d.stack" else epic
        runner.sidecar = lambda _: {"default_option": "C", "options": {"C": {}}, "ref": "{vars[finding_ref]}-r{vars[round]}"}
        row = {"ref": "pe-f1", "severity": "blocking", "reviewer": "review", "title": "issue", "remedy": "fix",
               "requirements": ["REQ-1"], "adrs": ["NONE"], **extra}
        with tempfile.TemporaryDirectory() as directory:
            runner.repo = Path(directory)
            findings = runner.repo / "findings.json"
            findings.write_text(json.dumps({"sprint": "t-phase-d", "round": 1, "filed_by": "review",
                                            "found_at_commit": "a" * 40, "findings": [row]}))
            with patch.object(groups.sprint_index_common, "load_phase_config", return_value={"root": "t-phase-d"}):
                return groups, runner.finding_targets(findings)

    def test_phase_epic_finding_pours_with_derived_facts(self):
        groups, result = self.targets(difficulty="hard")
        self.assertNotIsInstance(result[0], groups.TargetError)
        compose = result[0]["compose"]
        self.assertEqual((compose["phase"], compose["sprint"], compose["stack"], compose["difficulty"]),
                         ("d", "t-phase-d", "stack", "hard"))
        self.assertEqual(compose["pr_target"], "integrate/phase-d")
        self.assertEqual(compose["branch"], "fix/t-phase-d-pe-f1")

    def test_phase_epic_finding_without_difficulty_is_refused(self):
        groups, result = self.targets()
        self.assertIsInstance(result[0], groups.TargetError)
        self.assertEqual(result[0].code, "BEAD_GROUPS_FINDINGS_INVALID")
        self.assertIn("difficulty", str(result[0]))

    def test_found_on_layer_guard_still_applies_to_phase_end(self):
        groups, result = self.targets(difficulty="hard", found_on_layer="main (abc)")
        self.assertEqual(result[0].code, "BEAD_GROUPS_FOUND_ON_LAYER_NOT_INTEGER")
