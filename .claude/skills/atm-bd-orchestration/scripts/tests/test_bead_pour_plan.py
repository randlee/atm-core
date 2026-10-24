"""Fixed-input tests of bead-groups' requests, plans and receipt handling.

The external attach engine owns graph mutation and idempotency. These tests
check its caller's contract, not a second implementation of that engine.
No database, executable, port or process is needed.
"""
from __future__ import annotations

from contextlib import redirect_stdout
from copy import deepcopy
import importlib.machinery
import importlib.util
import io
import json
from pathlib import Path
from types import SimpleNamespace
import tempfile
import tomllib
import unittest
from unittest.mock import Mock, patch

SCRIPTS = Path(__file__).resolve().parents[1]
loader = importlib.machinery.SourceFileLoader("bead_groups_plan", str(SCRIPTS / "bead-groups"))
spec = importlib.util.spec_from_loader(loader.name, loader)
assert spec is not None
groups = importlib.util.module_from_spec(spec)
loader.exec_module(groups)
import sc_render


class PlanFixture:
    def __init__(self, root):
        self.root = root
        self.runner = object.__new__(groups.Run)
        self.runner.repo = root
        self.runner.option = None
        self.runner.config = {"worktree_base": str(root / "worktrees"), "lint_command": "just lint",
                              "policy_path": "policy.md", "max_sanity_rounds": 5}
        self.runner.plans = {}
        self.runner.sidecars = {}
        self.rows = {}
        self.runner.bd = Mock()
        self.runner.bd.show.side_effect = lambda bead: deepcopy(self.rows.get(bead))

    def sprint(self, phase="x", number=1, predecessors=()):
        name, bead = f"{phase}-{number}", f"t-{phase}-{number}"
        self.rows[bead] = {
            "id": bead, "status": "open", "priority": 2, "title": f"{name}: the work",
            "metadata": {"phase": phase, "sprint": name, "stack": f"phase-{phase}",
                         "layer": number, "difficulty": "normal", "branch": f"sprint/{name}",
                         "worktree": str(self.root / "worktrees" / name),
                         "pr_target": f"integrate/phase-{phase}", "requirements": ["REQ-1"], "adrs": ["NONE"]},
        }
        self.runner.__dict__.setdefault("features", {})[phase] = {"stack_bead": f"t-phase-{phase}.stack",
                                                                  "phase_feature": f"phase-{phase}: feature"}
        self.runner.plans.setdefault(phase, {})[name] = (bead, f"{bead}.group-sanity", list(predecessors))
        self.rows[f"{bead}.group-qa"] = {"id": f"{bead}.group-qa", "status": "open"}
        self.rows[f"t-phase-{phase}.stack"] = {
            "id": f"t-phase-{phase}.stack", "labels": [f"phase-{phase}", "stage:stack"],
            "metadata": {"trunk": f"integrate/phase-{phase}", "stack_number": None, "layers": []},
        }
        return bead

    def findings(self, sprint, qa, *rows, round_=1):
        path = self.root / "findings.json"
        path.write_text(json.dumps({"sprint": sprint, "round": round_, "filed_by": qa,
                                    "found_at_commit": "abc1234", "findings": list(rows)}))
        with patch.object(groups.sprint_index_common, "load_phase_config",
                          return_value={"root": f"t-phase-{self.rows[sprint]['metadata']['phase']}"}):
            return self.runner.finding_targets(path)

    def formula(self, target):
        """Render formula data in-process, normalizing the attach engine's pass delimiters."""
        source = groups.bead_formulas.template(target["formula"], self.root).read_text()
        template = self.root / "formula-pass.toml.j2"
        template.write_text(source.replace("{{{", "{{").replace("}}}", "}}"))
        return tomllib.loads(sc_render.render(template, target["compose"]))

    def receipt(self, graph=None, error=None):
        payload = {"outcome": "succeeded", "graph": graph or {
            "ids": {step: f"t-x-1.group-{step}" for step in ("dev", "sanity", "qa")}, "nodes": [], "edges": []}}
        if error:
            payload = {"outcome": "failed", "error": error}
        return SimpleNamespace(returncode=2 if error else 0, stdout=json.dumps({"payload": payload}), stderr="")

    def invoke(self, target, receipt, *, preview=False):
        template = self.root / "formula.toml.j2"
        template.write_text("fixed template input")
        with (patch.object(groups.bead_formulas, "template", return_value=template),
              patch.object(groups.subprocess, "run", return_value=receipt) as command):
            attachment = self.runner.pour(target, preview=preview)
        argv = command.call_args.args[0]
        request = json.loads(Path(argv[argv.index("--request") + 1]).read_text())
        return attachment, request, command


def assert_close_fields(case, steps, parent, ref, work):
    """Every poured step carries close_fields.POURED for its role, naming its group's own beads."""
    import close_fields
    for step in steps:
        metadata = step["metadata"]
        case.assertEqual([k for k in close_fields.POURED if metadata.get(k) in (None, "")], [], step["id"])
        case.assertEqual(metadata["role"], {"sanity": "sanity"}.get(step["id"], step["id"]))
        case.assertEqual((metadata["checked_bead"], metadata["sanity_bead"], metadata["qa_bead"], metadata["base"]),
                         (f"{parent}.{ref}-{work}", f"{parent}.{ref}-sanity", f"{parent}.{ref}-qa", metadata["pr_target"]))
        case.assertEqual((metadata["stack_bead"], metadata["lint_command"], metadata["max_sanity_rounds"]),
                         ("t-phase-x.stack", "just lint", 5))


class PourPlanTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.fixture = PlanFixture(Path(temporary.name))
        self.runner = self.fixture.runner
        self.sprint = self.fixture.sprint()
        self.no_process = patch("subprocess.Popen", side_effect=AssertionError("unit test spawned a process"))
        self.no_process.start()
        self.addCleanup(self.no_process.stop)
        self.no_socket = patch("socket.socket", side_effect=AssertionError("unit test opened a socket"))
        self.no_socket.start()
        self.addCleanup(self.no_socket.stop)

    def test_sprint_formula_and_relations_for_every_option(self):
        dependent = self.fixture.sprint(number=2, predecessors=("x-1",))
        for option in "ABC":
            with self.subTest(option=option):
                self.runner.option = option
                target = self.runner.sprint_target(dependent)
                formula = self.fixture.formula(target)
                steps = formula["steps"]
                self.assertEqual([step["id"] for step in steps], ["dev"] if option == "A" else ["dev", "sanity", "qa"])
                assert_close_fields(self, steps, dependent, "group", "dev")
                self.assertEqual(steps[0]["metadata"]["checked_title"], "x-2: the work")
                for step in steps:
                    self.assertNotIn("assignee", step)
                    for key in ("branch", "worktree", "pr_target", "difficulty"):
                        self.assertEqual(step["metadata"][key], self.fixture.rows[dependent]["metadata"][key])
                relations, missing = self.runner.relations(target, preview=False)
                self.assertEqual(missing, [])
                predecessor = f"{self.sprint}.group-{'dev' if option == 'A' else 'sanity'}"
                self.assertIn({"from": "step:dev", "to": f"bead:{predecessor}", "type": "blocks"}, relations)
                if option != "A":
                    self.assertEqual(steps[1].get("needs", []), ["dev"] if option == "C" else [])
                    self.assertEqual(steps[2]["needs"], ["sanity"])
                    self.assertIn({"from": "step:qa", "to": "step:dev", "type": "validates"}, relations)
                    if option == "B":
                        self.assertIn({"from": "step:sanity", "to": "step:dev", "type": "validates"}, relations)

    def test_flat_finding_formula_and_layer_cut(self):
        self.fixture.rows["t-phase-x.stack"]["metadata"]["layers"] = [
            {"branch": "fix/x-previous", "pr": 1, "head": "a" * 40, "base": "b" * 40, "linked": True}]
        finding = {"ref": "qa1-f1", "severity": "blocking", "reviewer": "arch-qa", "title": "issue", "remedy": "fix it"}
        for option in "ABC":
            with self.subTest(option=option):
                self.runner.option = option
                target = self.fixture.findings(self.sprint, f"{self.sprint}.group-qa", finding)[0]
                self.assertEqual(target["compose"]["parent"], self.sprint)
                self.assertEqual(target["compose"]["ref"], "qa1-f1-r1")
                steps = self.fixture.formula(target)["steps"]
                self.assertEqual([step["id"] for step in steps], ["fix"] if option == "A" else ["fix", "sanity", "qa"])
                for step in steps:
                    self.assertEqual(step["metadata"]["pr_target"], "fix/x-previous")
                    self.assertEqual(step["metadata"]["branch"], "fix/x-1-qa1-f1")
                self.assertEqual(steps[0]["metadata"]["requirements"], ["REQ-1"])
                assert_close_fields(self, steps, self.sprint, "qa1-f1-r1", "fix")
                self.assertEqual(steps[0]["metadata"]["checked_title"], steps[0]["title"])  # the fix bead's own title
                relations, _ = self.runner.relations(target, preview=False)
                self.assertIn({"from": "step:fix", "to": f"bead:{self.sprint}.group-qa", "type": "discovered-from"}, relations)

    def test_override_formula_wins_and_missing_variable_is_not_silently_filled(self):
        target = self.runner.sprint_target(self.sprint)
        source = groups.bead_formulas.template("sprint-group", self.fixture.root).read_text()
        override = self.fixture.root / ".atm-bd/formula/sprint-group.formula.toml.j2"
        override.parent.mkdir(parents=True)
        override.write_text(source.replace("Dev work for sprint", "OVERRIDE work for sprint"))
        self.assertTrue(self.fixture.formula(target)["steps"][0]["description"].startswith("OVERRIDE"))
        override.write_text(source.replace("Dev work for sprint", "{{{ extra }}} work for sprint"))
        with self.assertRaisesRegex(sc_render.RenderError, "extra"):
            self.fixture.formula(target)

    def test_attach_request_preserves_created_existing_and_interrupted_receipts(self):
        target = self.runner.sprint_target(self.sprint)
        original = deepcopy(self.fixture.rows)
        ids = {step: f"{self.sprint}.group-{step}" for step in ("dev", "sanity", "qa")}
        for actions in (("created",) * 3, ("existing",) * 3, ("existing", "existing", "created")):
            with self.subTest(actions=actions):
                graph = {"ids": ids, "nodes": [{"id": ids[step], "action": action} for step, action in zip(ids, actions)],
                         "edges": [{"from": ids["qa"], "to": ids["dev"], "type": "validates", "action": "existing"}]}
                before = deepcopy(graph)
                attachment, request, command = self.fixture.invoke(target, self.fixture.receipt(graph))
                self.assertEqual(attachment["nodes"], before["nodes"])
                self.assertEqual(attachment["edges"], before["edges"])
                self.assertEqual(request["parent"], self.sprint)
                self.assertEqual(request["ref"], "group")
                self.assertEqual(request["operation"], "attach")
                self.assertEqual(request["pour_authorization"], "CreatePersistentBeads")
                self.assertEqual(request["compose_variables"], target["compose"])
                self.assertEqual(request["bead_variables"], {})
                self.assertEqual(command.call_count, 1)
                self.runner.bd.run.assert_not_called()
                self.assertEqual(self.fixture.rows, original)

    def test_main_checks_json_plan_before_applying_and_keeps_only_missing_create(self):
        import bead_schema
        target = self.runner.sprint_target(self.sprint)
        ids = {step: f"{self.sprint}.group-{step}" for step in ("dev", "sanity", "qa")}
        plan = self.fixture.root / "interrupted-plan.json"
        missing = {"id": ids["qa"], "metadata": {"checked_bead": ids["dev"]}}
        plan.write_text(json.dumps({"nodes": [missing]}))
        preview = {"ids": ids, "plan_path": str(plan), "edges": [], "nodes": [
            {"id": ids["dev"], "action": "existing"}, {"id": ids["sanity"], "action": "existing"},
            {"id": ids["qa"], "action": "create"}]}
        applied = deepcopy(preview)
        applied["nodes"][-1]["action"] = "created"
        template = self.fixture.root / "template.toml.j2"
        template.write_text("fixed input")
        events = []

        def respond(argv, **kwargs):
            operation = json.loads(Path(argv[argv.index("--request") + 1]).read_text())["operation"]
            events.append(operation)
            return self.fixture.receipt(preview if operation == "preview_attach" else applied)

        def validate(rows):
            events.append("validate")
            self.assertEqual(rows, [missing])
            return []

        with (patch.object(groups, "Run", return_value=self.runner),
              patch.object(groups.repo_config, "repo_root", return_value=self.fixture.root),
              patch.object(groups.bead_formulas, "template", return_value=template),
              patch.object(groups.subprocess, "run", side_effect=respond),
              patch.object(bead_schema, "load_phase", return_value=[]),
              patch.object(bead_schema, "validate_phase", side_effect=validate),
              redirect_stdout(io.StringIO()) as output):
            code = groups.main(["--json", "--sprint", self.sprint])
        self.assertEqual(code, 0)
        self.assertEqual(events, ["preview_attach", "validate", "attach"])
        self.assertEqual(json.loads(output.getvalue())["targets"][0]["nodes"], applied["nodes"])
        self.assertEqual(json.loads(plan.read_text())["nodes"], [missing])
        self.assertEqual(target["compose"]["parent"], self.sprint)
        self.runner.bd.run.assert_not_called()

    def test_unsupplied_variable_receipt_is_refused_with_template_and_variable(self):
        target = self.runner.sprint_target(self.sprint)
        error = {"code": "BEADS_RENDER_FAILED", "message": "sprint-group.formula.toml.j2: undefined variable: extra"}
        with self.assertRaises(groups.TargetError) as caught:
            self.fixture.invoke(target, self.fixture.receipt(error=error))
        self.assertEqual(caught.exception.code, error["code"])
        self.assertIn(error["message"], str(caught.exception))
        self.runner.bd.run.assert_not_called()

    def test_real_schema_rejects_stripped_rendered_sanity_before_any_attach(self):
        import bead_schema
        target = self.runner.sprint_target(self.sprint)
        ids = {step: f"{self.sprint}.group-{step}" for step in ("dev", "sanity", "qa")}
        template = SCRIPTS.parents[1] / "atm-beads/templates/dev-sanity-bead.json.j2"
        values = {**target["compose"], "id": ids["sanity"], "dev_bead": ids["dev"], "qa_bead": ids["qa"],
                  "sprint_bead": self.sprint}
        rendered = json.loads(sc_render.render(template, values))
        self.assertEqual(bead_schema.validate_phase([rendered]), [])
        del rendered["metadata"]["branch"]
        self.assertTrue(any(problem.startswith(f"{ids['sanity']}: branch:")
                            for problem in bead_schema.validate_phase([rendered])))
        plan = self.fixture.root / "stripped-sanity-plan.json"
        plan.write_text(json.dumps({"nodes": [rendered]}))
        graph = {"ids": ids, "plan_path": str(plan), "edges": [],
                 "nodes": [{"id": ids["sanity"], "action": "create"}]}
        formula = self.fixture.root / "template.toml.j2"
        formula.write_text("fixed transport input")
        for mode in ([], ["--validate"]):
            with self.subTest(mode=mode):
                requests = []

                def respond(argv, **kwargs):
                    request = json.loads(Path(argv[argv.index("--request") + 1]).read_text())
                    requests.append(request)
                    return self.fixture.receipt(graph)

                with (patch.object(groups, "Run", return_value=self.runner),
                      patch.object(groups.repo_config, "repo_root", return_value=self.fixture.root),
                      patch.object(groups.bead_formulas, "template", return_value=formula),
                      patch.object(groups.subprocess, "run", side_effect=respond),
                      patch.object(bead_schema, "load_phase", return_value=[]),
                      redirect_stdout(io.StringIO()) as output):
                    code = groups.main(["--json", "--sprint", self.sprint, *mode])
                report = json.loads(output.getvalue())
                self.assertEqual(code, groups.PROBLEMS)
                self.assertTrue(report["schema_problems"])
                self.assertTrue(any(problem.startswith(f"{ids['sanity']}: branch:")
                                    for problem in report["targets"][0]["problems"]))
                self.assertEqual([request["operation"] for request in requests], ["preview_attach"])
                self.assertTrue(all("pour_authorization" not in request for request in requests))
                self.runner.bd.run.assert_not_called()

    def test_schema_read_failure_returns_cannot_run_without_attach(self):
        import bead_schema
        target = self.runner.sprint_target(self.sprint)
        graph = {"ids": {}, "nodes": [], "edges": []}
        with (patch.object(groups, "Run", return_value=self.runner),
              patch.object(groups.repo_config, "repo_root", return_value=self.fixture.root),
              patch.object(self.runner, "pour", return_value={"ids": {}, "nodes": [], "edges": [], "graph": graph}) as pour,
              patch.object(bead_schema, "load_phase", side_effect=ValueError("invalid schema response")),
              redirect_stdout(io.StringIO()) as output):
            code = groups.main(["--json", "--sprint", self.sprint])
        self.assertEqual(code, groups.FAILED)
        self.assertEqual(json.loads(output.getvalue())["error"], {
            "code": "BEAD_GROUPS_CANNOT_RUN", "message": "invalid schema response"})
        pour.assert_called_once_with(target, preview=True)
        self.runner.bd.run.assert_not_called()

    def test_preview_has_no_write_authorization_and_persists_exact_request(self):
        _, request, command = self.fixture.invoke(self.runner.sprint_target(self.sprint), self.fixture.receipt(), preview=True)
        self.assertEqual(request["operation"], "preview_attach")
        self.assertNotIn("pour_authorization", request)
        self.assertEqual(command.call_args.args[0][:3], ["sc-compose", "bead", "preview-attach"])

    def test_request_mode_does_not_leak_between_preview_and_apply(self):
        target = self.runner.sprint_target(self.sprint)
        for preview in (False, True, False, True):
            with self.subTest(preview=preview):
                _, request, _ = self.fixture.invoke(target, self.fixture.receipt(), preview=preview)
                self.assertEqual(request["operation"], "preview_attach" if preview else "attach")
                self.assertEqual("pour_authorization" in request, not preview)
                self.assertFalse(hasattr(self.runner, "validate"))

    def test_closed_qa_refusal_is_checked_once_before_preview_with_noop_and_validate_exceptions(self):
        import bead_schema
        qa = f"{self.sprint}.group-qa"
        finding = {"ref": "qa1-f1", "severity": "blocking", "reviewer": "arch-qa", "title": "issue", "remedy": "fix"}
        for option in ("A", "C"):
            for closed in (False, True):
                for presence in ("missing", "partial", "existing"):
                    for validate in (False, True):
                        with self.subTest(option=option, closed=closed, presence=presence, validate=validate):
                            self.runner.option = option
                            self.fixture.rows[qa]["status"] = "closed" if closed else "open"
                            for step in ("fix", "sanity", "qa"):
                                self.fixture.rows.pop(f"{self.sprint}.qa1-f1-r1-{step}", None)
                            target = self.fixture.findings(self.sprint, qa, finding)[0]
                            steps = self.runner.steps(target)
                            present = [] if presence == "missing" else steps[:1] if presence == "partial" else steps
                            for step in present:
                                bead = groups.attach_id(self.sprint, target["compose"]["ref"], step)
                                self.fixture.rows[bead] = {"id": bead}
                            modes = []

                            def pour(_target, *, preview):
                                modes.append(preview)
                                return {"ids": {}, "nodes": [], "edges": [], "missing": [],
                                        "graph": {"nodes": [], "edges": []}}

                            with (patch.object(groups, "Run", return_value=self.runner),
                                  patch.object(groups.repo_config, "repo_root", return_value=self.fixture.root),
                                  patch.object(self.runner, "finding_targets", return_value=[target]),
                                  patch.object(self.runner, "pour", side_effect=pour),
                                  patch.object(bead_schema, "load_phase", return_value=[]),
                                  redirect_stdout(io.StringIO()) as output):
                                code = groups.main(["--json", "--findings", "unused.json", *(["--validate"] if validate else [])])
                            refused = closed and not validate and len(present) < len(steps)
                            self.assertEqual(code, groups.FAILED if refused else groups.SUCCESS)
                            self.assertEqual(modes, [] if refused else [True] if validate else [True, False])
                            if refused:
                                error = json.loads(output.getvalue())["targets"][0]["error"]
                                self.assertEqual(error["code"], "BEAD_GROUPS_QA_CLOSED")
                            self.assertFalse(hasattr(self.runner, "validate"))
                            self.runner.bd.run.assert_not_called()

    def test_missing_relation_handling_uses_the_explicit_preview_mode(self):
        dependent = self.fixture.sprint(number=2, predecessors=("x-1",))
        target = self.runner.sprint_target(dependent)
        preview_relations, missing = self.runner.relations(target, preview=True)
        applied_relations, applied_missing = self.runner.relations(target, preview=False)
        relation = {"from": "step:dev", "to": f"bead:{self.sprint}.group-sanity", "type": "blocks"}
        self.assertNotIn(relation, preview_relations)
        self.assertIn(relation, applied_relations)
        self.assertEqual(len(missing), 1)
        self.assertIn(f"missing bead {self.sprint}.group-sanity", missing[0])
        self.assertEqual(applied_missing, [])

    def test_prospective_json_plan_receives_its_dependencies(self):
        plan = self.fixture.root / "plan.json"
        nodes = [{"id": f"{self.sprint}.group-dev", "metadata": {"branch": "sprint/x-1"}}]
        plan.write_text(json.dumps({"nodes": nodes}))
        graph = {"plan_path": "plan.json", "ids": {"dev": nodes[0]["id"]},
                 "edges": [{"from": "step:dev", "to": f"bead:{self.sprint}", "type": "parent-child"}]}
        prospective = self.runner.prospective({"graph": graph})
        self.assertEqual(prospective, [{**nodes[0], "dependencies": [{"type": "parent-child", "depends_on_id": self.sprint}]}])
        self.assertEqual(json.loads(plan.read_text()), {"nodes": nodes})

    def test_conflict_preview_prevents_attach_for_every_target_and_keeps_repair(self):
        import bead_schema
        second = self.fixture.sprint(number=2)
        repair = f"bd dep add '{self.sprint}.group-qa' '{self.sprint}.group-dev' --type 'validates'"
        error = {"code": "BEADS_GRAPH_EDGE_MISSING", "message": repair}
        template = self.fixture.root / "template.toml.j2"
        template.write_text("fixed input")
        commands = []

        def respond(argv, **kwargs):
            request = json.loads(Path(argv[argv.index("--request") + 1]).read_text())
            commands.append(request)
            return self.fixture.receipt(error=error) if request["parent"] == self.sprint else self.fixture.receipt()

        with (patch.object(groups, "Run", return_value=self.runner),
              patch.object(groups.repo_config, "repo_root", return_value=self.fixture.root),
              patch.object(groups.bead_formulas, "template", return_value=template),
              patch.object(groups.subprocess, "run", side_effect=respond),
              patch.object(bead_schema, "load_phase", return_value=[]),
              patch.object(bead_schema, "validate_phase", return_value=[]),
              patch.object(self.runner, "prospective", return_value=[]),
              redirect_stdout(io.StringIO()) as output):
            code = groups.main(["--json", "--sprint", self.sprint, "--sprint", second])
        report = json.loads(output.getvalue())
        self.assertEqual(code, 2)
        self.assertEqual(report["targets"][0]["error"]["code"], error["code"])
        self.assertIn(repair, report["targets"][0]["error"]["message"])
        self.assertEqual(len(commands), 2)
        self.assertTrue(all(command["operation"] == "preview_attach" for command in commands))
        self.assertTrue(all("pour_authorization" not in command for command in commands))
        self.runner.bd.run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
