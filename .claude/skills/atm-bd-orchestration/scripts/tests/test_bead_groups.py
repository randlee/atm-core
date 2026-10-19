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
        groups = importlib.util.module_from_spec(importlib.util.spec_from_loader(loader.name, loader))
        loader.exec_module(groups)
        runner = object.__new__(groups.Run)
        runner.option = None
        runner.config = {"worktree_base": "/worktrees"}
        runner.bd = Mock()
        runner.bd.show.return_value = None
        stack = {"id": "t-phase-d.stack", "metadata": {"trunk": "integrate/phase-d", "stack_number": None, "layers": []}}
        container = {"status": "open", "metadata": {"phase": "d", "sprint": "d-1", "stack": "stack", "difficulty": "normal",
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
                row = {"ref": "qa1-f1", "severity": "blocking", "reviewer": "qa", "title": "issue", "remedy": "fix"}
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
