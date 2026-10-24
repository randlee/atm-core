"""Every handoff that sends fenced JSON (or a command line) to another agent or script matches its receiver's declared input.

The sender side is what the skill tells an agent to send (a rendered assignment template, an instruction paragraph); the
receiver side is the parser or the `## Inputs` contract the receiver publishes. A reviewer whose agent file is not shipped
by this package (arch-qa, req-qa, ...) is checked against its agent file only when the host repository has one.
"""
from __future__ import annotations

import ast
import importlib.machinery
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

from subprocess_helpers import run_command
import rendering

ROOT = Path(__file__).parents[2]
AGENTS = ROOT.parents[1] / "agents"
TEMPLATES = ROOT / "templates"
EXAMPLES = ROOT / "examples"
def jev_environment():
    return {key: os.environ[key] for key in ("PATH", "HOME") if key in os.environ}


class BoundedCommandTests(unittest.TestCase):
    def test_jev_environment_excludes_ambient_settings(self):
        with patch.dict(os.environ, {"PATH": "/test/bin", "HOME": "/test/home", "TYPESAFE_API_KEY": "secret",
                                     "JEV_URL": "ambient"}, clear=True):
            self.assertEqual(jev_environment(), {"PATH": "/test/bin", "HOME": "/test/home"})


def render(template: str, variables: Path) -> str:
    result = rendering.render(TEMPLATES / template, variables)
    assert result.returncode == 0, result.stderr
    return result.stdout


# A receiver's declared input: the first ```json fence under its `## Inputs` or `## Input Contract` heading.
INPUTS = re.compile(r"^## (?:Inputs|Input Contract)\b.*?```json\n(.*?)\n```", re.S | re.M)


def required_match(match: re.Match[str] | None) -> re.Match[str]:
    assert match is not None
    return match


def inputs_json(agent: str) -> dict:
    return json.loads(required_match(INPUTS.search((AGENTS / agent).read_text())).group(1))


# Fenced JSON as agents send and return it.
FENCE = re.compile(r"^```(jsonl?)\n(.*?)^```", re.S | re.M)
# Example placeholders: a `<...>` or install-time placeholder inside a JSON string, then a bare one standing for a value.
QUOTED_PLACEHOLDER = re.compile(r'"[^"\n]*(?:<[^"\n>]*>|\{\{[^"\n}]*\}\})[^"\n]*"')
BARE_PLACEHOLDER = re.compile(r"<[^<>\n]*>|\{\{[^}\n]*\}\}")
# What this package ships; a host repository's other agents and skills are not this package's handoffs.
PACKAGE_AGENTS = ("ceremony-qa.md", "dev-sanity.md", "parallax.md", "sc-sanity-jev.md", "sc-sanity-llm.md")
PACKAGE_SKILLS = ("atm-bd-orchestration", "atm-beads", "prompt-rewrite-judge", "qa-report", "sprint-report", "sprint-review")


def fence(value) -> str:
    return "```json\n" + json.dumps(value, indent=2) + "\n```\n"


def example_json(body: str):
    """An example fence's body with its placeholders filled; raises ValueError when it is not JSON."""
    return json.loads(BARE_PLACEHOLDER.sub('"X"', QUOTED_PLACEHOLDER.sub('"X"', body)))


def fenced_json(text: str, heading: str):
    """The JSON object in the ```json fence right after `heading`; raises ValueError when it is missing or not JSON."""
    match = re.search(re.escape(heading) + r"\n```json\n(.*?)\n```", text, re.S)
    if not match:
        raise ValueError(f"no ```json fence after {heading!r}")
    return json.loads(match.group(1))


def section_fence(agent: str, heading: str) -> str:
    return required_match(re.search(r"^## " + heading + r"\b.*?```json\n(.*?)\n```", (AGENTS / agent).read_text(), re.S | re.M)).group(1)


def load_client():
    spec = importlib.util.spec_from_file_location("jev_client", ROOT / "scripts" / "jev_client.py")
    assert spec is not None and spec.loader is not None
    client = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(client)
    return client


def load_merge():
    loader = importlib.machinery.SourceFileLoader("sanity_merge", str(ROOT / "scripts" / "sanity-merge"))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    assert spec is not None
    merge = importlib.util.module_from_spec(spec)
    loader.exec_module(merge)
    return merge


def rerun_paragraph() -> str:
    text = (AGENTS / "dev-sanity.md").read_text()
    start = text.index("A rerun supplies one")
    return text[start:text.index("valid original reply", start)]


class DevSanityHandoffTests(unittest.TestCase):
    """dev-sanity -> sc-sanity-llm / sc-sanity-jev / jev_client.py, and dev-sanity -> sanity-merge."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        repo = Path(self.tmp.name) / "repo"
        repo.mkdir()

        def git(*args):
            return run_command(["git", "-C", str(repo), "-c", "user.email=t@t", "-c", "user.name=t", *args],
                                  check=True, capture_output=True, text=True).stdout.strip()

        git("init", "-q")
        (repo / "crates/example-otlp/src").mkdir(parents=True)
        (repo / "docs").mkdir()
        (repo / "docs/retry.md").write_text("retry policy\n")
        (repo / "docs/limits.md").write_text("limits\nbackoff 2s\njitter on\n")
        (repo / "crates/example-otlp/src/lib.rs").write_text("// old\n")
        git("add", "-A")
        git("commit", "-qm", "base")
        self.base = git("rev-parse", "HEAD")
        (repo / "crates/example-otlp/src/lib.rs").write_text("// exporter\n")
        git("add", "-A")
        git("commit", "-qm", "work")
        self.commit, self.repo = git("rev-parse", "HEAD"), repo

    def assignment(self, fixture: str) -> dict:
        variables = json.loads((EXAMPLES / fixture).read_text())
        variables.update(worktree_path=str(self.repo), commit=self.commit, base_sha=self.base)
        path = Path(self.tmp.name) / "vars.json"
        path.write_text(json.dumps(variables))
        return json.loads(render("dev-sanity-assignment.json.j2", path))

    def test_rendered_assignment_has_exactly_the_reviewers_input_fields(self):
        for fixture in ("dev-sanity-assignment-vars.json", "dev-sanity-assignment-context-vars.json"):
            for agent in ("sc-sanity-llm.md", "sc-sanity-jev.md"):
                with self.subTest(fixture=fixture, agent=agent):
                    self.assertEqual(set(self.assignment(fixture)), set(inputs_json(agent)))

    def test_rendered_assignment_is_accepted_by_the_jev_client(self):
        client = load_client()
        for fixture in ("dev-sanity-assignment-vars.json", "dev-sanity-assignment-context-vars.json"):
            with self.subTest(fixture=fixture):
                (request,) = client.assignment_requests(self.assignment(fixture))
                self.assertIn("deliverable", request["state"])

    def test_rerun_instruction_sends_context_the_receivers_accept(self):
        paragraph = rerun_paragraph()
        self.assertNotIn("{path, why}", paragraph)
        match = re.search(r"--argjson context '<\[(.*?)\]>'", paragraph)
        self.assertIsNotNone(match, "dev-sanity.md must show the rerun context as a JSON array of path strings")
        assert match is not None
        self.assertEqual(match.group(1), '"path", ...')
        self.assertIn("`path:start-end`", paragraph)
        base = self.assignment("dev-sanity-assignment-vars.json")
        manifest = Path(self.tmp.name) / "manifest.json"
        manifest.write_text(json.dumps({"assignments": [{"number": 1, "assignment": base}]}))
        jq = required_match(re.search(r"`(jq --argjson n <n> .*?)`;", paragraph, re.S)).group(1)
        jq = jq.replace("<n>", "1").replace("'<[\"path\", ...]>'", "\"$CONTEXT\"").replace("$manifest", str(manifest))
        pinned = {"worktree_path": str(self.repo), "sha": self.commit}
        # the documented shapes, a whole file and a line range, filled with real entries at the pinned commit
        for context, read in ((["docs/retry.md"], {"docs/retry.md": "retry policy\n"}),
                              (["docs/limits.md:2-3"], {"docs/limits.md:2-3": "backoff 2s\njitter on\n"})):
            with self.subTest(context=context):
                rerun = run_command(["bash", "-c", jq], env={"CONTEXT": json.dumps(context), "PATH": "/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin"},
                                       capture_output=True, text=True)
                self.assertEqual(rerun.returncode, 0, rerun.stderr)
                sent = json.loads(rerun.stdout)
                self.assertEqual(sent["context"], context)
                (request,) = load_client().assignment_requests(sent)   # what the client reads
                self.assertEqual(request["state"]["context"], read)
                self.assertTrue(load_merge().context_path_exists(pinned, context[0]))   # what sanity-merge's rerun check accepts
        self.assertFalse(load_merge().context_path_exists(pinned, "docs/limits.md:3-9"))

    def test_the_old_rerun_shape_is_rejected_by_the_receiver(self):
        sent = {**self.assignment("dev-sanity-assignment-vars.json"), "context": [{"path": "docs/retry.md", "why": "x"}]}
        client = load_client()
        with self.assertRaises(client.JevError):
            client.assignment_requests(sent)

    def test_selection_entry_keys_in_the_instruction_are_the_ones_sanity_merge_requires(self):
        text = (AGENTS / "dev-sanity.md").read_text()
        anchor = re.search(r"`sanity-merge` accepts exactly these keys(?: per entry)?:", text)
        self.assertIsNotNone(anchor, "dev-sanity.md must list the selection entry keys sanity-merge accepts")
        assert anchor is not None
        paragraph = text[anchor.start():]
        paragraph = paragraph[:paragraph.index("\n\n")]
        documented = set(re.findall(r"`(\w+)`", paragraph[anchor.end() - anchor.start():].split(". ", 1)[0]))
        merge = (ROOT / "scripts/sanity-merge").read_text()
        required = ast.literal_eval(required_match(re.search(r"required = (\{[^}]*\})", merge)).group(1))
        self.assertEqual(documented, required)
        self.assertIn("`{reviewer, context, reply}`", paragraph)
        self.assertIn("set(rerun) != {'reviewer', 'context', 'reply'}".replace("'", '"'), merge)


    def test_the_jev_client_takes_the_assignment_exactly_as_received_in_its_fence(self):
        """sc-sanity-jev writes the assignment it received (fenced JSON, as dev-sanity sends it) unchanged to a file and
        runs `jev_client.py --assignment <file>`; the client must read past the fence. With no key set the call stops at
        the key check, so reaching it proves the assignment was parsed and its committed evidence read."""
        sent = self.assignment("dev-sanity-assignment-vars.json")
        env = jev_environment()

        def run(body: str) -> dict:
            path = Path(self.tmp.name) / "assignment.md"
            path.write_text(body)
            proc = run_command([sys.executable, str(ROOT / "scripts/jev_client.py"), "--assignment", str(path)],
                                  capture_output=True, text=True, env=env)
            self.assertEqual(proc.returncode, 2, proc.stderr)
            return json.loads(proc.stdout)["error"]

        for body in (fence(sent), json.dumps(sent)):
            with self.subTest(fenced=body.startswith("```")):
                self.assertEqual(run(body)["message"], "TYPESAFE_API_KEY is missing; no Jev evaluation ran")
        # planted negatives: a broken body in the fence, and the old rerun context shape inside a well-formed fence
        self.assertEqual(run(fence(sent)[:-6] + "\n```\n")["message"], "Request file unavailable or invalid JSON")
        old = run(fence({**sent, "context": [{"path": "docs/retry.md", "why": "x"}]}))
        self.assertEqual((old["code"], old["message"]),
                         ("VALIDATION.INPUT", "Assignment lacks deliverable text, worktree, commits or file lists"))

class ReviewerHandoffTests(unittest.TestCase):
    """quality-mgr / plan review -> reviewers, and -> fix-round-scope."""

    def test_ceremony_qa_instruction_names_its_declared_inputs(self):
        declared = inputs_json("ceremony-qa.md")
        checked = 0
        for template in ("qa-template.xml.j2", "plan-review-template.xml.j2"):
            text = (TEMPLATES / template).read_text()
            for mention in re.findall(r"`ceremony-qa` with (.*?)\.", text):
                with self.subTest(template=template):
                    checked += 1
                    keys = set(re.findall(r"`(\w+)` =", mention))
                    self.assertTrue(keys, mention)
                    self.assertLessEqual(keys, set(declared))
                    self.assertIn(f"`review_mode` = `{declared['review_mode']}`", mention)
        self.assertGreater(checked, 0)

    def test_reviewer_assignments_render_valid_json_matching_the_receivers_agent_file_when_present(self):
        for template in sorted(TEMPLATES.glob("*-assignment.json.j2")):
            reviewer = template.name.removesuffix("-assignment.json.j2")
            if reviewer == "dev-sanity":
                continue
            with self.subTest(reviewer=reviewer):
                sent = json.loads(render(template.name, EXAMPLES / f"{reviewer}-assignment-vars.json"))
                agent = AGENTS / f"{reviewer}.md"
                if not agent.is_file() or not INPUTS.search(agent.read_text()):
                    self.skipTest(f"{reviewer}.md is absent or declares no ## Inputs JSON here; sender checked as valid JSON only")
                self.assertLessEqual(set(sent), set(inputs_json(f"{reviewer}.md")))

    def test_fix_round_scope_flags_in_the_senders_exist_in_the_script(self):
        script = (ROOT / "scripts/fix-round-scope").read_text()
        declared = set(re.findall(r'add_argument\("(--[\w-]+)"', script)) | {"-h", "--help"}
        commands = set(required_match(re.search(r'choices=\(([^)]*)\)', script)).group(1).replace('"', "").replace(" ", "").split(","))
        seen = 0
        for template in ("qa-template.xml.j2", "plan-review-template.xml.j2"):
            text = (TEMPLATES / template).read_text()
            for command, rest in re.findall(r"fix-round-scope (\w+) ((?:--?[\w-]+|<[^>]*>|\{\{.*?\}\}|[\w./<>-]+| )*)", text):
                with self.subTest(template=template, command=command):
                    seen += 1
                    self.assertIn(command, commands)
                    self.assertLessEqual(set(re.findall(r"(?<!\S)(--[\w-]+)", rest)), declared)
        self.assertGreater(seen, 0)


class FencedJsonTests(unittest.TestCase):
    """Every JSON example an agent is told to send or return sits in a ```json fence that parses, and every rendered
    completion carries a Machine Status fence that parses and holds the values it was rendered from."""

    def test_every_json_fence_in_the_package_parses_once_placeholders_are_filled(self):
        documents = [AGENTS / name for name in PACKAGE_AGENTS if (AGENTS / name).is_file()]
        documents += [doc for name in PACKAGE_SKILLS for doc in sorted((ROOT.parent / name).rglob("*.md"))
                      if "node_modules" not in doc.parts]
        checked = 0
        for doc in documents:
            text = doc.read_text()
            for match in FENCE.finditer(text):
                line = text[:match.start()].count("\n") + 1
                with self.subTest(fence=f"{doc.name}:{line}"):
                    checked += 1
                    if match.group(1) == "jsonl":
                        rows = [example_json(row) for row in match.group(2).splitlines() if row.strip()]
                        self.assertTrue(rows)
                    else:
                        example_json(match.group(2))
        self.assertGreaterEqual(checked, 9)  # the reviewers' inputs and outputs, the plan file, the post-mortem spec
        # planted negative: a key list written as if it were JSON is rejected by the same check
        with self.assertRaises(ValueError):
            example_json('{"findings": [{"ref": "qa1-f<n>", "severity": "blocking", "reviewer", "title"}]}')

    def test_every_machine_status_fence_renders_to_json_carrying_its_vars(self):
        hostile = 'x-d-4 "quoted" back\\\\slash\nnew line'
        rendered = 0
        for template in sorted(TEMPLATES.glob("*.md.j2")):
            if "## Machine Status (JSON)" not in template.read_text():
                continue
            values = json.loads((EXAMPLES / (template.name.removesuffix(".md.j2") + "-vars.json")).read_text())
            for task_id in (values["task_id"], hostile):
                with self.subTest(template=template.name, task_id=task_id), tempfile.TemporaryDirectory() as tmp:
                    path = Path(tmp) / "vars.json"
                    path.write_text(json.dumps({**values, "task_id": task_id}))
                    machine = fenced_json(render(template.name, path), "## Machine Status (JSON)")
                    rendered += 1
                    self.assertEqual(machine["task"], task_id)
                    for key in set(machine) & {"sprint", "branch", "commit", "verdict", "phase", "checked_bead"}:
                        self.assertEqual(machine[key], values[key], key)
        self.assertEqual(rendered, 2 * 7)
        # planted negative: a Machine Status block whose JSON is cut short is rejected
        text = render("task-refused.md.j2", EXAMPLES / "task-refused-vars.json").replace('"commit"', '"commit', 1)
        with self.assertRaises(ValueError):
            fenced_json(text, "## Machine Status (JSON)")


class SanityReplyHandoffTests(unittest.TestCase):
    """sc-sanity-llm / sc-sanity-jev `## Output Format` and failure envelope -> dev-sanity keeps the fenced reply text
    -> sanity-merge parses and validates it."""

    SHA = "c0ffee" + "0" * 34

    def reply(self, agent: str) -> tuple[str, dict]:
        """The documented reply as the reviewer returns it (fenced), pinned to the documented assignment's beads."""
        body = section_fence(agent, "Output Format").replace("<full 40-char sha>", self.SHA)
        return "```json\n" + body + "\n```", inputs_json(agent)

    def status(self, merge, reply: str, assignment: dict):
        manifest = {"sha": self.SHA, "deliverables_total": assignment["deliverables_total"]}
        return merge.reply_status(merge.parse_reply(reply, 0), 0, manifest, assignment["sanity_bead"],
                                  assignment["dev_bead"]["id"])

    def test_the_documented_reply_is_a_result_sanity_merge_accepts(self):
        merge = load_merge()
        for agent in ("sc-sanity-llm.md", "sc-sanity-jev.md"):
            with self.subTest(agent=agent):
                reply, assignment = self.reply(agent)
                self.assertEqual(self.status(merge, reply, assignment), (assignment["deliverable"]["number"], "undone"))
                envelope = json.loads(reply.removeprefix("```json\n").removesuffix("\n```"))
                # planted negatives: two findings, a wrong finding kind, another commit, a fence cut short
                for broken in ({**envelope, "data": {**envelope["data"], "findings": envelope["data"]["findings"] * 2}},
                               {**envelope, "data": {**envelope["data"], "findings": [
                                   {**envelope["data"]["findings"][0], "kind": "error"}]}},
                               {**envelope, "data": {**envelope["data"], "commit_checked": "f" * 40}}):
                    with self.assertRaises(merge.Reject):
                        self.status(merge, fence(broken), assignment)
                with self.assertRaises(merge.Reject):
                    self.status(merge, reply[:-5] + "\n```", assignment)

    def test_the_documented_failure_envelope_is_one_sanity_merge_accepts(self):
        merge = load_merge()
        typed = {"code": "SANITY.TARGET_UNREADABLE", "message": "worktree unreadable", "recoverable": False,
                 "suggested_action": "re-pin the target"}
        for agent in ("sc-sanity-llm.md", "sc-sanity-jev.md"):
            with self.subTest(agent=agent):
                text = (AGENTS / agent).read_text()
                keys = required_match(re.search(r"`error: \{([^}]*)\}`", text)).group(1).replace(" ", "").split(",")
                assignment = inputs_json(agent)
                number = assignment["deliverable"]["number"]
                error = {key: typed.get(key, number) for key in keys}
                envelope = {"success": False, "data": None, "error": error}
                self.assertEqual(self.status(merge, fence(envelope), assignment), (number, "cannot_run"))
                with self.assertRaises(merge.Reject):  # planted negative: the deliverable number left out
                    self.status(merge, fence({**envelope, "error": {k: v for k, v in error.items() if k != "deliverable"}}),
                                assignment)

    def test_the_documented_jev_field_is_the_clients_receipt_and_only_a_real_one_passes(self):
        import jev_receipts
        merge, client = load_merge(), load_client()
        reply, assignment = self.reply("sc-sanity-jev.md")
        envelope = json.loads(reply.removeprefix("```json\n").removesuffix("\n```"))
        self.assertEqual(set(envelope["data"]["jev"]), set(client.RECEIPT_KEYS))
        os.environ["TYPESAFE_API_KEY"], saved = jev_receipts.KEY, os.environ.get("TYPESAFE_API_KEY")
        try:
            real = jev_receipts.receipted(envelope)
            self.assertIsNone(merge.jev_reply_problem(real, (client, None), set()))
            self.assertEqual(self.status(merge, fence(real), assignment), (assignment["deliverable"]["number"], "undone"))
            # planted negative: the documented example's own receipt was written by hand, not by the client
            self.assertIn("no valid Jev receipt", merge.jev_reply_problem(envelope, (client, None), set()))
        finally:
            os.environ.pop("TYPESAFE_API_KEY") if saved is None else os.environ.__setitem__("TYPESAFE_API_KEY", saved)


class FixRoundResultHandoffTests(unittest.TestCase):
    """A verifier's fenced-JSON result -> quality-mgr -> `fix-round-scope filter --result <its result>`."""

    def test_filter_reads_the_reviewers_fenced_result(self):
        carried = [{"id": "x-d-4.qa1-f1-r1-fix", "metadata": {"reviewer": "arch-qa", "finding_ref": "ARCH-F001"}}]
        result = {"success": True, "data": {"findings": [
            {"id": "ARCH-F001", "disposition": "fixed", "evidence": "src/lib.rs:3 now bounded"}]}, "error": None}
        with tempfile.TemporaryDirectory() as tmp:
            carried_path, result_path = Path(tmp) / "carried.json", Path(tmp) / "result.md"
            carried_path.write_text(json.dumps(carried))

            def run(body: str) -> subprocess.CompletedProcess:
                result_path.write_text(body)
                return run_command([sys.executable, str(ROOT / "scripts/fix-round-scope"), "filter", "--carried",
                                       str(carried_path), "--reviewer", "arch-qa", "--result", str(result_path)],
                                      capture_output=True, text=True)

            for body in (fence(result), json.dumps(result)):
                with self.subTest(fenced=body.startswith("```")):
                    proc = run(body)
                    self.assertEqual(proc.returncode, 0, proc.stderr)
                    self.assertEqual(json.loads(proc.stdout)["dispositions"][0]["disposition"], "fixed")
            broken = run(fence(result)[:-6] + "\n```\n")  # planted negative: the fenced result cut short
            self.assertEqual(broken.returncode, 2)
            self.assertIn("not the reviewer's fenced or raw JSON result", broken.stderr)
            self.assertNotIn("Traceback", broken.stderr)


class DocumentedShapeTests(unittest.TestCase):
    """Fenced shapes in the skills' references, each accepted by the code that reads it."""

    def test_the_post_mortem_question_mapping_is_accepted_by_its_validator(self):
        sys.path.insert(0, str(ROOT / "scripts"))
        import post_mortem_jev
        text = (ROOT / "references/post-mortem-jev.md").read_text()
        (body,) = [m.group(2) for m in FENCE.finditer(text) if m.group(1) == "json"]
        specs = json.loads(body)
        questions = {qid: {"criteria": {"yes": "y", "no": "n", "insufficient": "?"}} for qid in specs}
        coverage = {"scope": "whole_finding", "obligation_ids": ["o1"]}
        predicates = [{"id": "o1", "text": "rejected input leaves persisted data unchanged"}]
        self.assertEqual(post_mortem_jev.validate_question_specs(questions, specs, coverage, predicates), {"o1"})
        for qid, spec in specs.items():  # planted negative: the author-declared polarity left out
            broken = {**specs, qid: {k: v for k, v in spec.items() if k != "expected"}}
            with self.assertRaises(ValueError):
                post_mortem_jev.validate_question_specs(questions, broken, coverage, predicates)

    def test_the_plan_file_example_is_accepted_by_the_plan_parser(self):
        sys.path.insert(0, str(ROOT.parent / "atm-beads/scripts"))
        import sprint_index_common
        text = (ROOT.parent / "atm-beads/resources/planning.md").read_text()
        (body,) = [m.group(2) for m in FENCE.finditer(text) if m.group(1) == "jsonl"]
        rows, problems = sprint_index_common.parse_plan(body, "planning.md")
        self.assertEqual(problems, [])
        self.assertTrue(rows)
        _, problems = sprint_index_common.parse_plan(body.replace('"depends_on": [', '"depends_on": "', 1)
                                                     .replace('"]}', '"}', 1), "planning.md")
        self.assertTrue(problems)  # planted negative: depends_on as a string

    def test_the_sprint_report_variable_examples_render_the_report(self):
        report = ROOT.parent / "sprint-report"
        text = (report / "SKILL.md").read_text()
        blocks = [m.group(2) for m in FENCE.finditer(text) if m.group(1) == "json"]
        self.assertEqual(len(blocks), 2)
        with tempfile.TemporaryDirectory() as tmp:
            for body in blocks:
                values = json.loads(body)
                for case, sent, ok in (("documented", values, True),
                                       ("planted negative", {k: v for k, v in values.items() if k != "integration_row"}, False)):
                    with self.subTest(mode=values["mode"], case=case):
                        path = Path(tmp) / "vars.json"
                        path.write_text(json.dumps(sent))
                        proc = rendering.render(report / "report.md.j2", path)
                        self.assertEqual(proc.returncode == 0, ok, proc.stderr)
                        if ok:
                            self.assertIn(values["integration_row"], proc.stdout)

    def test_the_sanity_bead_template_renders_a_bead_the_plan_schema_accepts(self):
        sys.path.insert(0, str(ROOT.parent / "atm-beads/scripts"))
        import bead_schema
        beads = ROOT.parent / "atm-beads"
        proc = rendering.render(beads / "templates/dev-sanity-bead.json.j2", beads / "examples/dev-sanity-bead-vars-d-4.json")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        bead = json.loads(proc.stdout)
        self.assertEqual(bead_schema.problems(bead, bead_schema.SanityBead), [])
        unblocked = {**bead, "dependencies": [d for d in bead["dependencies"] if d["type"] != "blocks"]}
        self.assertTrue(bead_schema.problems(unblocked, bead_schema.SanityBead))  # planted negative


def step(rendered: str, step_id: str) -> str:
    start = rendered.index(f'<step id="{step_id}">')
    return rendered[start:rendered.index("</step>", start)]


def named_fields(instruction: str) -> dict:
    """The fenced JSON an instruction tells its sender to write: each `key` = `value` it names (a described value as text)."""
    sent = {key: value or "<as described>" for key, value in re.findall(r"`(\w+)` = (?:`([^`]*)`)?", instruction)}
    return json.loads(required_match(FENCE.search(fence(sent))).group(2))


def required_inputs(agent: str) -> set:
    return set(re.findall(r"^- `(\w+)` \(required\)", (AGENTS / agent).read_text(), re.M))


# ceremony-finding-screen is a host agent (the host repository's .claude/agents/ceremony-finding-screen.md `## Inputs`), not shipped here.
CEREMONY_FINDING_SCREEN_REQUIRED = {"worktree_path", "sprint_doc", "findings"}
RENDER_ASSIGNMENTS = ("Render every reviewer's structured JSON assignment from "
                      "`.claude/skills/atm-bd-orchestration/templates/<reviewer>-assignment.json.j2`; if a reviewer has no "
                      "template there, give it the fenced-JSON input its own contract, `.claude/agents/<reviewer>.md`, names.")


class PlanReviewHandoffTests(unittest.TestCase):
    """plan-review-template -> req-qa / arch-qa / ceremony-qa / fix-round reviewers / ceremony-finding-screen."""

    def plan(self, example: str) -> tuple[dict, str]:
        values = json.loads((EXAMPLES / example).read_text())
        return values, render("plan-review-template.xml.j2", EXAMPLES / example)

    def assignment(self, reviewer: str, **values) -> dict:
        """The reviewer's assignment template rendered as the plan review fills it, sent as a ```json fence."""
        base = json.loads((EXAMPLES / f"{reviewer}-assignment-vars.json").read_text())
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "vars.json"
            path.write_text(json.dumps({**base, **values}))
            return json.loads(required_match(FENCE.search(fence(json.loads(render(f"{reviewer}-assignment.json.j2", path))))).group(2))

    def renders_assignments(self, step_c: str, worktree: str) -> bool:
        return RENDER_ASSIGNMENTS in step_c and f"Each carries worktree `{worktree}`" in step_c and '("Rendering")' in step_c

    def test_round_one_reviewers_get_their_assignments_with_the_worktree(self):
        values, text = self.plan("plan-review-template-vars.json")
        worktree, branch, step_c = values["worktree_path"], values["integration_branch"], step(text, "c")
        self.assertTrue(self.renders_assignments(step_c, worktree))
        self.assertIn("fenced ```json block", (ROOT / "roles/quality-mgr.md").read_text())
        for reviewer in ("req-qa", "arch-qa", "plan-scope-reviewer"):
            with self.subTest(reviewer=reviewer):
                sent = self.assignment(reviewer, worktree_path=worktree, branch=branch)
                self.assertEqual((sent["worktree_path"], sent["branch"]), (worktree, branch))
        mention = required_match(re.search(r"Run `ceremony-qa` with (.*?)\. Run ", step_c, re.S)).group(1)
        sent = named_fields(mention)
        self.assertLessEqual(required_inputs("ceremony-qa.md"), set(sent))
        self.assertLessEqual(set(sent), set(inputs_json("ceremony-qa.md")))
        self.assertEqual((sent["review_mode"], sent["worktree_path"]), ("doc_review", worktree))
        # planted negatives: the old ceremony-qa mention lacks its required worktree, and the plan review no longer
        # renders without one
        old = named_fields("`review_mode` = `doc_review` and `review_targets` = every piped sprint container file")
        self.assertFalse(required_inputs("ceremony-qa.md") <= set(old))
        missing = {k: v for k, v in values.items() if k != "worktree_path"}
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "vars.json"
            path.write_text(json.dumps(missing))
            proc = rendering.render(TEMPLATES / "plan-review-template.xml.j2", path)
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("worktree_path", proc.stderr)

    def test_fix_round_reviewers_get_their_assignments_with_the_worktree(self):
        values, text = self.plan("plan-review-template-fix-round-vars.json")
        worktree, step_c = values["worktree_path"], step(text, "c")
        self.assertIn("fix-round-scope owned --plan", step_c)
        self.assertTrue(self.renders_assignments(step_c, worktree))
        with tempfile.TemporaryDirectory() as tmp:  # the reviewers step c dispatches: what `fix-round-scope owned --plan` prints
            carried = Path(tmp) / "carried.txt"
            carried.write_text(re.sub(r"\{\{[^}]*\}\}", "x", values["carry_forward"]))  # an install-time prefix
            proc = run_command([sys.executable, str(ROOT / "scripts/fix-round-scope"), "owned", "--plan", "--carried",
                                   str(carried)], capture_output=True, text=True)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        owned = {k: v for k, v in json.loads(proc.stdout).items() if k != "validate-plan"}  # step b re-checks those
        self.assertEqual(set(owned), {"arch-qa"})
        for reviewer, ids in owned.items():
            sent = self.assignment(reviewer, worktree_path=worktree, branch=values["integration_branch"],
                                   carry_forward_findings_json=json.dumps(ids))
            self.assertEqual((sent["worktree_path"], sent["carry_forward_findings"]), (worktree, ids))
            self.assertIn("SCOPE LOCK", sent["notes"])
        # planted negative: the fix-round step c without the rendering instruction is rejected
        self.assertFalse(self.renders_assignments(step_c.replace(RENDER_ASSIGNMENTS, ""), worktree))

    def test_ceremony_finding_screen_gets_its_declared_fenced_inputs(self):
        values, plan = self.plan("plan-review-template-vars.json")
        qa = render("qa-template.xml.j2", EXAMPLES / "qa-template-vars.json")
        qa_values = json.loads((EXAMPLES / "qa-template-vars.json").read_text())
        for name, instruction, worktree in (("plan-review d", step(plan, "d"), values["worktree_path"]),
                                            ("qa f", step(qa, "f"), qa_values["worktree_path"])):
            with self.subTest(step=name):
                self.assertIn("with the fenced-JSON input its contract names", instruction)
                sent = named_fields(instruction)
                self.assertLessEqual(CEREMONY_FINDING_SCREEN_REQUIRED, set(sent))
                self.assertEqual(sent["worktree_path"], worktree)
        # planted negative: the old plan step d names no inputs
        old = named_fields("Run `ceremony-finding-screen` over this round's findings, except the step b findings.")
        self.assertFalse(CEREMONY_FINDING_SCREEN_REQUIRED <= set(old))


def blocking_shape(rendered: str) -> str:
    """The `<scratch>/<qa bead>-blocking.json` content step g of a rendered qa-template tells quality-mgr to write."""
    step = rendered[rendered.index('<step id="g">'):rendered.index('<step id="h">')]
    return required_match(re.search(r"-blocking\.json` = `(\{.*?\})` and run", step, re.S)).group(1)


class BlockingFindingsHandoffTests(unittest.TestCase):
    """Rendered QA JSON is accepted by the in-process bead-groups receiver."""

    def setUp(self):
        from test_bead_pour_plan import PlanFixture
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.ws = PlanFixture(Path(temporary.name))

    def received(self, sprint, path):
        from test_bead_pour_plan import groups
        phase = self.ws.rows[sprint]["metadata"]["phase"]
        with patch.object(groups.sprint_index_common, "load_phase_config", return_value={"root": f"t-phase-{phase}"}):
            return self.ws.runner.finding_targets(path)[0]

    def written(self, sprint: str, qa: str, severity: str, found_on_layer=None) -> Path:
        values = json.loads((EXAMPLES / "qa-template-fix-round-vars.json").read_text())
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "vars.json"
            path.write_text(json.dumps({**values, "task_id": qa}))
            shape = blocking_shape(render("qa-template.xml.j2", path))
        filled = {"<the fix bead's metadata.sprint_bead>": json.dumps(sprint), "<its metadata.round + 1>": "2",
                  "<its metadata.finding_ref>": '"qa1-f1"', "<its metadata.reviewer>": '"arch-qa"',
                  '"blocking"': json.dumps(severity), "<what is still wrong>": "retry still unbounded",
                  "<remedy, with the evidence at ": "bound the retry, see src/lib.rs:3 at "}
        for placeholder, value in filled.items():
            self.assertIn(placeholder, shape)
            shape = shape.replace(placeholder, value)
        out = self.ws.root / f"{sprint}-{severity}-blocking.json"
        document = json.loads(shape)
        if found_on_layer is not None:
            document["findings"][0]["found_on_layer"] = found_on_layer
        out.write_text(json.dumps(document))
        return out

    def test_the_fix_round_shape_is_accepted_and_a_non_blocking_one_is_refused(self):
        ws = self.ws
        sprint = ws.sprint("hz", 1)
        qa = f"{sprint}.group-qa"
        result = self.received(sprint, self.written(sprint, qa, "blocking", found_on_layer=15))
        self.assertEqual(result["compose"]["reviewer"], "arch-qa")
        self.assertEqual(result["compose"]["found_on_layer"], 15)
        self.assertEqual(result["compose"]["ref"], "qa1-f1-r2")
        refused = self.received(sprint, self.written(sprint, qa, "important"))
        self.assertEqual(refused.code, "BEAD_GROUPS_NOT_BLOCKING")


    def round_one(self, sprint: str, qa: str, **finding) -> Path:
        """Step g's round-1 shape, rendered, parsed as it stands (its placeholders are JSON strings) and filled."""
        values = json.loads((EXAMPLES / "qa-template-vars.json").read_text())
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "vars.json"
            path.write_text(json.dumps({**values, "task_id": qa}))
            sent = json.loads(blocking_shape(render("qa-template.xml.j2", path)))
        self.assertEqual(sent["filed_by"], qa)
        filled = {"ref": "qa1-f1", "reviewer": "arch-qa", "title": "retry unbounded", "remedy": "bound the retry"}
        sent["sprint"], row = sprint, sent["findings"][0]
        self.assertEqual(set(row), {"ref", "severity", "reviewer", "title", "remedy", "priority", "difficulty"})
        row.update(filled)
        del row["difficulty"]  # optional: omitted, the sprint's applies
        row.update(finding)
        out = self.ws.root / f"{sprint}-{len(list(self.ws.root.glob('*-round1-*')))}-round1-blocking.json"
        out.write_text(json.dumps({**sent, "findings": [{k: v for k, v in row.items() if v is not None}]}))
        return out

    def test_the_round_one_shape_is_accepted_and_one_without_its_remedy_is_refused(self):
        ws = self.ws
        sprint = ws.sprint("hx", 1)
        qa = f"{sprint}.group-qa"
        target = self.received(sprint, self.round_one(sprint, qa, found_on_layer=15))
        self.assertEqual(target["compose"]["found_on_layer"], 15)
        self.assertEqual({key: target["compose"][key] for key in ("reviewer", "difficulty")},
                         {"reviewer": "arch-qa", "difficulty": "normal"})
        target = self.received(sprint, self.round_one(sprint, qa, ref="qa1-f2", difficulty="hard"))
        self.assertEqual(target["compose"]["difficulty"], "hard")
        self.assertFalse(target["compose"]["has_found_on_layer"])
        # planted negatives: the old key-list shape is not JSON, and a finding without its remedy is refused
        with self.assertRaises(ValueError):
            json.loads('{"findings": [{"ref": "qa1-f1", "severity": "blocking", "reviewer", "title", "remedy", '
                       '"priority": 1, optional "difficulty"}]}')
        refused = self.received(sprint, self.round_one(sprint, qa, ref="qa1-f3", remedy=None))
        self.assertEqual(refused.code, "BEAD_GROUPS_FINDINGS_INVALID")
        self.assertIn("remedy", str(refused))

    def test_a_workflow_issue_jsonl_has_its_parent_and_requires_its_remedy(self):
        ws = self.ws
        parent = ws.sprint("hy", 1)
        values = {**json.loads((EXAMPLES / "workflow-issue-bead-vars.json").read_text()), "id": "t-wf-jev-outage", "parent": parent}
        path = ws.root / "workflow-vars.json"
        for case, sent, ok in (("documented", values, True),
                               ("planted negative", {k: v for k, v in values.items() if k != "remedy"}, False)):
            with self.subTest(case=case):
                path.write_text(json.dumps(sent))
                proc = rendering.render(TEMPLATES / "workflow-issue-bead.json.j2", path)
                self.assertEqual(proc.returncode == 0, ok, proc.stderr)
        rows = ws.root / "workflow.jsonl"
        path.write_text(json.dumps(values))
        rows.write_text(json.dumps(json.loads(render("workflow-issue-bead.json.j2", path))) + "\n")
        bead = json.loads(rows.read_text())
        self.assertIn("workflow-issue", bead["labels"])
        self.assertEqual(bead["dependencies"], [{"issue_id": "t-wf-jev-outage", "depends_on_id": parent, "type": "parent-child"}])


if __name__ == "__main__":
    unittest.main()
