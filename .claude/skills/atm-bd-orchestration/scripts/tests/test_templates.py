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
        self.assertIn("SANITY.NOT_REBASED", text)

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

    def test_sanity_template_base_mismatch_code_matches_the_coordinator(self):
        text = (ROOT / "templates/dev-sanity-template.xml.j2").read_text()
        self.assertIn("require, else `SANITY.NOT_STACKED`:", text)  # agents/dev-sanity.md and assignment-gates.py sanity
        self.assertNotIn("STALE_BASE", text)
        self.assertIn("git merge-base --is-ancestor origin/<pr_target> origin/{{ base | string | cdata_escape }}", text)
        self.assertNotIn("PR_TARGET_MISMATCH", text)
        self.assertLess(text.index("bd ready -n 0 --json"), text.index("Otherwise claim"))
        self.assertLess(text.index("git fetch origin"), text.index("origin/{{"))  # the tracking ref is fresh before any origin/ check

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

    def test_a_finding_bead_is_a_child_of_the_phase_or_feature_bead_not_the_sprint(self):
        import json, subprocess
        result = subprocess.run(["sc-compose", "render", "--file", str(ROOT / "templates/finding-bead.json.j2"),
                                 "--var-file", str(ROOT / "examples/finding-bead-vars.json"), "--strict"],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        bead = json.loads(result.stdout)
        edges = {d["type"]: d["depends_on_id"] for d in bead["dependencies"]}
        self.assertTrue(edges["parent-child"].endswith("-phase-d"), edges)
        self.assertEqual(edges["discovered-from"], bead["id"].rsplit("-f", 1)[0])
        self.assertTrue(bead["metadata"]["sprint_bead"].endswith("-d-4"))

    def test_every_dev_assignment_has_the_itemized_private_checklist(self):
        for name in ("dev-template", "dev-fix", "fix-assignment"):
            text = (ROOT / f"templates/{name}.xml.j2").read_text()
            with self.subTest(template=name):
                self.assertIn("create an itemized private checklist outside the tracked tree with every task identified, "
                              "and work through the checklist one item at a time", text)
                self.assertIn("go through the checklist again one item at a time", text)

    def test_fix_assignment_takes_a_poured_fix_bead(self):
        head = (ROOT / "templates/fix-assignment.xml.j2").read_text().split("---", 2)[1]
        self.assertIn("task_id is a poured fix bead (`<sprint>.<ref>-r<n>-fix`", head)

    def test_dev_step_a_rebases_before_the_gate(self):
        text = (ROOT / "templates/dev-template.xml.j2").read_text()
        self.assertIn("`git fetch origin && git rebase origin/{{ pr_target | string | cdata_escape }}` in the worktree", text)
        self.assertLess(text.index("git rebase origin/"), text.index("assignment-gates.py dev"))

    def test_fix_step_a_on_a_branch_cut_from_the_top_only_checks_ancestry(self):
        for name in ("fix-assignment", "dev-fix"):
            step = (ROOT / f"templates/{name}.xml.j2").read_text().split('<step id="a">', 1)[1].split("</step>", 1)[0]
            with self.subTest(template=name):
                self.assertNotIn("git rebase", step)
                self.assertLess(step.index("`git fetch origin`"), step.index("assignment-gates.py dev"))
                self.assertIn("`git merge-base --is-ancestor origin/{{ pr_target | string | cdata_escape }} HEAD` exits 0, else `WRONG_BASE`", step)

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
            ({**passed, "post_mortem_jev": {**passed["post_mortem_jev"], "status": "not_applicable"}}, True),
            ({**passed, "post_mortem_jev": {**passed["post_mortem_jev"], "status": "unavailable"}}, False),
            ({k: v for k, v in passed.items() if k != "post_mortem_jev"}, False),
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

    def test_sc_sanity_jev_hands_the_client_an_assignment_it_accepts(self):
        import importlib.util
        text = (ROOT.parents[1] / "agents" / "sc-sanity-jev.md").read_text()
        self.assertIn("python3 .claude/skills/atm-bd-orchestration/scripts/jev_client.py --assignment <file>", text)
        self.assertNotIn("--request", text)
        assignment = json.loads(re.search(r"## Inputs\n\n```json\n(.*?)\n```", text, re.S).group(1))
        spec = importlib.util.spec_from_file_location("jev_client", ROOT / "scripts" / "jev_client.py")
        client = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(client)
        # every field the client reads is in the documented input; only the example's paths are not a repository
        with self.assertRaises(client.JevError) as raised:
            client.assignment_request(assignment)
        self.assertEqual(raised.exception.message, "Committed evidence unreadable at the pinned commits")
        for code in ("SANITY.JEV_UNAVAILABLE", "SANITY.JEV_RESPONSE_INVALID", "SANITY.JEV_INCONCLUSIVE"):
            self.assertIn(code, text)


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

    def test_a_sprint_review_pours_blocking_findings_and_files_the_rest_under_the_phase_feature(self):
        qa = _render(self.QA, _example("qa-template-vars.json"))
        self.assertEqual(qa.returncode, 0, qa.stderr)
        step_g = qa.stdout[qa.stdout.index('<step id="g">'):qa.stdout.index('<step id="h">')]
        self.assertIn("scripts/bead-groups --findings <scratch>/", step_g)
        self.assertIn('"round": 1', step_g)
        self.assertIn("File every other finding (important, minor, or screened `ceremony`)", step_g)
        self.assertIn(f"`parent` = `{_example('qa-template-vars.json')['phase_feature']}`", step_g)
        self.assertNotIn("sprints.jsonl", qa.stdout)

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
        self.assertIn("This is fix verification of fix bead", out)
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
        self.assertNotIn("bd close <finding>", step_g)  # the fixer closes; verification confirms or pours round n+1
        self.assertNotIn("bd reopen", step_g)          # a failed fix verification never reopens
        self.assertIn("scripts/bead-groups --findings <scratch>/", step_g)
        self.assertIn("<its metadata.round + 1>", step_g)
        self.assertIn("PASS requires a PASS from the filing reviewer and the carried finding confirmed fixed.", out)

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
        qa = _render(self.QA, {**_example("qa-template-vars.json"), "carry_forward": "x-d-4.qa1-f1-r1-fix"})
        self.assertEqual(qa.returncode, 0, qa.stderr)
        self.assertIn("This is fix verification of fix bead `x-d-4.qa1-f1-r1-fix`", qa.stdout)


    def test_every_assignment_taking_carried_ids_carries_the_scope_lock(self):
        import json
        for name in ("req-qa", "arch-qa", "flaky-test-qa"):
            base = _example(f"{name}-assignment-vars.json")
            with self.subTest(reviewer=name):
                free = _render(f"{name}-assignment.json.j2", base)
                self.assertEqual(free.returncode, 0, free.stderr)
                self.assertNotIn("SCOPE LOCK", json.loads(free.stdout)["notes"])
                locked = _render(f"{name}-assignment.json.j2", {**base, "carry_forward_findings_json": '["RQ-1"]'})
                self.assertEqual(locked.returncode, 0, locked.stderr)
                data = json.loads(locked.stdout)
                self.assertEqual(data["carry_forward_findings"], ["RQ-1"])
                self.assertIn("Report a disposition (fixed | open | regressed) for each id", data["notes"])


class BlockedRefusalTests(unittest.TestCase):
    """A task whose bead is not ready, or that meets a blocker mid-task, is refused with the bead left open; never held."""

    NOT_READY_STEP = {"dev-template.xml.j2": "a1", "dev-fix.xml.j2": "a1", "fix-assignment.xml.j2": "a1",
                      "dev-sanity-template.xml.j2": "a1", "qa-template.xml.j2": "a2",
                      "review-template.xml.j2": "a", "plan-review-template.xml.j2": "a"}

    def _rendered(self, name: str) -> str:
        result = _render(name, _example(name.removesuffix(".j2").rsplit(".", 1)[0] + "-vars.json"))
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout

    @staticmethod
    def _step(text: str, step: str) -> str:
        start = text.index(f'<step id="{step}">')
        return text[start:text.index("</step>", start)]

    def test_the_not_ready_step_refuses_and_leaves_the_bead_open(self):
        for name, step in self.NOT_READY_STEP.items():
            with self.subTest(template=name):
                text = self._rendered(name)
                body = self._step(text, step)
                self.assertIn("do not claim", body)
                self.assertIn("`bead_state` `open`", body)
                self.assertIn("--blocked-by <blocker>", body)
                self.assertIn("never wait", body)
                self.assertTrue("refused --template .claude/skills/atm-bd-orchestration/templates/task-refused.md.j2" in body
                                or "refuse as in step" in body)
                self.assertNotIn("and wait", text)

    def test_the_ready_check_comes_before_any_claim(self):
        for name in ("qa-template.xml.j2", "dev-sanity-template.xml.j2"):
            with self.subTest(template=name):
                step_a = self._step(self._rendered(name), "a")
                self.assertTrue(step_a.startswith('<step id="a"><![CDATA[Before'))
                self.assertLess(step_a.index("bd ready -n 0 --json"), step_a.index("gh pr view"))

    def test_a_mid_task_blocker_returns_the_bead_open_and_refuses(self):
        for name in self.NOT_READY_STEP:
            with self.subTest(template=name):
                text = self._rendered(name)
                self.assertIn('--status open --assignee "" --append-notes "BLOCKED: <blocker>: <why>"', text)
                self.assertIn("never stay active waiting", text)

    def test_a_shared_change_the_dev_can_make_is_a_quick_fix_not_a_blocker(self):
        for name, step in (("dev-template.xml.j2", "d1"), ("dev-fix.xml.j2", "b1"), ("fix-assignment.xml.j2", "d1")):
            with self.subTest(template=name):
                text = self._rendered(name)
                self.assertIn("Parallel Quick Fix", self._step(text, step))
                self.assertIn(f"A shared change you can make yourself is not a blocker: it is a Parallel Quick Fix (step {step}) and the task continues.", text)

class DevAssignmentTests(unittest.TestCase):
    """Dev, dev-fix and fix assignments: the a1 trigger, actor-stamped bd writes, and the not_reproducible close."""

    def test_a1_fires_on_not_ready_and_every_bd_write_has_an_actor(self):
        for name in ("dev-template.xml.j2", "dev-fix.xml.j2", "fix-assignment.xml.j2"):
            result = _render(name, _example(name.removesuffix(".j2").rsplit(".", 1)[0] + "-vars.json"))
            with self.subTest(template=name):
                self.assertEqual(result.returncode, 0, result.stderr)
                a1 = result.stdout[result.stdout.index('<step id="a1">'):]
                self.assertTrue(a1.startswith('<step id="a1"><![CDATA[When it prints `NOT_READY`'))
                writes = re.findall(r"`(bd (?:close|update) [^`]*)`", result.stdout)
                self.assertTrue(any("--claim" in w for w in writes))
                for write in writes:
                    self.assertIn('--actor "$ATM_IDENTITY"', write)

    def test_not_reproducible_needs_no_commit_but_fixed_does(self):
        base = {k: v for k, v in _example("fix-complete-vars.json").items() if k not in ("commit", "rebased_onto")}
        result = _render("fix-complete.md.j2", {**base, "outcome": "not_reproducible"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("Commit:", result.stdout)
        self.assertNotEqual(_render("fix-complete.md.j2", {**base, "outcome": "fixed"}).returncode, 0)
        self.assertEqual(_render("fix-complete.md.j2", _example("fix-complete-vars.json")).returncode, 0)
        for missing in ("pr_number", "pr_url", "stack_view"):
            with self.subTest(missing=missing):
                values = {k: v for k, v in _example("fix-complete-vars.json").items() if k != missing}
                self.assertNotEqual(_render("fix-complete.md.j2", values).returncode, 0)

    def test_every_dev_lands_its_pr_on_the_current_stack_top(self):
        for name in ("dev-template.xml.j2", "dev-fix.xml.j2", "fix-assignment.xml.j2"):
            result = _render(name, _example(name.removesuffix(".j2").rsplit(".", 1)[0] + "-vars.json"))
            with self.subTest(template=name):
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("Rebase onto the stack's current top (it may have moved since dispatch)", result.stdout)
                self.assertRegex(result.stdout, r"the top is what `\S+/\.claude/skills/atm-bd-orchestration/scripts/assignment-gates\.py stack-top --pr-target \S+` prints on exit 0")
                self.assertIn("any other exit prints a code (`STACK_AMBIGUOUS`, `GATE_CANNOT_RUN`): never guess, refuse with it by step ", result.stdout)
                self.assertNotIn("no output means the top is", result.stdout)
                self.assertNotIn("gh_stack_view.py", result.stdout)
                self.assertIn("run `/sc-gh-stack-view` (read-only) and keep its output verbatim", result.stdout)
                self.assertIn("`git rebase origin/<top>`", result.stdout)
                self.assertRegex(result.stdout, r"`gh pr create --base <top> --head \S+ --fill`")
                self.assertIn("(never `--draft`)", result.stdout)
                self.assertNotIn("rebases only onto", result.stdout)
                self.assertNotIn("Rebase only onto", result.stdout)
                self.assertIn("`pr_target` is a lower bound", result.stdout)
                self.assertNotIn("`metadata.pr_target` equals", result.stdout)

    def test_a_sanity_fail_fix_is_a_new_layer_above_the_frozen_checked_layer(self):
        values = _example("dev-fix-vars.json")
        result = _render("dev-fix.xml.j2", values)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"The sprint's first layer `{values['pr_target']}` and any fix layer above it are linked and frozen: never rebase, re-target or push them.", result.stdout)
        self.assertIn(f"`{values['branch']}` is a new layer cut from the top of stack", result.stdout)
        self.assertNotIn("is not linked", result.stdout)
        self.assertNotIn("confirm the open one", result.stdout)

    def test_after_a_dev_fix_sanity_and_qa_diff_only_the_sprints_own_layer_prs(self):
        values = _example("dev-sanity-template-vars.json")
        plain = _render("dev-sanity-template.xml.j2", values)
        self.assertEqual(plain.returncode, 0, plain.stderr)
        self.assertIn("<layer-prs><![CDATA[]]></layer-prs>", plain.stdout)
        fixed = _render("dev-sanity-template.xml.j2", {**values, "layer_prs": [250, 262]})
        self.assertEqual(fixed.returncode, 0, fixed.stderr)
        self.assertIn("<layer-prs><![CDATA[250 262]]></layer-prs>", fixed.stdout)
        self.assertIn("Run sanity-split once, with `--layer-pr <n>` for each PR in `<layer-prs>` (empty: the single range from `<base>`)", fixed.stdout)
        self.assertIn('--base "$base" "${layer_pr_args[@]}"', (ROOT.parents[1] / "agents/dev-sanity.md").read_text())
        self.assertNotIn("diff_base", (ROOT / "templates/dev-sanity-template.xml.j2").read_text())
        qa = _example("qa-template-vars.json")
        single = _render("qa-template.xml.j2", qa)
        self.assertEqual(single.returncode, 0, single.stderr)
        self.assertIn(f"The change under review is `git diff origin/{qa['base']}...{qa['commit']}`, never", single.stdout)
        self.assertIn(f"change = git diff origin/{qa['base']}...{qa['commit']}.", single.stdout)
        layered = _render("qa-template.xml.j2", {**qa, "layer_prs": [250, 262]})
        self.assertEqual(layered.returncode, 0, layered.stderr)
        self.assertIn("The change under review is the sprint's own layer ranges, `git diff <baseRefOid>...<headRefOid>` of each PR 250, 262 "
                      "(`gh pr view <n> --json baseRefOid,headRefOid`), never", layered.stdout)
        self.assertIn("change = git diff <baseRefOid>...<headRefOid> of each layer PR 250 262 (gh pr view <n> --json baseRefOid,headRefOid).", layered.stdout)
        self.assertNotIn(f"git diff origin/{qa['base']}", layered.stdout)

    def test_sanity_reads_the_stack_from_githubs_stacks_api(self):
        for text in ((ROOT / "templates/dev-sanity-template.xml.j2").read_text(), (ROOT.parents[1] / "agents/dev-sanity.md").read_text()):
            with self.subTest(text=text[:40]):
                self.assertIn("gh api 'repos/{owner}/{repo}/stacks' --paginate --jq '.[]'`", text)
                self.assertIn("never local `gh stack` tracking", text)
                self.assertNotIn("gh_stack_view.py", text)
                self.assertNotIn("`gh stack view --json` in the", text)

    def test_completion_renders_a_stack_issue_unless_coherent_and_landable(self):
        for name in ("dev-complete.md.j2", "fix-complete.md.j2"):
            values = _example(name.removesuffix(".md.j2") + "-vars.json")
            good = _render(name, values)
            with self.subTest(template=name):
                self.assertEqual(good.returncode, 0, good.stderr)
                self.assertNotIn("## Stack issue", good.stdout)
                self.assertIn(values["stack_view"], good.stdout)
                self.assertIn(f"Verify PR #{values['pr_number']} exists and link it on top of the phase stack", good.stdout)
                self.assertIn("do not message or re-dispatch the dev for stacking", good.stdout)
                for bad in (values["stack_view"].replace("VERDICT: ✅ COHERENT", "VERDICT: ❌ NOT COHERENT"),
                            values["stack_view"].replace("LANDING: ✅", "LANDING: ❌"),
                            values["stack_view"].replace("LANDING: ✅", "LANDING: ❓")):
                    issue = _render(name, {**values, "stack_view": bad})
                    self.assertEqual(issue.returncode, 0, issue.stderr)
                    self.assertLess(issue.stdout.index("## Stack issue"), issue.stdout.index("## Next (task assigner)"))


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
    "dev-template.xml.j2": ("test_command", "policy_path"),
    "fix-assignment.xml.j2": ("test_command", "policy_path", "requirements_globs", "adr_globs"),
    "dev-fix.xml.j2": ("test_command",),
    "dev-sanity-template.xml.j2": ("lint_command",),
    "qa-template.xml.j2": ("policy_path", "reviewers_round1"),
    "plan-review-template.xml.j2": ("integration_branch", "plans_dir", "requirements_globs", "adr_globs"),
    "review-template.xml.j2": ("integration_branch",),
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

    def test_no_template_names_a_recipient(self):
        """Closes and reports go to the task assigner; no template names or computes a recipient."""
        for path in sorted((ROOT / "templates").glob("*.j2")):
            text = path.read_text().lower()
            for needle in ("to the lead", "to lead", "team-lead", "{{ lead", "{{ cc", "atm send {{",
                           "\n  - lead\n", "\n  - cc\n"):
                with self.subTest(template=path.name, needle=needle):
                    self.assertNotIn(needle, text)

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

    def test_qa_bead_marks_a_quick_fix_explicitly(self):
        import json
        plain = _render("qa-bead.json.j2", _example("qa-bead-vars.json"))
        self.assertEqual(plain.returncode, 0, plain.stderr)
        self.assertIs(json.loads(plain.stdout)["metadata"]["quick_fix"], False)
        quick = _render("qa-bead.json.j2", {**_example("qa-bead-vars.json"), "quick_fix": True})
        self.assertEqual(quick.returncode, 0, quick.stderr)
        self.assertIs(json.loads(quick.stdout)["metadata"]["quick_fix"], True)
        self.assertIsNone(json.loads(plain.stdout)["metadata"]["pr_target"])
        bounded = _render("qa-bead.json.j2", {**_example("qa-bead-vars.json"), "quick_fix": True, "pr_target": "sprint/d-2"})
        self.assertEqual(bounded.returncode, 0, bounded.stderr)
        self.assertEqual(json.loads(bounded.stdout)["metadata"]["pr_target"], "sprint/d-2")


class QaLogContractTests(unittest.TestCase):
    """The QA metrics log is a stable contract (paths, record fields and their order); config never changes it."""

    ROUND_ROW = ("'{completed_at:$completed_at, completed_local:$completed_local, duration:$duration, phase:$phase, sprint:$sprint,\n"
                 "    task:$task, pr_number:(($pr_number|tonumber?)//null), iteration:$iteration, verdict:$verdict, tested:$tested,\n"
                 "    fnd:$fnd, blk:$blk, imp:$imp, min:$min}')\nprintf '%s\\n' \"$ROW\" >> .sc/qa-log/phase-d.jsonl")
    STATS_ROW = ("  {snapshot_at: $completed_at, snapshot_local: $completed_local, phase: $ph, trigger_task: $task,\n"
                 "   tot: ($f | length),\n")

    def test_log_paths_and_fields_are_unchanged(self):
        for example in ("qa-template-vars.json", "qa-template-fix-round-vars.json"):
            with self.subTest(example=example):
                result = _render("qa-template.xml.j2", _example(example))
                self.assertEqual(result.returncode, 0, result.stderr)
                step = result.stdout[result.stdout.index('<step id="j">'):]
                self.assertIn("mkdir -p .sc/qa-log\n", step)
                self.assertNotIn("LOCKDIR", step)   # one printf append per row; no lock
                self.assertIn(self.ROUND_ROW, step)
                self.assertIn(self.STATS_ROW, step)
                self.assertIn("min: ([$f[] | select(.status==\"open\" and .metadata.severity==\"minor\")] | length)}')\nprintf '%s\\n' \"$STATS\" >> .sc/qa-log/phase-d-stats.jsonl", step)


if __name__ == "__main__":
    unittest.main()
