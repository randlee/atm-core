from __future__ import annotations

import importlib.machinery
import importlib.util
import json
import os
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
from qa_reviewers import qa_capable_reviewers


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
                row: dict[str, object] = {"ref": "qa1-f1", "severity": "blocking", "reviewer": "flaky-test-qa", "title": "issue", "remedy": "fix"}
                if provenance is not None:
                    row["found_on_layer"] = provenance
                findings.write_text(json.dumps({"sprint": "d-1", "round": 1, "filed_by": "qa",
                                                "found_at_commit": "a" * 40, "findings": [row]}))
                with patch.object(groups.sprint_index_common, "load_phase_config", return_value={"root": "t-phase-d", "code_checkout": "../code"}), \
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
                steps = tomllib.loads((root / "rendered.formula.toml").read_text())["steps"]
                assert steps[2]["metadata"]["reviewers_round1"] == ["flaky-test-qa"]
                assert "code-worktrees/fix/d-1-qa1-f1" in target["compose"]["worktree"]
                assert all(step["metadata"]["worktree"] == target["compose"]["worktree"] for step in steps)
                metadata = steps[0]["metadata"]
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
        row = {"ref": "qa1-f1", "severity": "blocking", "reviewer": "flaky-test-qa", "title": "issue", "remedy": "fix"}
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
    def targets(self, code_checkout=None, stack_layers=None, **extra):
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
        if stack_layers is not None:
            stack["metadata"]["layers"] = stack_layers
        epic = {"status": "open", "title": "phase-d: feature", "metadata": {"integration_branch": "integrate/phase-d", "phase": "d", "plan_scope": "epic"}}
        runner.show = lambda bead: stack if bead == "t-phase-d.stack" else epic
        runner.sidecar = lambda _: {"default_option": "C", "options": {"C": {}}, "ref": "{vars[finding_ref]}-r{vars[round]}"}
        row = {"ref": "pe-f1", "severity": "blocking", "reviewer": "flaky-test-qa", "title": "issue", "remedy": "fix",
               "requirements": ["REQ-1"], "adrs": ["NONE"], **extra}
        with tempfile.TemporaryDirectory() as directory:
            runner.repo = Path(directory)
            findings = runner.repo / "findings.json"
            findings.write_text(json.dumps({"sprint": "t-phase-d", "round": 1, "filed_by": "review",
                                            "found_at_commit": "a" * 40, "findings": [row]}))
            with patch.object(groups.sprint_index_common, "load_phase_config", return_value={"root": "t-phase-d", **({"code_checkout": code_checkout} if code_checkout else {})}):
                return groups, runner.finding_targets(findings)

    def test_held_last_layer_is_not_the_fix_target(self):
        layers = [{"pr": 1, "branch": "linked", "head": "a" * 40, "base": "integrate/phase-d", "linked": True},
                  {"pr": 3, "branch": "held", "head": "c" * 40, "base": "pending", "linked": False,
                   "held_on": {"pr": 2, "branch": "pending"}}]
        groups, result = self.targets(stack_layers=layers, difficulty="hard")
        self.assertNotIsInstance(result[0], groups.TargetError)
        self.assertEqual(result[0]["compose"]["pr_target"], "linked")

    def test_passed_unlinked_first_layer_is_the_fix_target(self):
        layers = [{"pr": 1, "branch": "first", "head": "a" * 40, "base": "integrate/phase-d", "linked": False},
                  {"pr": 2, "branch": "held", "head": "b" * 40, "base": "first", "linked": False,
                   "held_on": {"pr": 1, "branch": "first"}}]
        groups, result = self.targets(stack_layers=layers, difficulty="hard")
        self.assertNotIsInstance(result[0], groups.TargetError)
        self.assertEqual(result[0]["compose"]["pr_target"], "first")

    def test_code_checkout_controls_fix_worktree(self):
        groups, result = self.targets(code_checkout="../code", difficulty="hard")
        self.assertNotIsInstance(result[0], groups.TargetError)
        self.assertIn("code-worktrees/fix/t-phase-d-pe-f1", result[0]["compose"]["worktree"])

    @unittest.skipUnless(shutil.which("sc-compose") and os.environ.get("ATM_TEST_BD"),
                         "sc-compose and the CI-pinned embedded bd are required")
    def test_code_checkout_pour_persists_worktrees_for_all_three_beads(self):
        groups = load_groups("bead_groups_code_checkout_pour")
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory).resolve()
            root, checkout, bin_dir = base / "planning", base / "code", base / "bin"
            root.mkdir()
            checkout.mkdir()
            bin_dir.mkdir()
            bd_path = bin_dir / ("bd.exe" if os.name == "nt" else "bd")
            shutil.copy2(os.environ["ATM_TEST_BD"], bd_path)
            env = {key: value for key, value in os.environ.items()
                   if not key.startswith(("BEADS_", "BD_", "DOLT_"))}
            env.update(HOME=str(root), PATH=str(bin_dir) + os.pathsep + os.environ.get("PATH", ""))
            for args in (("init", "--prefix", "t", "--non-interactive", "--quiet"),
                         ("create", "phase", "--id", "t-phase-d"),
                         ("create", "review", "--id", "t-phase-d-review")):
                result = subprocess.run([str(bd_path), *args], cwd=root, env=env, capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

            phase_dir = root / ".atm-bd"
            phase_dir.mkdir(exist_ok=True)
            template_path = ".claude/skills/atm-bd-orchestration/templates/fixture.j2"
            template = root / template_path
            template.parent.mkdir(parents=True)
            template.write_text("fixture")
            (phase_dir / "phase-d.toml").write_text(
                'plan = "plan.jsonl"\nroot = "t-phase-d"\nintegration_branch = "integrate/phase-d"\n'
                'lead = "lead"\ncode_checkout = "../code"\n[templates]\n'
                + "".join(f'{key} = "{template_path}"\n' for key in
                          ("dev", "fix", "sanity", "qa", "plan_review", "review"))
            )
            formula_dir = phase_dir / "formula"
            formula_dir.mkdir()
            for suffix in ("formula.toml.j2", "relations.json"):
                shutil.copy2(SCRIPTS.parent / "formulas" / f"finding-group.{suffix}",
                             formula_dir / f"finding-group.{suffix}")
            findings = root / "findings.json"
            findings.write_text(json.dumps({"sprint": "t-phase-d", "round": 1, "filed_by": "t-phase-d-review",
                "found_at_commit": "a" * 40, "findings": [{"ref": "pe-f1", "severity": "blocking",
                "reviewer": "flaky-test-qa", "title": "issue", "remedy": "fix", "difficulty": "hard",
                "requirements": ["REQ-1"], "adrs": ["NONE"]}]}))
            runner = object.__new__(groups.Run)
            runner.repo, runner.option, runner.sidecars = root, None, {}
            runner.config = {"worktree_base": "/worktrees", "lint_command": "just lint",
                             "policy_path": "policy.md", "max_sanity_rounds": 5}
            runner.bd = Mock()
            runner.bd.show.return_value = None
            stack = {"id": "t-phase-d.stack", "metadata": {"trunk": "integrate/phase-d", "stack_number": None, "layers": []}}
            epic = {"status": "open", "title": "phase-d: feature", "metadata": {"integration_branch": "integrate/phase-d", "phase": "d", "plan_scope": "epic"}}
            runner.show = lambda bead: stack if bead == "t-phase-d.stack" else epic
            with patch.dict(os.environ, env, clear=True):
                target = runner.finding_targets(findings)[0]
                self.assertNotIsInstance(target, groups.TargetError)
                receipt = runner.pour(target, preview=False)
            expected = Path(str(checkout) + "-worktrees") / "fix/t-phase-d-pe-f1"
            self.assertEqual(set(receipt["ids"]), {"fix", "sanity", "qa"})
            for role, bead in receipt["ids"].items():
                result = subprocess.run([str(bd_path), "show", bead, "--json"], cwd=root, env=env,
                                        capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                record = json.loads(result.stdout)[0]
                self.assertEqual(record["metadata"]["worktree"], str(expected), role)
                self.assertEqual(record["metadata"]["branch"], "fix/t-phase-d-pe-f1", role)

    def test_non_qa_reviewers_are_refused_before_pour(self):
        for reviewer in ("team-lead", "rust-developer", "dev-sanity", "plan-scope-reviewer"):
            groups, result = self.targets(difficulty="hard", reviewer=reviewer)
            self.assertIsInstance(result[0], groups.TargetError)
            self.assertEqual(result[0].code, "BEAD_GROUPS_REVIEWER_INVALID")
            self.assertIn("flaky-test-qa", str(result[0]))
            self.assertIn("--reviewer <agent>", str(result[0]))

    def test_every_supported_qa_reviewer_can_file_a_finding(self):
        for reviewer in qa_capable_reviewers(SCRIPTS.parent / "templates"):
            with self.subTest(reviewer=reviewer):
                groups, result = self.targets(difficulty="hard", reviewer=reviewer)
                self.assertNotIsInstance(result[0], groups.TargetError)
                self.assertEqual(result[0]["compose"]["reviewer"], reviewer)

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


def load_groups(name: str):
    loader = importlib.machinery.SourceFileLoader(name, str(SCRIPTS / "bead-groups"))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    assert spec is not None
    groups = importlib.util.module_from_spec(spec)
    loader.exec_module(groups)
    return groups


class SchemaIssuesTests(unittest.TestCase):
    def test_closed_beads_are_not_schema_checked(self):
        """#578: a closed history bead that predates the models does not refuse the pour; open beads still are checked."""
        groups = load_groups("bead_groups_schema_issues")
        schema = Mock()
        closed = {"id": "t-d-1.group-qa", "status": "closed", "metadata": {}}
        live = {"id": "t-d-2.group-qa", "status": "open", "metadata": {}}
        prospective = {"id": "t-d-3.group-qa", "metadata": {}}
        schema.load_phase.return_value = [closed, live]
        schema.validate_phase.return_value = ["t-d-2.group-qa: reviewers_round1: Field required"]
        issues = groups.schema_issues(schema, "d", [prospective])
        schema.load_phase.assert_called_once_with("d")
        schema.validate_phase.assert_called_once_with([live, prospective])
        self.assertEqual(issues, ["t-d-2.group-qa: reviewers_round1: Field required"])

    def test_closed_bead_with_historical_metadata_is_not_read_as_a_bead_row(self):
        """#639: a closed QA bead whose metadata has a `null` branch and an integer stack (the shape earlier
        pours left) is skipped before the BeadRow check instead of refusing the pour; a malformed open bead
        still refuses it."""
        groups = load_groups("bead_groups_schema_issues")
        schema = Mock()
        closed = {"id": "t-d-1.group-qa", "status": "closed", "metadata": {"branch": None, "stack": 3, "layer": None}}
        live = {"id": "t-d-2.group-qa", "status": "open", "metadata": {}}
        schema.load_phase.return_value = [closed, live]
        schema.validate_phase.return_value = []
        self.assertEqual(groups.schema_issues(schema, "d", []), [])
        schema.validate_phase.assert_called_once_with([live])
        schema.load_phase.return_value = [closed, {"id": "t-d-2.group-qa", "status": "open", "metadata": {"branch": None}}]
        schema.validate_phase.return_value = ["t-d-2.group-qa: branch: invalid field"]
        issues = groups.schema_issues(schema, "d", [])
        self.assertTrue(any("t-d-2.group-qa: metadata.branch" in issue for issue in issues), issues)


class ClosedQaRecoveryTests(unittest.TestCase):
    def test_failed_round_can_pour_next_but_round_cap_refuses(self):
        from test_bead_pour_plan import PlanFixture, groups as pour_groups

        with tempfile.TemporaryDirectory() as directory:
            fixture = PlanFixture(Path(directory))
            sprint = fixture.sprint()
            qa = f"{sprint}.group-qa"
            finding = {"ref": "qa1-f1", "severity": "blocking", "reviewer": "req-qa", "title": "issue", "remedy": "fix"}
            fixture.rows["t-d-1.qa1-f1-r1-fix"] = {"id": "t-d-1.qa1-f1-r1-fix", "status": "closed", "labels": ["stage:fix"],
                                                     "metadata": {"finding_ref": "qa1-f1", "sprint_bead": sprint}}
            fixture.rows[qa].update(status="closed", close_reason="FAIL at abc1234",
                                    metadata={"round": 1, "checked_bead": "t-d-1.qa1-f1-r1-fix"})
            target = fixture.findings(sprint, qa, finding, round_=2)[0]
            self.assertFalse(target["qa_closed"])
            self.assertEqual(target["compose"]["round"], 2)
            other = fixture.findings(sprint, qa, {**finding, "ref": "unrelated"}, round_=2)[0]
            self.assertEqual(other.code, "BEAD_GROUPS_QA_CLOSED")
            fixture.rows[qa]["metadata"]["round"] = 2
            with self.assertRaisesRegex(pour_groups.TargetError, "QA round cap reached") as caught:
                fixture.findings(sprint, qa, finding, round_=3)
            self.assertEqual(caught.exception.code, "BEAD_GROUPS_ROUND_CAP")


class SprintGroupReviewersTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("sc-compose"), "sc-compose is required to render bead formulas")
    def test_sprint_group_qa_step_carries_configured_reviewers(self):
        """#573: the poured sprint QA bead carries reviewers_round1 from the repository configuration."""
        groups = load_groups("bead_groups_sprint_reviewers")
        reviewers = ["req-qa", "arch-qa", "ruthless-boundary-qa"]
        runner = object.__new__(groups.Run)
        runner.option = None
        runner.config = {"worktree_base": "/worktrees", "lint_command": "just lint", "policy_path": "policy.md",
                         "max_sanity_rounds": 5, "reviewers_round1": reviewers}
        container = {"status": "open", "title": "d-1: the work", "priority": 2,
                     "metadata": {"phase": "d", "sprint": "d-1", "stack": "stack", "layer": 1, "difficulty": "normal",
                                  "branch": "sprint/d-1-work", "worktree": "/worktrees/sprint/d-1-work", "pr_target": "integrate/phase-d"}}
        root = {"id": "t-phase-d", "title": "phase d"}
        runner.show = lambda bead: root if bead == "t-phase-d" else container
        runner.plan = lambda _: {"d-1": ("t-d-1", "t-d-1.group-sanity", [])}
        runner.sidecar = lambda _: {"default_option": "C", "options": {"C": {}}, "ref": "group"}
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            runner.repo = work
            shutil.copyfile(SCRIPTS.parent / "formulas/sprint-group.formula.toml.j2", work / "formula.toml.j2")
            with patch.object(groups.sprint_index_common, "load_phase_config", return_value={"root": "t-phase-d"}):
                target = runner.sprint_target("t-d-1")
            self.assertEqual(target["compose"]["reviewers_round1"], reviewers)
            request = {"schema": "sc-compose/beads/v1", "operation": "render", "working_directory": directory,
                       "template": "formula.toml.j2", "rendered_formula": "rendered.formula.toml",
                       "bead_variables": {}, "compose_variables": target["compose"]}
            (work / "request.json").write_text(json.dumps(request))
            output = subprocess.run(["sc-compose", "bead", "render", "--request", "request.json", "--json"],
                                    cwd=work, capture_output=True, text=True, timeout=30)
            self.assertEqual(output.returncode, 0, output.stdout + output.stderr)
            steps = tomllib.loads((work / "rendered.formula.toml").read_text())["steps"]
            qa = [step["metadata"] for step in steps if step["metadata"].get("role") == "qa"]
            self.assertEqual([m["reviewers_round1"] for m in qa], [reviewers])
            self.assertTrue(all("reviewers_round1" not in step["metadata"] for step in steps if step["metadata"].get("role") != "qa"))


def test_compose_timeout_is_typed_and_actionable_for_preview_and_attach(tmp_path, monkeypatch):
    groups = load_groups("bead_groups_timeout")
    runner = object.__new__(groups.Run)
    runner.repo = tmp_path
    runner.relations = lambda target, preview: ([], [])
    template = tmp_path / "formula.toml.j2"
    template.write_text("test")
    monkeypatch.setattr(groups.bead_formulas, "template", lambda *_: template)
    target = {"id": "phase.group", "formula": "sprint-group", "compose": {"parent": "phase", "ref": "group"}}
    for preview, operation in ((True, "preview-attach"), (False, "attach")):
        with patch.object(groups.subprocess, "run", side_effect=subprocess.TimeoutExpired("sc-compose", groups.SC_COMPOSE_TIMEOUT_SECONDS)) as run:
            try:
                runner.pour(target, preview=preview)
                assert False, "expected timeout"
            except groups.TargetError as error:
                request = tmp_path / ".atm-bd/pour/phase.group.group.sprint-group.request.json"
                assert error.code == "BEAD_GROUPS_POUR_TIMEOUT"
                assert all(part in str(error) for part in (f"sc-compose bead {operation}", str(request),
                           str(groups.SC_COMPOSE_TIMEOUT_SECONDS), "inspect", "before retrying"))
                assert run.call_args.kwargs["timeout"] == groups.SC_COMPOSE_TIMEOUT_SECONDS


def test_conflict_receipt_is_reported_and_retained(tmp_path, monkeypatch):
    loader = importlib.machinery.SourceFileLoader("bead_groups_conflict", str(SCRIPTS / "bead-groups"))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    assert spec is not None
    groups = importlib.util.module_from_spec(spec)
    loader.exec_module(groups)
    runner = object.__new__(groups.Run)
    runner.repo = tmp_path
    runner.relations = lambda target, preview: ([], [])
    template = tmp_path / "formula.toml.j2"
    template.write_text("test")
    monkeypatch.setattr(groups.bead_formulas, "template", lambda *_: template)
    receipt = {"payload": {"outcome": {"refused": {"code": "BEADS_GRAPH_CONFLICT"}}, "stages": [
        {"outcome": {"failed": {"code": "BEADS_GRAPH_CONFLICT"}},
         "stderr_excerpt": "graph bead `phase.group-dev` conflicts: provenance_differs; inspect it or use a new ref (existing beads are never edited)"}]}, "diagnostics": []}
    monkeypatch.setattr(groups.subprocess, "run", lambda *a, **k: subprocess.CompletedProcess(a, 2, json.dumps(receipt), ""))
    target = {"id": "phase.group", "formula": "sprint-group", "compose": {"parent": "phase", "ref": "group"}}
    try:
        runner.pour(target, preview=True)
        assert False, "expected conflict"
    except groups.TargetError as error:
        assert error.code == "BEADS_GRAPH_CONFLICT"
        assert all(part in str(error) for part in ("phase.group-dev", "metadata.sc_compose_graph", "use a new ref"))
    assert json.loads(next((tmp_path / ".atm-bd/pour").glob("*.receipt.json")).read_text()) == receipt


@unittest.skipUnless(shutil.which("sc-compose") and os.environ.get("ATM_TEST_BD"),
                     "sc-compose and the CI-pinned embedded bd are required")
class EmbeddedFindingRecoveryTests(unittest.TestCase):
    def test_closed_fail_round_cap_and_unrelated_bad_qa(self):
        from test_templates import _embedded_git_env, _example, _render

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            bin_dir = root / "bin"
            bin_dir.mkdir()
            bd = bin_dir / ("bd.exe" if os.name == "nt" else "bd")
            shutil.copy2(os.environ["ATM_TEST_BD"], bd)
            env = _embedded_git_env(root)
            env["PATH"] = str(bin_dir) + os.pathsep + os.environ.get("PATH", "")

            def run(*argv, expected=0):
                try:
                    proc = subprocess.run(list(argv), cwd=root, env=env, stdin=subprocess.DEVNULL,
                                          capture_output=True, text=True, timeout=120, check=False)
                except subprocess.TimeoutExpired as error:
                    self.fail(f"SUBPROCESS.TIMEOUT: {argv!r} exceeded {error.timeout}s")
                self.assertEqual(proc.returncode, expected, proc.stdout + proc.stderr)
                return proc

            run("git", "init", "-q")
            run(str(bd), "init", "--prefix", "t", "--non-interactive", "--quiet")
            cfg = root / ".claude/project/atm-bd-orchestration.yaml"
            cfg.parent.mkdir(parents=True)
            cfg.write_text('worktree_base: /tmp/worktrees\nlint_command: just lint\npolicy_path: policy.md\n'
                           'max_sanity_rounds: 5\nreviewers_round1: [req-qa]\n')
            phase_dir = root / ".atm-bd"
            phase_dir.mkdir(exist_ok=True)
            template_path = ".claude/skills/atm-bd-orchestration/templates/fixture.j2"
            template = root / template_path
            template.parent.mkdir(parents=True)
            template.write_text("fixture")
            (phase_dir / "phase-d.toml").write_text(
                'plan = "plan.jsonl"\nroot = "t-phase-d"\nintegration_branch = "integrate/phase-d"\nlead = "lead"\n[templates]\n'
                + "".join(f'{key} = "{template_path}"\n' for key in ("dev", "fix", "sanity", "qa", "plan_review", "review")))
            formula_dir = phase_dir / "formula"
            formula_dir.mkdir()
            for suffix in ("formula.toml.j2", "relations.json"):
                shutil.copy2(SCRIPTS.parent / "formulas" / f"finding-group.{suffix}", formula_dir / f"finding-group.{suffix}")
            values = {**_example("qa-bead-vars.json"), "id": "t-d-1.manual-qa", "parent": "t-d-1",
                      "checked_bead": "t-d-1", "sprint_bead": "t-d-1", "layer": "", "commit": ""}
            rendered = _render("qa-bead.json.j2", values)
            self.assertEqual(rendered.returncode, 0, rendered.stderr)
            qa = json.loads(rendered.stdout)
            self.assertNotIn("layer", qa["metadata"])
            self.assertNotIn("commit", qa["metadata"])
            rows = [
                {"id": "t-phase-d", "title": "phase-d: feature", "issue_type": "epic", "labels": ["phase-d"],
                 "metadata": {"integration_branch": "integrate/phase-d"}},
                {"id": "t-phase-d.stack", "title": "stack", "issue_type": "task", "labels": ["phase-d", "stage:stack"],
                 "metadata": {"trunk": "integrate/phase-d", "stack_number": None, "layers": []}},
                {"id": "t-d-1", "title": "d-1: sprint", "issue_type": "task", "labels": ["phase-d", "stage:sprint"],
                 "description": "## Deliverables\n1. Work", "acceptance_criteria": "done",
                 "metadata": {"phase": "d", "sprint": "d-1", "stack": "phase-d", "requirements": ["NONE"],
                              "adrs": ["NONE"], "worktree": "/tmp/worktree", "branch": "sprint/d-1",
                              "pr_target": "integrate/phase-d", "difficulty": "normal"}},
                {"id": "t-d-1.f1-r1-fix", "title": "fix", "status": "closed", "issue_type": "task",
                 "labels": ["phase-d", "stage:fix"], "metadata": {"finding_ref": "qa1-f1", "sprint_bead": "t-d-1"}},
                {"id": "t-d-1.f1-r1-qa", "title": "QA", "status": "closed", "close_reason": "FAIL at abc1234",
                 "issue_type": "task", "labels": ["phase-d", "stage:qa"],
                 "metadata": {"round": 1, "checked_bead": "t-d-1.f1-r1-fix"}}, qa,
            ]
            other_sprint = json.loads(json.dumps(rows[2]))
            other_sprint["id"] = "t-d-2"
            other_sprint["metadata"]["sprint"] = "d-2"
            rows.append(other_sprint)
            (root / "initial.jsonl").write_text("".join(json.dumps(row) + "\n" for row in rows))
            run(str(bd), "import", "initial.jsonl")
            findings = {"sprint": "t-d-1", "round": 2, "filed_by": "t-d-1.f1-r1-qa", "found_at_commit": "abc1234",
                        "findings": [{"ref": "qa1-f1", "severity": "blocking", "reviewer": "req-qa", "title": "issue", "remedy": "fix"}]}
            (root / "findings.json").write_text(json.dumps(findings))
            proc = run(sys.executable, str(SCRIPTS / "bead-groups"), "--findings", "findings.json", "--json")
            report = json.loads(proc.stdout)
            self.assertEqual(report["outcome"], "succeeded", report)
            fix_id = report["targets"][0]["ids"]["fix"]
            run(str(bd), "show", fix_id, "--json")
            bad_qa = json.loads(json.dumps(qa))
            bad_qa["id"] = "t-d-2.bad-qa"
            bad_qa["dependencies"] = []
            bad_qa["metadata"]["reviewers_round1"] = []
            bad_qa["metadata"]["layer"] = None
            bad_qa["metadata"].update(sprint="d-2", sprint_bead="t-d-2", checked_bead="t-d-2")
            (root / "bad.jsonl").write_text(json.dumps(bad_qa) + "\n")
            run(str(bd), "import", "bad.jsonl")
            other = {**findings, "round": 1, "filed_by": "t-d-1.manual-qa",
                     "findings": [{**findings["findings"][0], "ref": "qa-other"}]}
            (root / "other.json").write_text(json.dumps(other))
            partial = run(sys.executable, str(SCRIPTS / "bead-groups"), "--findings", "other.json", "--json", expected=5)
            partial_report = json.loads(partial.stdout)
            self.assertIn("t-d-2.bad-qa", partial.stdout)
            self.assertIn("reviewers_round1", partial.stdout)
            self.assertIn("metadata.layer", partial.stdout)
            healthy = next(item for item in partial_report["targets"] if item["kind"] == "finding")
            run(str(bd), "show", healthy["ids"]["fix"], "--json")
            run(str(bd), "update", "t-d-1.f1-r1-qa", "--set-metadata", "round=2")
            findings["round"] = 3
            (root / "findings.json").write_text(json.dumps(findings))
            capped = run(sys.executable, str(SCRIPTS / "bead-groups"), "--findings", "findings.json", "--json", expected=2)
            self.assertIn("BEAD_GROUPS_ROUND_CAP", capped.stdout)
