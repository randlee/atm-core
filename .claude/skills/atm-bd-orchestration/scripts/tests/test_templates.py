from __future__ import annotations

from pathlib import Path
import json
import re
import subprocess
import unittest


ROOT = Path(__file__).parents[2]


class TemplateContractTests(unittest.TestCase):
    def test_dev_template_requires_pr_target_not_obsolete_top(self):
        text = (ROOT / "templates/dev-template.xml.j2").read_text()
        self.assertIn("- pr_target", text)
        self.assertNotIn("- top", text)
        self.assertNotIn("pushed" + " top", text)

    def test_finding_requires_difficulty_and_priority_map_is_current(self):
        text = (ROOT / "templates/finding-bead.json.j2").read_text()
        self.assertIn("- difficulty", text)
        import json, sys
        sys.path.insert(0, str(ROOT.parents[1] / "atm-beads" / "scripts"))
        from plan_contract import SEVERITY_PRIORITY
        self.assertIn(json.dumps(SEVERITY_PRIORITY).replace(", ", ", "), text)  # the literal mirrors plan_contract; Jinja cannot import it
        self.assertIn('"difficulty"', text)
        self.assertIn("## Deliverables\\n1.", text)

    def test_sanity_assignment_has_pr_and_exact_checks(self):
        text = (ROOT / "templates/dev-sanity-template.xml.j2").read_text()
        self.assertIn("- pr_number", text)
        self.assertIn("- pr_url", text)
        self.assertIn("gh pr view", text)
        self.assertIn("SANITY.ZERO_DELTA", text)

    def test_workflow_issue_template_exists(self):
        self.assertTrue((ROOT / "templates/workflow-issue-bead.json.j2").exists())

    def test_gate_commands_render_root_and_primary_checkout(self):
        for name in ("dev-template", "fix-assignment", "dev-fix", "dev-sanity-template"):
            text = (ROOT / f"templates/{name}.xml.j2").read_text()
            with self.subTest(template=name):
                self.assertNotIn("<phase>", text)
                self.assertNotIn("git worktree list", text)
                self.assertIn("{{ primary_checkout | string | cdata_escape }}/.claude/skills/", text)
                if name != "dev-sanity-template":
                    self.assertIn("--root {{ phase_root | string | cdata_escape }}", text)

    def test_sanity_template_has_no_stale_base_check(self):
        text = (ROOT / "templates/dev-sanity-template.xml.j2").read_text()
        self.assertNotIn("STALE_BASE", text)  # sanity-split pins origin/<base> itself (three-dot diff)
        self.assertIn("git fetch origin && git log --format=%H origin/", text)  # but the tracking ref must be fresh

    def test_finding_bead_deliverables_are_splittable(self):
        import importlib.machinery, importlib.util, json, subprocess
        loader = importlib.machinery.SourceFileLoader("sanity_split", str(ROOT / "scripts/sanity-split"))
        split = importlib.util.module_from_spec(importlib.util.spec_from_loader("sanity_split", loader))
        loader.exec_module(split)
        result = subprocess.run(["sc-compose", "render", "--file", str(ROOT / "templates/finding-bead.json.j2"),
                                 "--var-file", str(ROOT / "examples/finding-bead-vars.json"), "--strict"],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        items = split.parse_deliverables(json.loads(result.stdout)["description"])
        self.assertEqual(len(items), 1)

    def test_dev_step_a_rebases_before_the_gate(self):
        for name in ("dev-template", "fix-assignment", "dev-fix"):
            text = (ROOT / f"templates/{name}.xml.j2").read_text()
            with self.subTest(template=name):
                self.assertIn("`git fetch origin && git rebase origin/{{ pr_target | string | cdata_escape }}` in the worktree", text)
                self.assertLess(text.index("git rebase origin/"), text.index("assignment-gates.py dev"))

    def test_integration_completion_requires_audit_evidence(self):
        import json
        import tempfile

        template = ROOT / "templates/review-complete.md.j2"
        original = json.loads((ROOT / "examples/review-complete-vars.json").read_text())
        passed = {**original, "findings_important": 0, "verdict": "PASS", "integration_review": "integration_review_passed",
                  "post_mortem_counts": {"total": 0, "verified_fixed": 0, "justified_nonfix": 0, "unresolved": 0},
                  "post_mortem_md": "Empty inventory verified; no_systemic_followup"}
        cases = [
            (passed, True),
            ({**passed, "findings_blocking": 1}, False),
            ({**passed, "findings_important": 1}, False),
            ({**passed, "findings_minor": -1}, False),
            (original, True),
            ({**original, "integration_review": "PASS"}, False),
            ({**original, "post_mortem_md": "  "}, False),
            ({k: v for k, v in original.items() if k != "integration_review"}, False),
            ({k: v for k, v in original.items() if k != "post_mortem_md"}, False),
            ({**passed, "post_mortem_counts": original["post_mortem_counts"]}, False),
            ({**passed, "integration_commit": "f" * 40}, False),
            ({**passed, "integration_commit": "short"}, False),
            ({**passed, "verdict": "FAIL"}, False),
            ({**passed, "post_mortem_counts": {"total": 2, "verified_fixed": 1, "justified_nonfix": 0, "unresolved": 0}}, False),
            ({**passed, "post_mortem_counts": {"total": 0, "verified_fixed": 1, "justified_nonfix": -1, "unresolved": 0}}, False),
        ]
        with tempfile.TemporaryDirectory() as directory:
            variables = Path(directory) / "vars.json"
            for values, valid in cases:
                with self.subTest(values=values, valid=valid):
                    variables.write_text(json.dumps(values))
                    import sys
                    checked = subprocess.run([
                        sys.executable, str(ROOT / "scripts/check-review-completion.py"),
                        str(variables)], capture_output=True, text=True)
                    self.assertEqual(checked.returncode == 0, valid, checked.stderr)
                    if not valid:
                        self.assertIn("review completion:", checked.stderr)
                        continue
                    result = subprocess.run([
                        "sc-compose", "render", "--file", str(template),
                        "--var-file", str(variables), "--strict"], capture_output=True, text=True)
                    self.assertEqual(result.returncode == 0, valid, result.stderr)
                    if valid:
                        machine = json.loads(result.stdout.split("```json\n", 1)[1].split("```", 1)[0])
                        self.assertEqual(machine["integration_review"], values["integration_review"])

    def test_review_completion_requires_jev_execution_receipt(self):
        import json
        import tempfile
        values = json.loads((ROOT / "examples/review-complete-vars.json").read_text())
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "vars.json"
            for include in (True, False):
                candidate = dict(values)
                if not include:
                    del candidate["post_mortem_jev"]
                path.write_text(json.dumps(candidate))
                result = subprocess.run([
                    "sc-compose", "render", "--file", str(ROOT / "templates/review-complete.md.j2"),
                    "--var-file", str(path), "--strict"], capture_output=True, text=True)
                self.assertEqual(result.returncode == 0, include, result.stderr)
                if include:
                    machine = json.loads(result.stdout.split("```json\n", 1)[1].split("```", 1)[0])
                    self.assertEqual(machine["post_mortem_jev"], candidate["post_mortem_jev"])

    def test_assignment_examples_render_strictly(self):
        examples = ROOT / "examples"
        templates = sorted((ROOT / "templates").glob("*.j2"))
        self.assertTrue(templates, "templates directory must not be empty")
        for template_path in templates:
            fixture_name = template_path.name.removesuffix(".j2").rsplit(".", 1)[0] + "-vars.json"
            variables = examples / fixture_name
            with self.subTest(template=template_path.name):
                self.assertTrue(variables.is_file(), f"missing strict-render fixture: {fixture_name}")
                result = subprocess.run([
                    "sc-compose", "render", "--file", str(template_path),
                    "--var-file", str(variables), "--strict"], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_sanity_assignment_context_shape_matches_reviewer_inputs(self):
        assignment = ROOT / "templates/dev-sanity-assignment.json.j2"
        examples = ROOT / "examples"
        rendered_values = []
        for fixture, expected_context in (("dev-sanity-assignment-vars.json", []),
                                          ("dev-sanity-assignment-context-vars.json", [{"path": "docs/retry.md", "why": "The deliverable delegates retry policy here."}])):
            with self.subTest(fixture=fixture):
                result = subprocess.run([
                    "sc-compose", "render", "--strict", "--file", str(assignment),
                    "--var-file", str(examples / fixture),
                ], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                rendered = json.loads(result.stdout)
                self.assertEqual(rendered["context"], expected_context)
                rendered_values.append(rendered)

        expected_keys = set(rendered_values[0])
        for reviewer in ("sc-sanity-llm.md", "sc-sanity-jev.md"):
            with self.subTest(reviewer=reviewer):
                text = (ROOT.parents[1] / "agents" / reviewer).read_text()
                matched = re.search(r"## Inputs.*?```json\n(.*?)\n```", text, re.S)
                self.assertIsNotNone(matched)
                input_json = json.loads(matched.group(1))
                self.assertEqual(set(input_json), expected_keys)
                self.assertEqual(input_json["context"], [])
                self.assertIn("Read only that evidence plus any `context` paths at the pinned commit; never request more.", text)


def _render(template: str, values: dict) -> subprocess.CompletedProcess:
    import json
    import tempfile
    with tempfile.TemporaryDirectory() as directory:
        variables = Path(directory) / "vars.json"
        variables.write_text(json.dumps(values))
        return subprocess.run(["sc-compose", "render", "--file", str(ROOT / "templates" / template),
                               "--var-file", str(variables), "--strict"], capture_output=True, text=True)


def _example(name: str) -> dict:
    import json
    return json.loads((ROOT / "examples" / name).read_text())


class FixRoundReviewerScopeTests(unittest.TestCase):
    """A fix is verified only by its filing reviewer, locked to the original finding; sprint rounds keep the full set."""

    RBQA = "ruthless-boundary-qa-assignment.json.j2"
    QA = "qa-template.xml.j2"

    def rbqa(self, **values) -> subprocess.CompletedProcess:
        base = {k: v for k, v in _example("ruthless-boundary-qa-assignment-vars.json").items()
                if k not in ("qa_round", "carry_forward_findings_json")}
        return _render(self.RBQA, {**base, **values})

    def test_round_one_is_unlocked(self):
        import json
        result = self.rbqa(qa_round=1)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIs(json.loads(result.stdout)["findings_scope_locked"], False)
        qa = _render(self.QA, _example("qa-template-vars.json"))
        self.assertEqual(qa.returncode, 0, qa.stderr)
        self.assertIn("This is a sprint review: run the full reviewer set", qa.stdout)
        self.assertIn("`qa_round` = 1", qa.stdout)
        self.assertNotIn("fix-round-scope", qa.stdout)
        self.assertIn("<fix-verification-precedence>", qa.stdout)  # always present, as upstream

    def test_sprint_round_two_is_a_sprint_review(self):
        qa = _render(self.QA, {**_example("qa-template-vars.json"), "round": 2})
        self.assertEqual(qa.returncode, 0, qa.stderr)
        self.assertIn("This is a sprint review: run the full reviewer set", qa.stdout)
        self.assertIn("`qa_round` = 2", qa.stdout)
        self.assertIn("`bd import <scratch>/", qa.stdout)

    def test_fix_verification_dispatches_only_the_filing_reviewer(self):
        import json
        result = self.rbqa(qa_round=2, carry_forward_findings_json='["RBQA-004"]')
        self.assertEqual(result.returncode, 0, result.stderr)
        data = json.loads(result.stdout)
        self.assertIs(data["findings_scope_locked"], True)
        self.assertEqual(data["carry_forward_findings"], ["RBQA-004"])
        qa = _render(self.QA, _example("qa-template-fix-round-vars.json"))
        self.assertEqual(qa.returncode, 0, qa.stderr)
        out = qa.stdout
        self.assertIn("This is fix verification of finding beads", out)
        self.assertIn("scripts/fix-round-scope owned --carried", out)
        self.assertIn("scripts/fix-round-scope check --carried", out)
        self.assertIn("Dispatch exactly those reviewers and no other", out)
        self.assertIn("`qa_round` = 2", out)
        self.assertNotIn("full reviewer set", out)
        for reviewer in ("req-qa", "arch-qa", "rust-qa-agent"):  # no automatic panel
            self.assertNotIn(f"`{reviewer}`", out)
        self.assertNotIn('<step id="f">', out)  # no ceremony screen, as upstream 12be0fad
        self.assertNotIn('<step id="g1">', out)
        step_g = out[out.index('<step id="g">'):out.index('<step id="h">')]
        self.assertIn("Do not screen or file new findings", step_g)
        self.assertNotIn("bd import", step_g)
        self.assertNotIn("bd close <finding>", step_g)  # the fixer closes; verification confirms or reopens
        self.assertIn("PASS requires a PASS from the filing reviewer and every carried finding confirmed fixed and closed.", out)

    def test_fix_round_with_empty_scope_fails(self):
        for scope in (None, "[]", "[ ]", "", "  ", "null"):
            with self.subTest(scope=scope):
                values = {"qa_round": 2} if scope is None else {"qa_round": 2, "carry_forward_findings_json": scope}
                self.assertNotEqual(self.rbqa(**values).returncode, 0)

    def test_missing_round_fails(self):
        result = self.rbqa(carry_forward_findings_json='["RBQA-004"]')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("qa_round", result.stderr)
        values = {k: v for k, v in _example("qa-template-vars.json").items() if k != "round"}
        self.assertNotEqual(_render(self.QA, values).returncode, 0)

    def test_quick_fix_branch_without_a_finding_is_a_sprint_review(self):
        qa = _render(self.QA, {**_example("qa-template-vars.json"), "branch": "fix/d-4-qa1-f1-retry-jitter"})
        self.assertEqual(qa.returncode, 0, qa.stderr)
        self.assertIn("This is a sprint review: run the full reviewer set", qa.stdout)
        self.assertNotIn("fix-round-scope", qa.stdout)

    def test_carried_finding_on_a_sprint_branch_is_fix_verification(self):
        qa = _render(self.QA, {**_example("qa-template-vars.json"), "carry_forward": "x-d-4-qa1-f1"})
        self.assertEqual(qa.returncode, 0, qa.stderr)
        self.assertIn("This is fix verification of finding beads `x-d-4-qa1-f1`", qa.stdout)


class PlanFixRoundTests(unittest.TestCase):
    """A plan fix round runs only the carried findings' filing reviewers; round 1 runs the full plan review."""

    PLAN = "plan-review-template.xml.j2"
    PSR = "plan-scope-reviewer-assignment.json.j2"

    def test_round_one_is_the_full_plan_review(self):
        out = _render(self.PLAN, _example("plan-review-template-vars.json"))
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn("Run `req-qa` and `arch-qa` as background agents", out.stdout)
        self.assertIn("Run `plan-scope-reviewer` over the whole set", out.stdout)
        self.assertIn("Run `ceremony-finding-screen`", out.stdout)
        self.assertNotIn("fix-round-scope", out.stdout)
        self.assertIn("`<bead> <severity> <reviewer> <field>: <what is wrong>`", out.stdout)

    def test_fix_round_dispatches_only_filing_reviewers(self):
        out = _render(self.PLAN, _example("plan-review-template-fix-round-vars.json"))
        self.assertEqual(out.returncode, 0, out.stderr)
        text = out.stdout
        self.assertIn("<fix-verification-precedence>", text)
        self.assertIn("fix-round-scope owned --plan --carried", text)
        self.assertIn("fix-round-scope check --plan --carried", text)
        self.assertNotIn("Run `req-qa` and `arch-qa` as background agents", text)
        self.assertNotIn("over the whole set", text)
        self.assertNotIn("Run `ceremony-finding-screen`", text)
        self.assertIn("files no new findings", text)
        self.assertIn("validate-plan", text)  # step b still runs

    def test_plan_scope_reviewer_fix_round_is_locked_to_its_own_ids(self):
        import json
        base = {k: v for k, v in _example("plan-scope-reviewer-assignment-vars.json").items()
                if k not in ("round_index", "carry_forward_findings_json")}
        for scope in (None, "[]", "[ ]", "", "null"):
            with self.subTest(scope=scope):
                values = {**base, "round_index": 2}
                if scope is not None:
                    values["carry_forward_findings_json"] = scope
                self.assertNotEqual(_render(self.PSR, values).returncode, 0)
        first = _render(self.PSR, {**base, "round_index": 1})
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertIs(json.loads(first.stdout)["findings_scope_locked"], False)
        later = _render(self.PSR, {**base, "round_index": 2, "carry_forward_findings_json": '["PLAN-SCOPE-003"]'})
        self.assertEqual(later.returncode, 0, later.stderr)
        data = json.loads(later.stdout)
        self.assertIs(data["findings_scope_locked"], True)
        self.assertEqual(data["carry_forward_findings"], ["PLAN-SCOPE-003"])
        plan = _render(self.PLAN, _example("plan-review-template-fix-round-vars.json"))
        self.assertIn("`round_index` = 2", plan.stdout)

    def test_removed_variable_is_not_required(self):
        values = {**_example("plan-review-template-fix-round-vars.json"), "reviewers_scope_locked": ["x"]}
        self.assertEqual(_render(self.PLAN, values).returncode, 0)


# Config-backed dispatch variables (the lead fills them from .claude/project/atm-bd-orchestration.yaml).
CONFIG_VARS = {
    "dev-template.xml.j2": ("lead", "cc", "test_command", "policy_path"),
    "fix-assignment.xml.j2": ("lead", "cc", "test_command", "policy_path", "requirements_globs", "adr_globs"),
    "dev-fix.xml.j2": ("lead", "cc", "test_command"),
    "dev-sanity-template.xml.j2": ("lead", "cc", "lint_command"),
    "qa-template.xml.j2": ("lead", "cc", "policy_path", "reviewers_round1"),
    "plan-review-template.xml.j2": ("lead", "cc", "integration_branch", "plans_dir", "requirements_globs", "adr_globs"),
    "review-template.xml.j2": ("lead", "cc", "integration_branch"),
    "schema-reviewer-assignment.json.j2": ("policy_path",),
    "qa-bead.json.j2": ("qa_member",),
}
REPO_DEFAULTS = ("team-lead", "just validate", "just lint", "quality-policy.md", "develop", "integrate/phase-",
                 "docs/requirements.md", "docs/architecture.md", "docs/plans", "sc-rust", "rust-development",
                 "practice-inventory")


class ConfigVariableTests(unittest.TestCase):
    """Repository values are required dispatch variables: no defaults, so a missing one fails the render."""

    def test_each_config_variable_is_required(self):
        for template, names in CONFIG_VARS.items():
            values = _example(template.removesuffix(".j2").rsplit(".", 1)[0] + "-vars.json")
            self.assertEqual(_render(template, values).returncode, 0, template)
            for name in names:
                with self.subTest(template=template, variable=name):
                    result = _render(template, {k: v for k, v in values.items() if k != name})
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(name, result.stderr)

    def test_no_repository_default_is_built_in(self):
        for path in sorted((ROOT / "templates").glob("*.j2")):
            text = path.read_text()
            for needle in REPO_DEFAULTS:
                with self.subTest(template=path.name, value=needle):
                    self.assertNotIn(needle, text)

    def test_reviewer_sets_come_from_the_variables(self):
        lists = {"reviewers_round1": ["alpha-qa", "beta-qa", "gamma-qa"]}
        round1 = _render("qa-template.xml.j2", {**_example("qa-template-vars.json"), **lists})
        self.assertEqual(round1.returncode, 0, round1.stderr)
        self.assertIn("run the full reviewer set, `alpha-qa`, `beta-qa` and `gamma-qa`,", round1.stdout)
        fix = _render("qa-template.xml.j2", {**_example("qa-template-fix-round-vars.json"), **lists})
        self.assertEqual(fix.returncode, 0, fix.stderr)
        self.assertNotIn("alpha-qa", fix.stdout)  # a fix is verified by its filing reviewer, not a configured set
        for text in (round1.stdout, fix.stdout):
            self.assertNotIn("`rust-qa-agent`", text)
            self.assertNotIn("`ruthless-boundary-qa`", text)

    def test_removed_reviewer_variables_are_not_required(self):
        values = {**_example("qa-template-fix-round-vars.json"),
                  "reviewers_fix_round": ["req-qa"], "reviewers_scope_locked": ["ruthless-boundary-qa"]}
        result = _render("qa-template.xml.j2", values)
        self.assertEqual(result.returncode, 0, result.stderr)  # a 0.4.0-style var file still renders

    def test_qa_bead_assignee_is_the_configured_member(self):
        import json
        result = _render("qa-bead.json.j2", {**_example("qa-bead-vars.json"), "qa_member": "my-qa"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["assignee"], "my-qa")


class QaLogContractTests(unittest.TestCase):
    """The QA metrics log is a stable contract (paths, record fields and their order); config never changes it."""

    ROUND_ROW = ("'{completed_at:$completed_at, completed_local:$completed_local, duration:$duration, phase:$phase, sprint:$sprint,\n"
                 "    task:$task, pr_number:(($pr_number|tonumber?)//null), iteration:$iteration, verdict:$verdict, tested:$tested,\n"
                 "    fnd:$fnd, blk:$blk, imp:$imp, min:$min}' >> .sc/qa-log/phase-d.jsonl")
    STATS_ROW = ("  {snapshot_at: $completed_at, snapshot_local: $completed_local, phase: $ph, trigger_task: $task,\n"
                 "   tot: ($f | length),\n")

    def test_log_paths_and_fields_are_unchanged(self):
        for example in ("qa-template-vars.json", "qa-template-fix-round-vars.json"):
            with self.subTest(example=example):
                result = _render("qa-template.xml.j2", _example(example))
                self.assertEqual(result.returncode, 0, result.stderr)
                step = result.stdout[result.stdout.index('<step id="j">'):]
                self.assertIn("mkdir -p .sc/qa-log\nLOCKDIR=.sc/qa-log/.append.lock\n", step)
                self.assertIn(self.ROUND_ROW, step)
                self.assertIn(self.STATS_ROW, step)
                self.assertIn("min: ([$f[] | select(.status==\"open\" and .metadata.severity==\"minor\")] | length)}' >> .sc/qa-log/phase-d-stats.jsonl", step)


if __name__ == "__main__":
    unittest.main()
