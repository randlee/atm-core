"""Exercise the installed sc-compose -> typed JSON -> locked append pipeline."""
import concurrent.futures
import importlib.machinery
import importlib.util
import json
import os
import shutil
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

from jev_receipts import CLIENT, env as receipt_env, receipted

ROOT = Path(__file__).parents[2]
SCRIPTS = ROOT / "scripts"
LOADER = importlib.machinery.SourceFileLoader("sanity_history", str(SCRIPTS / "sanity-run-history"))
HISTORY = importlib.util.module_from_spec(importlib.util.spec_from_loader(LOADER.name, LOADER))
LOADER.exec_module(HISTORY)
SPLIT_LOADER = importlib.machinery.SourceFileLoader("sanity_split_tests", str(Path(__file__).parent / "test_sanity_split.py"))
SPLIT_TESTS = importlib.util.module_from_spec(importlib.util.spec_from_loader(SPLIT_LOADER.name, SPLIT_LOADER))
SPLIT_LOADER.exec_module(SPLIT_TESTS)


def record(**changes):
    value = dict(run_id="run-one", reviewer="sanity-llm", commit="a" * 40,
                 task='task-"quote"\nnext', sprint="d-4", phase="d", started_at="2026-09-30T16:00:00Z",
                 completed_at="2026-09-30T16:01:05Z", duration="1m05s", duration_seconds=65,
                 pr_number=42, iteration=1, verdict="PASS", final_verdict="PASS", findings=0,
                 error=None, errors=[], jev_receipts=[], completed_local="09-30 09:01")
    return dict(value, **changes)


def jev_reply(number, findings=()):
    return receipted({"success": True, "error": None, "data": {
        "sanity_bead": "s", "dev_bead": "d", "deliverable": number, "commit_checked": "a" * 40, "findings": list(findings)}})


RECEIPT = jev_reply(1)["data"]["jev"]


class SanityHistory(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.log = self.root / "phase-d.jsonl"

    def test_real_render_compact_append_escaping_types_and_retry(self):
        original = record()
        rendered = HISTORY.render_record(original)
        self.assertEqual(rendered, original)
        HISTORY.append_record(self.log, rendered)
        HISTORY.append_record(self.log, rendered)
        lines = self.log.read_text().splitlines()
        self.assertEqual(len(lines), 1)
        self.assertEqual(json.loads(lines[0]), original)
        self.assertNotIn(': ', lines[0])
        self.assertIs(type(rendered["findings"]), int)
        unavailable = record(reviewer="sanity-jev", verdict="CANNOT_RUN", findings=None,
                             error={"code": "JEV.UNAVAILABLE", "message": 'failed "transport"\nretry'})
        HISTORY.append_record(self.log, HISTORY.render_record(unavailable))
        self.assertEqual(json.loads(self.log.read_text().splitlines()[1]), unavailable)

    def test_real_render_or_validation_failure_leaves_log_unchanged(self):
        HISTORY.append_record(self.log, HISTORY.render_record(record()))
        before = self.log.read_bytes()
        missing = record()
        del missing["reviewer"]
        missing_final = record()
        del missing_final["final_verdict"]
        for invalid in (missing, missing_final, record(pr_number="42"), record(completed_at="2026-09-30T09:01:05-07:00")):
            with self.subTest(invalid=invalid), self.assertRaises(SystemExit):
                HISTORY.append_record(self.log, HISTORY.render_record(invalid))
            self.assertEqual(self.log.read_bytes(), before)
        template = self.root / "invalid.json.j2"
        template.write_text('---\nname: invalid\nformat: json\n---\n{broken json')
        with patch.object(HISTORY, "RECORD_TEMPLATE", template), self.assertRaises(SystemExit):
            HISTORY.append_record(self.log, HISTORY.render_record(record()))
        self.assertEqual(self.log.read_bytes(), before)

    def test_assignment_selection_renders_without_legacy_reviewers(self):
        variables = json.loads((SCRIPTS.parent / "examples/dev-sanity-template-vars.json").read_text())
        var_file = self.root / "vars.json"
        var_file.write_text(json.dumps(variables))
        rendered = subprocess.run(["sc-compose", "render", "--strict", "--file",
                                   str(SCRIPTS.parent / "templates/dev-sanity-template.xml.j2"),
                                   "--var-file", str(var_file)], capture_output=True, text=True)
        self.assertEqual(rendered.returncode, 0, rendered.stderr)
        self.assertNotIn("<reviewers>", rendered.stdout)
        self.assertIn("canonical coordinator", rendered.stdout)
        self.assertIn("--llm-vars <sanity-llm-vars.json>", rendered.stdout)
        self.assertIn("send identical assignments to both reviewers concurrently", rendered.stdout)

    def test_conflicting_identity_or_truncated_log_is_not_appended(self):
        HISTORY.append_record(self.log, record())
        before = self.log.read_bytes()
        for conflict in (record(commit="b" * 40, reviewer="sanity-jev", jev_receipts=[RECEIPT]), record(findings=1, verdict="FAIL")):
            with self.assertRaises(SystemExit):
                HISTORY.append_record(self.log, conflict)
            self.assertEqual(self.log.read_bytes(), before)
        self.log.write_bytes(before + b'{"truncated"')
        with self.assertRaises(SystemExit):
            HISTORY.append_record(self.log, record(run_id="new"))
        self.assertEqual(self.log.read_bytes(), before + b'{"truncated"')

    def test_cli_real_render_concurrent_appends(self):
        # Real separate writer processes and installed sc-compose. The only fake
        # external dependency is bd (absent bead -> documented sprint fallback).
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        bd = bin_dir / "bd"
        bd.write_text("#!/bin/sh\nexit 1\n")
        bd.chmod(0o755)
        env = dict(os.environ, PATH=str(bin_dir) + os.pathsep + os.environ["PATH"])

        def run(index):
            values = record(run_id=f"run-{index // 2}", reviewer="sanity-llm" if index % 2 == 0 else "sanity-jev")
            values["reviewer_results"] = [jev_reply(1)]
            del values["jev_receipts"]
            values["findings_count"] = values.pop("findings")
            del values["completed_local"]
            var_file = self.root / f"vars-{index}.json"
            var_file.write_text(json.dumps(values))
            return subprocess.run([str(SCRIPTS / "sanity-run-history"), "--vars", str(var_file),
                                   "--task", values["task"], "--bead", "dev-d-4", "--pr-number", "42",
                                   "--iteration", "1", "--final-verdict", "PASS"],
                                  cwd=self.root, env=env, capture_output=True, text=True)
        with concurrent.futures.ThreadPoolExecutor(max_workers=6) as executor:
            results = list(executor.map(run, range(12)))
        for result in results:
            self.assertEqual(result.returncode, 0, result.stderr)
        log = self.root / ".sc/sanity-log/phase-d.jsonl"
        rows = [json.loads(line) for line in log.read_text().splitlines()]
        self.assertEqual(len(rows), 12)
        self.assertEqual(len({(r["run_id"], r["reviewer"]) for r in rows}), 12)
        self.assertTrue(all(r["completed_local"] for r in rows))
        result = run(0)  # idempotent retry
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(log.read_text().splitlines()), 12)

    def test_retry_under_another_time_zone_is_the_same_row(self):
        HISTORY.append_record(self.log, record())
        HISTORY.append_record(self.log, record(completed_local="09-30 18:01"))
        self.assertEqual(len(self.log.read_text().splitlines()), 1)

    def test_retry_of_a_row_logged_before_errors_existed_is_the_same_row(self):
        failed = record(errors=[{"code": "SANITY.JEV_UNAVAILABLE", "message": "Jev retry budget exhausted",
                                 "recoverable": True, "deliverable": 1}])
        legacy = {key: value for key, value in failed.items() if key != "errors"}
        self.log.write_text(json.dumps(legacy) + "\n")
        HISTORY.append_record(self.log, HISTORY.render_record(failed))
        self.assertEqual([json.loads(line) for line in self.log.read_text().splitlines()], [legacy])
        with self.assertRaises(SystemExit):
            HISTORY.append_record(self.log, record(errors=failed["errors"], findings=1, verdict="FAIL"))

    def test_jev_receipts_are_required_on_a_jev_verdict_and_absent_on_llm(self):
        vars = {"reviewer": "sanity-jev", "reviewer_results": [jev_reply(1), jev_reply(2, [{"kind": "skipped"}]),
                                                               {"success": False, "data": None, "error": {}}]}
        self.assertEqual(HISTORY.jev_receipts(vars), [jev_reply(1)["data"]["jev"], jev_reply(2, [{"kind": "skipped"}])["data"]["jev"]])
        self.assertEqual(HISTORY.jev_receipts(dict(vars, reviewer="sanity-llm")), [])
        jev = record(reviewer="sanity-jev", jev_receipts=[RECEIPT])
        self.assertEqual(HISTORY.render_record(jev), jev)
        for invalid in (record(reviewer="sanity-jev"), record(reviewer="sanity-jev", verdict="FAIL", findings=1),
                        record(jev_receipts=[RECEIPT]), record(reviewer="sanity-jev", jev_receipts=[None])):
            with self.subTest(invalid=invalid), self.assertRaises(SystemExit):
                HISTORY.render_record(invalid)
        unavailable = record(reviewer="sanity-jev", verdict="CANNOT_RUN", findings=None,
                             error={"code": "SANITY.JEV_UNAVAILABLE", "message": "Jev retry budget exhausted"})
        self.assertEqual(HISTORY.render_record(unavailable), unavailable)

    def test_retry_of_a_row_logged_before_jev_receipts_existed_is_the_same_row(self):
        jev = record(reviewer="sanity-jev", jev_receipts=[RECEIPT])
        legacy = {key: value for key, value in jev.items() if key not in ("jev_receipts", "errors")}
        self.log.write_text(json.dumps(legacy) + "\n")
        HISTORY.append_record(self.log, jev)
        self.assertEqual([json.loads(line) for line in self.log.read_text().splitlines()], [legacy])

    def test_malformed_failure_envelopes_never_block_the_cannot_run_row(self):
        error = {"code": "SANITY.RESULT_INVALID", "message": "reply had no fence", "recoverable": False}
        vars = {"reviewer_results": [
            {"success": False, "data": None, "error": error},
            {"success": False, "data": None, "error": {"message": "no code"}},
            {"success": False, "data": None, "error": dict(error, deliverable=2)}]}
        errors = HISTORY.slot_errors(vars)
        self.assertEqual(errors, [dict(error, deliverable=None), dict(error, deliverable=2)])
        row = record(reviewer="sanity-jev", verdict="CANNOT_RUN", final_verdict="PASS", findings=None,
                     error={"code": "SANITY.RESULT_INVALID", "message": "deliverable 1 envelope invalid"}, errors=errors)
        HISTORY.append_record(self.log, HISTORY.render_record(row))
        self.assertEqual(json.loads(self.log.read_text()), row)

    def test_table_skips_pre_0_8_2_rows(self):
        legacy = {key: value for key, value in record().items() if key not in ("completed_local", "final_verdict")}
        runs = [dict(legacy, reviewer="sanity-selected"), legacy]
        table = subprocess.run(["sc-compose", "render", "--strict", "--root", str(SCRIPTS.parent),
                                "--file", str(SCRIPTS.parent / "templates/sanity-run-table.md.j2"),
                                "--var-file", "/dev/stdin"],
                               input=json.dumps({"runs": runs}), capture_output=True, text=True)
        self.assertEqual(table.returncode, 0, table.stderr)
        self.assertEqual(table.stdout.splitlines()[2:],
                         ["| d-4 | #42 | LLM | 0 | ✅ | — | 2026-09-30T16:01:05Z · 1m05s | 1 |"])


CONSOLE = (  # agents/dev-sanity.md "Mandatory Console Report"
    "set -o pipefail\n"
    'test -s "$log" && tail -n 20 "$log" | jq -s \'{runs: .}\' | sc-compose render --strict '
    "--file .claude/skills/atm-bd-orchestration/templates/sanity-run-table.md.j2 --var-file /dev/stdin")


def fenced(task, bead, number, sha, findings=()):
    return "```json\n" + json.dumps(receipted({"success": True, "data": {
        "deliverable": number, "sanity_bead": task, "dev_bead": bead, "commit_checked": sha,
        "findings": list(findings)}, "error": None}), indent=2) + "\n```"


class ShippedFlow(unittest.TestCase):
    """Fenced replies -> sanity-split manifest -> sanity-merge (LLM, JEV, selected) ->
    sanity-run-history -> the console command, all shipped scripts on a real pushed repo."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = SPLIT_TESTS.Repo(self.root)
        with (self.repo.wt / ".git/info/exclude").open("a") as exclude:
            exclude.write(".sc/\n")
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        (bin_dir / "bd").write_text("#!/bin/sh\nexit 1\n")  # absent bead -> phase from the sprint
        (bin_dir / "bd").chmod(0o755)
        self.env = dict(os.environ, PATH=str(bin_dir) + os.pathsep + os.environ["PATH"])
        self.primary = self.root / "primary"  # where the console command runs: .claude/skills/... resolves
        shutil.copytree(ROOT / "templates", self.primary / ".claude/skills/atm-bd-orchestration/templates")
        bead = SPLIT_TESTS.bead()
        self.bead, self.task = bead[0]["id"], "t-1-sanity"
        (self.root / "bead.json").write_text(json.dumps(bead))
        split = subprocess.run([str(SCRIPTS / "sanity-split"), "--task", self.task, "--bead", self.bead,
                                "--worktree", str(self.repo.wt), "--branch", "sprint/x", "--commit", self.repo.sha,
                                "--base", "develop", "--lint-command", "true", "--scratch", str(self.root / "scratch"),
                                "--bead-json", str(self.root / "bead.json")], capture_output=True, text=True)
        self.assertEqual(split.returncode, 0, split.stderr)
        self.manifest = self.root / "manifest.json"
        self.manifest.write_text(split.stdout)
        exit_file = Path(json.loads(split.stdout)["lint"]["exit_file"])
        SPLIT_TESTS.wait_for(exit_file.exists, f"exit file {exit_file.name}")
        self.assertEqual(exit_file.read_text().strip(), "0")

    def merge(self, reviewer, *extra, stdin=None):
        now = time.time()
        return subprocess.run([str(SCRIPTS / "sanity-merge"), str(self.manifest), self.task, self.bead, "d-4",
                               "--reviewer", reviewer, "--started-at", str(now - 60), "--completed-at", str(now),
                               *extra, "--client", str(CLIENT)], input=stdin, capture_output=True, text=True, env=receipt_env())

    def run_flow(self, select, jev_failure=None):
        sha = self.repo.sha
        skipped = {"kind": "skipped", "file": "crates/types/src/retry.rs", "line": 1, "issue": "404 test not written"}
        replies = {"sanity-llm": [fenced(self.task, self.bead, 1, sha), fenced(self.task, self.bead, 2, sha)],
                   "sanity-jev": [fenced(self.task, self.bead, 1, sha),
                                  jev_failure or fenced(self.task, self.bead, 2, sha, [skipped])]}
        reviewer_vars = {}
        for reviewer, texts in replies.items():
            merged = self.merge(reviewer, stdin=json.dumps(texts))  # reply text kept unchanged as strings
            self.assertEqual(merged.returncode, 3 if reviewer == "sanity-jev" and jev_failure else 0, merged.stderr)
            reviewer_vars[reviewer] = self.root / f"{reviewer}-vars.json"
            reviewer_vars[reviewer].write_text(merged.stdout)
        digests = {r: json.loads(v.read_text())["reviewer_results_sha256"] for r, v in reviewer_vars.items()}
        selection = [{"deliverable": n, "llm": "done", "jev": "done" if n == 1 else "undone", "selected": "llm",
                      "reason": "" if n == 1 else "retry.rs tests cover 404", "rerun": None, "checker_defect": False,
                      "llm_sha256": digests["sanity-llm"], "jev_sha256": digests["sanity-jev"]} for n in (1, 2)]
        select(selection)
        (self.root / "selection.json").write_text(json.dumps(selection))
        selected = self.merge("sanity-selected", "--llm-vars", str(reviewer_vars["sanity-llm"]),
                              "--jev-vars", str(reviewer_vars["sanity-jev"]),
                              "--selection", str(self.root / "selection.json"))
        final = json.loads(selected.stdout)["verdict"]
        logs = set()
        for reviewer in ("sanity-llm", "sanity-jev"):
            history = subprocess.run([str(SCRIPTS / "sanity-run-history"), "--vars", str(reviewer_vars[reviewer]),
                                      "--task", self.task, "--bead", self.bead, "--pr-number", "42",
                                      "--iteration", "1", "--final-verdict", final],
                                     cwd=self.repo.wt, env=self.env, capture_output=True, text=True)
            self.assertEqual(history.returncode, 0, history.stderr)
            logs.add(history.stdout.strip())
        self.assertEqual(len(logs), 1)
        log = logs.pop()
        self.assertEqual(Path(log), self.repo.wt.resolve() / ".sc/sanity-log/phase-d.jsonl")
        rows = [json.loads(line) for line in Path(log).read_text().splitlines()]
        table = subprocess.run(["bash", "-c", CONSOLE], cwd=self.primary, env=dict(self.env, log=log),
                               capture_output=True, text=True)
        self.assertEqual(table.returncode, 0, table.stderr)
        return final, rows, table.stdout, log

    def expected_table(self, rows, cells):
        return ("| S | PR | R | Find | Result | Match | Done | Iter |\n"
                "|:--|:--|:--|---:|:--|:--|:--|---:|\n" + "".join(
                    f"| d-4 | #42 | {cell} | {row['completed_local']} · {row['duration']} | 1 |\n"
                    for row, cell in zip(rows, cells)))

    def test_selected_verdict_differs_from_one_reviewer(self):
        final, rows, table, _ = self.run_flow(lambda selection: None)
        self.assertEqual(final, "PASS")
        self.assertEqual([(r["reviewer"], r["verdict"], r["final_verdict"]) for r in rows],
                         [("sanity-llm", "PASS", "PASS"), ("sanity-jev", "FAIL", "PASS")])
        self.assertEqual(table, self.expected_table(rows, ["LLM | 0 | ✅ | ✓", "JEV | 1 | ❌ | ✗"]))

    def test_selected_merge_cannot_run_keeps_reviewer_verdicts(self):
        def invalid(selection):
            selection[1]["reason"] = ""  # a disagreement without a reason: selected merge rejects it
        final, rows, table, _ = self.run_flow(invalid)
        self.assertEqual(final, "CANNOT_RUN")
        self.assertEqual([(r["reviewer"], r["verdict"], r["final_verdict"]) for r in rows],
                         [("sanity-llm", "PASS", "CANNOT_RUN"), ("sanity-jev", "FAIL", "CANNOT_RUN")])
        self.assertEqual(table, self.expected_table(rows, ["LLM | 0 | ✅ | ✗", "JEV | 1 | ❌ | ✗"]))

    def test_failed_jev_slot_error_is_logged_verbatim_while_llm_is_selected(self):
        error = {"code": "SANITY.JEV_UNAVAILABLE", "message": "TYPESAFE_API_KEY is missing; no Jev evaluation ran",
                 "recoverable": True, "suggested_action": "set TYPESAFE_API_KEY", "deliverable": 2}
        failure = {"success": False, "data": None, "error": error}

        def take_llm(selection):
            selection[1].update(jev="cannot_run", reason="JEV unavailable: SANITY.JEV_UNAVAILABLE")
        final, rows, table, _ = self.run_flow(take_llm, jev_failure=failure)
        self.assertEqual(final, "PASS")
        self.assertEqual([(r["reviewer"], r["verdict"], r["errors"]) for r in rows], [
            ("sanity-llm", "PASS", []),
            ("sanity-jev", "CANNOT_RUN", [{k: error[k] for k in ("code", "message", "recoverable", "deliverable")}])])
        self.assertEqual(table, self.expected_table(rows, ["LLM | 0 | ✅ | ✓", "JEV | — | ⚠ unavailable | ✗"]))

    def test_all_ok_rows_log_no_errors(self):
        _, rows, _, _ = self.run_flow(lambda selection: None)
        self.assertEqual([r["errors"] for r in rows], [[], []])
        jev = [json.loads(text.split("\n", 1)[1].rsplit("\n", 1)[0])["data"]["jev"] for text in (
            fenced(self.task, self.bead, 1, self.repo.sha),
            fenced(self.task, self.bead, 2, self.repo.sha, [{"kind": "skipped", "file": "crates/types/src/retry.rs", "line": 1, "issue": "404 test not written"}]))]
        self.assertEqual([r["jev_receipts"] for r in rows], [[], jev])

    def test_console_fails_on_a_wrong_ledger_path(self):
        doc = (ROOT.parents[1] / "agents/dev-sanity.md").read_text()
        self.assertIn(CONSOLE.replace("--strict --file", "--strict \\\n  --file"), doc)
        *_, log = self.run_flow(lambda selection: None)
        for wrong in (log + ".missing", str(self.root / "empty.jsonl")):
            Path(self.root / "empty.jsonl").touch()
            table = subprocess.run(["bash", "-c", CONSOLE], cwd=self.primary, env=dict(self.env, log=wrong),
                                   capture_output=True, text=True)
            self.assertNotEqual(table.returncode, 0, table.stdout)
